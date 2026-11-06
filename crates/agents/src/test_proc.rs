//! 测试辅助（仅 Unix）：判断孙进程是否已经结束。

use std::path::Path;
use std::time::{Duration, Instant};

/// 进程是否还在运行。不存在或已是僵尸进程（Z）都算已结束：
/// 被杀掉的孙进程会被 init 收养，容器里的 init 不一定及时回收。
pub fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // 格式：pid (comm) state ...；comm 可能带空格，取最后一个 ')' 之后的字段
        Ok(stat) => stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next()).is_some_and(|state| state != "Z"),
        Err(_) => false,
    }
}

/// 最多等 `secs` 秒，直到进程结束；返回是否已结束
pub fn wait_dead(pid: u32, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !alive(pid)
}

/// 读出脚本写下的孙进程 pid，最多等 `secs` 秒
pub fn read_pid(file: &Path, secs: u64) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(pid) = std::fs::read_to_string(file).ok().and_then(|s| s.trim().parse().ok()) {
            return pid;
        }
        assert!(Instant::now() < deadline, "没有等到 pid 文件 {}", file.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 测试结束时兜底清理，避免 `sleep 300` 留在机器上
pub fn force_kill(pid: u32) {
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
}
