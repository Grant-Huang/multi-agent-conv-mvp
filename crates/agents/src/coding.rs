//! Coding Agent：外部 Harness 适配器，以子进程方式调用命令行编码 agent（非交互模式），
//! 默认是 Codex CLI，也可以配置成 pi（`pi -p`）等任何"传入任务、执行完退出"的命令。
//!
//! 设计要点：
//! - 命令和参数完全由配置决定，参数里的占位符会被替换：
//!   `{prompt}` 任务描述、`{workdir}` 工作目录、`{last_message_file}` 最后一条回复的输出文件；
//!   不同版本 Codex 的参数可能不同，请用 `codex exec --help` 核对后修改配置；
//! - 卡片的 id / 名称 / 类型可在配置中指定，默认是 codex / Codex CLI / codex；
//! - 子进程放在独立的进程组里（[`crate::proc_group`]）：取消、超时、正常结束时，
//!   连同它启动的孙进程一起结束，不会残留；
//! - 执行后在工作目录跑 `git diff`，把改动作为产物和证据交给 Reviewer。
//!   "Codex 说改好了"不等于"改对了"，验收要看证据。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use an_core::{
    codes, truncate_chars, AgentAdapter, AgentCard, AgentError, EventSink, Evidence, Role,
    TaskResult, TaskSpec, WorkerStatus,
};

use crate::proc_group::{self, GroupChild};
use crate::util::{collect_git_changes, expand_tilde, resolve_command, snapshot_worktree};
use an_llm::Backend;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Coding Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingConfig {
    /// 可执行文件，默认 `codex`
    #[serde(default = "default_command")]
    pub command: String,
    /// 参数模板，支持 `{prompt}` `{workdir}` `{last_message_file}` 占位符
    #[serde(default = "default_args")]
    pub args: Vec<String>,
    /// 代码工作目录（建议指向一个独立的 git 仓库或 worktree）
    #[serde(default = "default_workdir")]
    pub workdir: PathBuf,
    /// 超时时间（秒）
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// 同时最多执行几个任务（多个任务共用一个工作目录时，建议保持 1）
    #[serde(default = "default_concurrency")]
    pub max_concurrent: u32,
    /// agent 的唯一标识（配置执行组 [agents.assign] 时使用），默认 `codex`
    #[serde(default = "default_id")]
    pub id: String,
    /// 显示名称，默认 `Codex CLI`
    #[serde(default = "default_name")]
    pub name: String,
    /// agent 类型（执行组只允许同类型互为备份），默认 `codex`
    #[serde(default = "default_id")]
    pub agent_type: String,
    /// 本地推理后端名（`[backends.<名>]`，必须是 `llama_cpp`）：agent 直连这个后端时填写。
    /// 每个任务启动前先向调度器申请一个后台槽位，槽位号通过环境变量 `AN_LLM_SLOT` 交给子进程，
    /// 由 agent 侧（pi 的扩展）写进请求的 `id_slot`，保证它的请求只落在后台槽位、不占前台。
    /// agent 用云端模型时不要配置
    #[serde(default)]
    pub backend: Option<String>,
}

/// 子进程里携带槽位号的环境变量名（agent 侧的适配读取它）
pub const SLOT_ENV: &str = "AN_LLM_SLOT";

fn default_concurrency() -> u32 {
    1
}
fn default_id() -> String {
    "codex".into()
}
fn default_name() -> String {
    "Codex CLI".into()
}

fn default_command() -> String {
    "codex".into()
}
fn default_args() -> Vec<String> {
    [
        "exec",
        "--skip-git-repo-check",
        "--sandbox",
        "workspace-write",
        "--cd",
        "{workdir}",
        "--output-last-message",
        "{last_message_file}",
        "{prompt}",
    ]
    .map(String::from)
    .to_vec()
}
fn default_workdir() -> PathBuf {
    PathBuf::from("workspace/code")
}
fn default_timeout() -> u64 {
    900
}

impl Default for CodingConfig {
    fn default() -> Self {
        Self {
            command: default_command(),
            args: default_args(),
            workdir: default_workdir(),
            timeout_secs: default_timeout(),
            max_concurrent: default_concurrency(),
            id: default_id(),
            name: default_name(),
            agent_type: default_id(),
            backend: None,
        }
    }
}

pub struct CodingAgent {
    cfg: CodingConfig,
    card: AgentCard,
    /// 固定槽位用的后端（`cfg.backend` 解析后由运行时注入）
    slot_backend: Option<Arc<Backend>>,
}

/// 保留的输出行数（用于摘要和证据）
const TAIL_LINES: usize = 40;

impl CodingAgent {
    /// 创建时检查命令是否可用；不可用时 Agent 仍然注册，但标记为 disabled，Router 不会选它
    pub fn new(mut cfg: CodingConfig) -> Self {
        cfg.workdir = PathBuf::from(expand_tilde(&cfg.workdir.to_string_lossy()));
        // 只检查命令是否存在，不运行它
        let (enabled, disabled_reason) = match resolve_command(&cfg.command) {
            Some(path) => {
                cfg.command = path.to_string_lossy().into_owned();
                (true, None)
            }
            None => (false, Some(format!("找不到命令 `{}`", cfg.command))),
        };
        let card = AgentCard {
            id: cfg.id.clone(),
            kind: "cli".into(),
            agent_type: cfg.agent_type.clone(),
            location: "本机".into(),
            max_concurrent: cfg.max_concurrent.max(1),
            role: Role::Coding,
            name: cfg.name.clone(),
            description: Role::Coding.description().into(),
            tools: vec![],
            enabled,
            disabled_reason,
        };
        Self {
            cfg,
            card,
            slot_backend: None,
        }
    }

    /// 连通性探测：运行 `命令 --version`（5 秒超时）。只确认命令能正常启动，不调用模型。
    /// 模型服务是否可用由运行时另行检查（pi 依赖的后端）。
    pub async fn probe(&self) -> std::result::Result<(), String> {
        if !self.card.enabled {
            return Err(self
                .card
                .disabled_reason
                .clone()
                .unwrap_or_else(|| "未安装".into()));
        }
        let run = tokio::process::Command::new(&self.cfg.command)
            .arg("--version")
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(Duration::from_secs(5), run).await {
            Ok(Ok(out)) if out.status.success() => Ok(()),
            Ok(Ok(out)) => Err(format!(
                "--version 退出码 {}",
                out.status.code().unwrap_or(-1)
            )),
            Ok(Err(e)) => Err(format!("无法运行：{e}")),
            Err(_) => Err("--version 超时（5 秒）".into()),
        }
    }

    /// 注入固定槽位用的后端：之后每个任务先申请后台槽位租约，再启动子进程
    pub fn with_slot_backend(mut self, backend: Arc<Backend>) -> Self {
        self.slot_backend = Some(backend);
        self
    }

    /// 替换参数模板中的占位符
    fn render_args(&self, prompt: &str, workdir: &str, last_msg: &str) -> Vec<String> {
        self.cfg
            .args
            .iter()
            .map(|a| {
                a.replace("{prompt}", prompt)
                    .replace("{workdir}", workdir)
                    .replace("{last_message_file}", last_msg)
            })
            .collect()
    }

    /// 把一行输出整理成适合展示的进度文本。
    /// 如果是 JSON（例如 codex 的 --json 模式），尽量提取类型和文本。
    fn describe_line(line: &str) -> String {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let ty = v["type"]
                .as_str()
                .or_else(|| v["msg"]["type"].as_str())
                .unwrap_or("event");
            let text = v["text"]
                .as_str()
                .or_else(|| v["msg"]["message"].as_str())
                .unwrap_or("");
            return truncate_chars(&format!("[{ty}] {text}"), 300);
        }
        truncate_chars(line, 300)
    }
}

/// 子进程标准输出的进度合并：满 20 行，或距第一条已经过了 200ms，才发一条进度。
const PROGRESS_LINES: usize = 20;
const PROGRESS_WAIT: Duration = Duration::from_millis(200);

struct ProgressBuf {
    lines: Vec<String>,
    since: tokio::time::Instant,
}

impl ProgressBuf {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            since: tokio::time::Instant::now(),
        }
    }

    fn pending(&self) -> bool {
        !self.lines.is_empty()
    }

    fn deadline(&self) -> tokio::time::Instant {
        self.since + PROGRESS_WAIT
    }

    /// 返回 Some 表示这一批该发出去了
    fn push(&mut self, line: String) -> Option<String> {
        if self.lines.is_empty() {
            self.since = tokio::time::Instant::now();
        }
        self.lines.push(line);
        if self.lines.len() >= PROGRESS_LINES || self.since.elapsed() >= PROGRESS_WAIT {
            Some(self.drain())
        } else {
            None
        }
    }

    fn drain(&mut self) -> String {
        let text = self.lines.join("\n");
        self.lines.clear();
        truncate_chars(&text, 2000)
    }
}

#[async_trait::async_trait]
impl AgentAdapter for CodingAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        if !self.card.enabled {
            return Err(AgentError::new(
                codes::AGENT_NOT_INSTALLED,
                self.card.disabled_reason.clone().unwrap_or_default(),
            )
            .into());
        }

        // 1. 准备工作目录和输出文件
        tokio::fs::create_dir_all(&self.cfg.workdir)
            .await
            .context("创建代码工作目录失败")?;
        let workdir = self.cfg.workdir.canonicalize()?;
        let last_msg_file = std::env::temp_dir().join(format!("an-codex-{}.txt", task.task_id));
        let _ = tokio::fs::remove_file(&last_msg_file).await;

        let prompt = if task.context.is_empty() {
            task.goal.clone()
        } else {
            format!("{}\n\n## 背景信息\n{}", task.goal, task.context)
        };
        let args = self.render_args(
            &prompt,
            &workdir.to_string_lossy(),
            &last_msg_file.to_string_lossy(),
        );
        let baseline = snapshot_worktree(&workdir).await;
        events.progress(format!(
            "启动 {}（工作目录 {}）",
            self.cfg.command,
            workdir.display()
        ));

        // 2. 固定槽位：先拿后台槽位租约（排队有期限，超时 E_LLM_BUSY），任务结束或被取消时随 `lease` 一起归还
        let lease = match &self.slot_backend {
            Some(b) => match b.lease_background("coding").await {
                Ok(l) => Some(l),
                Err(e) => return Ok(TaskResult::failed(e.to_string())), // 消息自带 `[E_LLM_BUSY]` 前缀
            },
            None => None,
        };
        let slot = lease.as_ref().and_then(|l| l.slot());
        if let Some(s) = slot {
            events.progress(format!("已分配本地模型槽位 {s}"));
        }

        // 3. 启动子进程
        let mut cmd = Command::new(&self.cfg.command);
        // 没有租约时也清掉继承来的值，避免旧值把请求钉到别人的槽位上
        match slot {
            Some(s) => cmd.env(SLOT_ENV, s.to_string()),
            None => cmd.env_remove(SLOT_ENV),
        };
        cmd.args(&args)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // 新进程组：取消 / 超时 / 结束时连同孙进程一起清理（见 proc_group）
        proc_group::configure(&mut cmd);
        let mut child = GroupChild::spawn(&mut cmd, &task.task_id).map_err(|e| {
            AgentError::new(
                codes::AGENT_SPAWN,
                format!("无法启动 {}：{e}", self.cfg.command),
            )
        })?;

        let mut stdout =
            BufReader::new(child.child_mut().stdout.take().expect("stdout piped")).lines();
        let mut stderr =
            BufReader::new(child.child_mut().stderr.take().expect("stderr piped")).lines();
        let mut out_tail: VecDeque<String> = VecDeque::new();
        let mut err_tail: VecDeque<String> = VecDeque::new();
        let push = |q: &mut VecDeque<String>, l: String| {
            if q.len() == TAIL_LINES {
                q.pop_front();
            }
            q.push_back(l);
        };

        // 4. 一边读输出（进度按 20 行或 200ms 合并成一条），一边等待进程结束，整体受超时控制。
        //    子进程退出后最多再读 DRAIN_AFTER_EXIT：孙进程可能继承了管道，不能等管道关闭
        let run = async {
            let mut progress = ProgressBuf::new();
            let (mut out_done, mut err_done) = (false, false);
            let mut exit = None;
            let mut drain_until = tokio::time::Instant::now();
            while !(out_done && err_done) {
                let deadline = progress.deadline();
                let pending = progress.pending();
                tokio::select! {
                    status = child.wait(), if exit.is_none() => {
                        exit = Some(status);
                        drain_until = tokio::time::Instant::now() + proc_group::DRAIN_AFTER_EXIT;
                    },
                    _ = tokio::time::sleep_until(drain_until), if exit.is_some() => break,
                    line = stdout.next_line(), if !out_done => match line {
                        Ok(Some(l)) => {
                            if let Some(msg) = progress.push(Self::describe_line(&l)) {
                                events.progress(msg);
                            }
                            push(&mut out_tail, l);
                        }
                        _ => out_done = true,
                    },
                    line = stderr.next_line(), if !err_done => match line {
                        Ok(Some(l)) => push(&mut err_tail, l),
                        _ => err_done = true,
                    },
                    _ = tokio::time::sleep_until(deadline), if pending => {
                        events.progress(progress.drain());
                    },
                }
            }
            if progress.pending() {
                events.progress(progress.drain());
            }
            match exit {
                Some(status) => status,
                None => child.wait().await,
            }
        };
        let outcome = tokio::time::timeout(Duration::from_secs(self.cfg.timeout_secs), run).await;
        let exit = match outcome {
            Ok(r) => {
                // 正常退出：清掉组里残留的孙进程（例如没关的 dev server）
                child.kill_leftovers();
                r.context("等待子进程失败")?
            }
            // 超时：先 SIGTERM 整个进程组，等一会儿再 SIGKILL
            Err(_) => {
                child.terminate("timeout").await;
                return Ok(TaskResult::failed_code(
                    codes::AGENT_TIMEOUT,
                    format!("{} 超时（{} 秒）", self.card.name, self.cfg.timeout_secs),
                ));
            }
        };

        // 5. 收集结果与证据
        let mut evidence = vec![Evidence {
            kind: "exit_code".into(),
            content: format!("{exit}"),
        }];
        let stdout_tail = out_tail.iter().cloned().collect::<Vec<_>>().join("\n");
        evidence.push(Evidence {
            kind: "stdout_tail".into(),
            content: truncate_chars(&stdout_tail, 3000),
        });
        if !exit.success() && !err_tail.is_empty() {
            let s = err_tail.iter().cloned().collect::<Vec<_>>().join("\n");
            evidence.push(Evidence {
                kind: "stderr_tail".into(),
                content: truncate_chars(&s, 3000),
            });
        }

        let (git_evidence, artifacts) = collect_git_changes(&workdir, baseline.as_deref()).await;
        evidence.extend(git_evidence);

        // 摘要优先使用 Codex 写出的最后一条回复；没有时用标准输出末尾（pi -p 只在标准输出打印最终回复）
        let summary = match tokio::fs::read_to_string(&last_msg_file).await {
            Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => stdout_tail.trim().to_string(),
        };
        let _ = tokio::fs::remove_file(&last_msg_file).await;

        Ok(TaskResult {
            status: if exit.success() {
                WorkerStatus::Succeeded
            } else {
                WorkerStatus::Failed
            },
            summary,
            artifacts,
            evidence,
            errors: if exit.success() {
                vec![]
            } else {
                vec![format!("[{}] 进程退出：{exit}", codes::AGENT_RUNTIME)]
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_card_is_codex() {
        let cfg: CodingConfig =
            serde_json::from_value(serde_json::json!({ "command": "sh" })).unwrap();
        let agent = CodingAgent::new(cfg);
        let card = agent.card();
        assert_eq!(
            (
                card.id.as_str(),
                card.agent_type.as_str(),
                card.name.as_str()
            ),
            ("codex", "codex", "Codex CLI")
        );
        assert!(card.enabled);
    }

    #[test]
    fn card_follows_config() {
        let cfg: CodingConfig = serde_json::from_value(serde_json::json!({
            "command": "sh",
            "id": "pi",
            "name": "pi（本地模型）",
            "agent_type": "pi"
        }))
        .unwrap();
        let agent = CodingAgent::new(cfg);
        let card = agent.card();
        assert_eq!(
            (
                card.id.as_str(),
                card.agent_type.as_str(),
                card.name.as_str()
            ),
            ("pi", "pi", "pi（本地模型）")
        );
        assert_eq!(card.role, Role::Coding);
    }

    // ───── 固定槽位：子进程拿到租约分配的槽位号 ─────

    struct NullPublisher;
    impl an_core::EventPublisher for NullPublisher {
        fn publish(&self, _: an_core::TaskEvent) {}
    }

    fn spec(id: &str) -> TaskSpec {
        use an_core::{Risk, RouteDecision, TaskClass};
        TaskSpec {
            task_id: id.into(),
            conversation_id: "c".into(),
            goal: "g".into(),
            context: String::new(),
            route: RouteDecision {
                task_class: TaskClass::L3,
                execution_role: Role::Coding,
                required_tools: vec![],
                risk_level: Risk::Low,
                reason: String::new(),
            },
        }
    }

    /// 子进程把收到的槽位环境变量打印出来
    fn echo_agent(tag: &str, sleep_secs: &str) -> CodingAgent {
        let dir = std::env::temp_dir().join(an_core::new_id(&format!("an-coding-{tag}")));
        let cfg: CodingConfig = serde_json::from_value(serde_json::json!({
            "command": "sh",
            "args": ["-c", format!("echo slot=[$AN_LLM_SLOT]; sleep {sleep_secs}")],
            "workdir": dir,
        }))
        .unwrap();
        CodingAgent::new(cfg)
    }

    fn sink() -> EventSink {
        EventSink::new(Arc::new(NullPublisher), "c", "t")
    }

    fn llama(front: &[u32], back: &[u32], queue_secs: Option<u64>) -> Arc<Backend> {
        use an_llm::{BackendConfig, BackendKind};
        Arc::new(Backend::new(
            "local",
            BackendConfig {
                kind: BackendKind::LlamaCpp,
                base_url: "http://x/v1".into(),
                slots: 6,
                interactive_slots: front.to_vec(),
                background_slots: back.to_vec(),
                queue_secs,
                ..Default::default()
            },
        ))
    }

    #[tokio::test]
    async fn child_gets_a_background_slot_via_env_and_it_is_returned_afterwards() {
        let b = llama(&[0, 1], &[2, 3, 4, 5], None);
        let agent = echo_agent("env", "0").with_slot_backend(b.clone());
        let r = agent.run(spec("t1"), sink()).await.unwrap();
        assert_eq!(r.status, WorkerStatus::Succeeded, "{r:?}");
        assert!(
            r.summary.contains("slot=[2]"),
            "应拿到第一个后台槽位：{}",
            r.summary
        );
        assert!(
            b.sched.snapshot().slots.iter().all(|s| s.holder.is_none()),
            "任务结束后槽位应已归还"
        );
    }

    #[tokio::test]
    async fn without_a_backend_the_slot_variable_is_not_set_even_if_inherited() {
        std::env::set_var(SLOT_ENV, "1");
        let r = echo_agent("none", "0")
            .run(spec("t2"), sink())
            .await
            .unwrap();
        std::env::remove_var(SLOT_ENV);
        assert!(r.summary.contains("slot=[]"), "{}", r.summary);
    }

    #[tokio::test]
    async fn concurrent_tasks_never_share_a_slot_and_extra_ones_wait_for_the_lease() {
        let b = llama(&[0], &[1, 2], Some(1));
        let agents: Vec<_> = (0..3)
            .map(|i| Arc::new(echo_agent(&format!("c{i}"), "2").with_slot_backend(b.clone())))
            .collect();
        let runs: Vec<_> = agents
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let a = a.clone();
                tokio::spawn(async move { a.run(spec(&format!("c{i}")), sink()).await.unwrap() })
            })
            .collect();
        let mut slots = vec![];
        let mut busy = 0;
        for h in runs {
            let r = h.await.unwrap();
            match r
                .summary
                .split(['[', ']'])
                .nth(1)
                .and_then(|s| s.parse::<u32>().ok())
            {
                Some(s) => slots.push(s),
                None => {
                    busy += 1;
                    assert!(
                        r.errors.iter().any(|e| e.contains("E_LLM_BUSY"))
                            || r.summary.contains("E_LLM_BUSY"),
                        "{r:?}"
                    );
                }
            }
        }
        slots.sort_unstable();
        assert_eq!(
            slots,
            vec![1, 2],
            "两个后台槽位各被一个任务占用，且不会落到前台槽位 0"
        );
        assert_eq!(busy, 1, "第三个任务等不到槽位，应明确报忙");
    }

    #[test]
    fn stdout_progress_merges_every_n_lines_or_after_the_wait() {
        let mut buf = ProgressBuf::new();
        for i in 0..19 {
            assert!(buf.push(format!("l{i}")).is_none());
        }
        let merged = buf.push("l19".into()).expect("满 20 行应合并成一条");
        assert_eq!(merged.lines().count(), 20);

        let mut buf = ProgressBuf::new();
        assert!(buf.push("a".into()).is_none());
        std::thread::sleep(Duration::from_millis(210));
        assert_eq!(buf.push("b".into()).as_deref(), Some("a\nb"));
    }

    // ───── 子进程清理：按进程组结束，孙进程不残留（仅 Unix） ─────

    /// 用 sh 冒充 agent：后台起一个 `sleep 300` 孙进程，pid 写到工作目录的 gc.pid
    #[cfg(unix)]
    fn grandchild_agent(tag: &str, script_tail: &str, timeout_secs: u64) -> (CodingAgent, PathBuf) {
        let dir = std::env::temp_dir().join(an_core::new_id(&format!("an-coding-{tag}")));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg: CodingConfig = serde_json::from_value(serde_json::json!({
            "command": "sh",
            "args": ["-c", format!("sleep 300 & echo $! > {{workdir}}/gc.pid; {script_tail}")],
            "workdir": dir,
            "timeout_secs": timeout_secs,
        }))
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        (CodingAgent::new(cfg), dir)
    }

    /// 超时：整个进程组被结束，孙进程不残留
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_grandchildren() {
        use crate::test_proc::*;
        let (agent, dir) = grandchild_agent("timeout", "wait", 1);
        let r = agent.run(spec("gc-timeout"), sink()).await.unwrap();
        assert!(r.errors.join("").contains(codes::AGENT_TIMEOUT), "{r:?}");
        let pid = read_pid(&dir.join("gc.pid"), 2);
        let dead = wait_dead(pid, 5);
        force_kill(pid);
        assert!(dead, "超时后孙进程 {pid} 还在运行");
    }

    /// 取消（future 被 drop）：整个进程组被结束
    // 多线程运行时：测试线程在等 pid 文件时，后台任务要能继续跑
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abort_kills_grandchildren() {
        use crate::test_proc::*;
        let (agent, dir) = grandchild_agent("abort", "wait", 60);
        let handle = tokio::spawn(async move { agent.run(spec("gc-abort"), sink()).await });
        let pid = read_pid(&dir.join("gc.pid"), 5);
        handle.abort();
        let _ = handle.await;
        let dead = wait_dead(pid, 5);
        force_kill(pid);
        assert!(dead, "取消后孙进程 {pid} 还在运行");
    }

    /// 子进程已经退出、孙进程还占着输出管道：任务要马上结束，残留的孙进程也要清理
    #[cfg(unix)]
    #[tokio::test]
    async fn exited_child_does_not_hang_on_inherited_pipe() {
        use crate::test_proc::*;
        let (agent, dir) = grandchild_agent("pipe", "echo done", 10);
        let started = std::time::Instant::now();
        let r = agent.run(spec("gc-pipe"), sink()).await.unwrap();
        let elapsed = started.elapsed();
        let pid = read_pid(&dir.join("gc.pid"), 2);
        let dead = wait_dead(pid, 5);
        force_kill(pid);
        assert!(
            elapsed < Duration::from_secs(5),
            "子进程退出后任务应尽快结束，实际用了 {elapsed:?}"
        );
        assert_eq!(r.status, WorkerStatus::Succeeded, "{r:?}");
        assert!(r.summary.contains("done"), "{}", r.summary);
        assert!(dead, "正常结束后残留的孙进程 {pid} 应被清理");
    }

    #[test]
    fn missing_command_keeps_configured_id() {
        let cfg: CodingConfig =
            serde_json::from_value(serde_json::json!({ "command": "no-such-cmd-xyz", "id": "pi" }))
                .unwrap();
        let card = CodingAgent::new(cfg).card().clone();
        assert_eq!(card.id, "pi");
        assert!(!card.enabled && card.disabled_reason.unwrap().contains("no-such-cmd-xyz"));
    }
}
