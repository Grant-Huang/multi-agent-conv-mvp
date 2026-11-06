//! 子进程按进程组管理：取消、超时、正常结束时，连同它启动的孙进程一起结束。
//!
//! 为什么需要：codex、pi 这类 agent 执行时还会再启动 shell、npm、测试进程。
//! `kill_on_drop` 只杀直接子进程，孙进程会残留，还可能继承输出管道，让读输出的循环一直等到超时。
//!
//! 做法（Unix）：启动时让子进程成为新进程组的组长（`process_group(0)`），结束时对整个组发信号：
//! - 取消（future 被 drop）：`Drop` 是同步的，不能等待，立即 SIGKILL 整个组（与 ACP SDK 一致）；
//! - 超时：先 SIGTERM 整个组，最多等 [`TERM_GRACE`]，再 SIGKILL；
//! - 正常退出：子进程回收后立即 SIGKILL 组内残留进程（例如 agent 起了没关的 dev server）。
//!
//! Windows 没有进程组，只能保留 `kill_on_drop`（只结束直接子进程），见 DESIGN「已知限制」。

use std::process::ExitStatus;
use std::time::Duration;

use tokio::process::{Child, Command};

/// 超时时 SIGTERM 之后等多久再 SIGKILL
pub const TERM_GRACE: Duration = Duration::from_secs(3);

/// 子进程退出后，最多再读这么久的剩余输出（孙进程可能还占着管道）
pub const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(500);

/// 让子进程成为新进程组的组长，并在 drop 时至少结束直接子进程
pub fn configure(cmd: &mut Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.kill_on_drop(true);
}

/// 已启动的子进程及其进程组。drop 时如果还没清理过，就 SIGKILL 整个组。
pub struct GroupChild {
    child: Child,
    /// 进程组 id（= 子进程 pid）；非 Unix 或拿不到 pid 时为 None
    pgid: Option<i32>,
    /// 已经清理过进程组：之后不再发信号（组 id 可能被系统复用）
    cleaned: bool,
    /// 日志里关联的任务（工具调用没有任务时为空）
    task_id: String,
}

impl GroupChild {
    /// 启动子进程。调用前需要先 [`configure`]
    pub fn spawn(cmd: &mut Command, task_id: &str) -> std::io::Result<Self> {
        let child = cmd.spawn()?;
        let pgid = if cfg!(unix) { child.id().and_then(|p| i32::try_from(p).ok()) } else { None };
        Ok(Self { child, pgid, cleaned: false, task_id: task_id.to_string() })
    }

    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// 等子进程退出（会回收子进程）。退出后应尽快调用 [`Self::kill_leftovers`]
    pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// 子进程已正常退出：结束组内残留的孙进程。
    /// 必须在回收子进程后立即调用：组里没有进程时组 id 会被释放，久了可能被别的进程复用。
    pub fn kill_leftovers(&mut self) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        if self.signal(Signal::Kill) {
            self.log("exited_with_leftovers");
        }
    }

    /// 超时：先 SIGTERM 整个组，最多等 [`TERM_GRACE`] 让子进程退出，再 SIGKILL 整个组
    pub async fn terminate(&mut self, reason: &'static str) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        if self.pgid.is_some() {
            self.signal(Signal::Term);
            let _ = tokio::time::timeout(TERM_GRACE, self.child.wait()).await;
            self.signal(Signal::Kill);
        }
        // 非 Unix，或进程组信号没生效：至少结束直接子进程
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(TERM_GRACE, self.child.wait()).await;
        self.log(reason);
    }

    /// 给整个进程组发信号；返回 true 表示组里还有进程（信号送达）
    fn signal(&self, sig: Signal) -> bool {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            let signo = match sig {
                Signal::Term => libc::SIGTERM,
                Signal::Kill => libc::SIGKILL,
            };
            // SAFETY: killpg 只发送信号，不访问内存；组不存在时返回 -1（ESRCH），忽略即可
            return unsafe { libc::killpg(pgid, signo) } == 0;
        }
        let _ = sig;
        false
    }

    fn log(&self, reason: &'static str) {
        tracing::info!(event = "agent.process_group_killed", task_id = %self.task_id, reason, pgid = self.pgid, "已结束子进程及其进程组");
    }
}

impl Drop for GroupChild {
    /// 取消路径（future 被 drop）：不能等待，立即 SIGKILL 整个组
    fn drop(&mut self) {
        if !self.cleaned {
            self.cleaned = true;
            if self.signal(Signal::Kill) {
                self.log("cancelled");
            }
        }
        // 直接子进程另由 kill_on_drop 结束，tokio 会在后台回收
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}
