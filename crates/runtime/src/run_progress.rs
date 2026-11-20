//! 长任务执行时的进度心跳：编排层定时推送，避免界面长时间无更新。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use an_core::EventSink;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

/// 两次心跳之间的间隔（秒）
pub const HEARTBEAT_INTERVAL_SECS: u64 = 50;
/// 达到任务期限的多少比例时提示即将超时（只提示一次）
const TIMEOUT_WARN_FRACTION: f64 = 0.85;

/// 单轮执行的阶段说明（写入进度文案）
pub fn run_phase_label(round: u32) -> &'static str {
    if round == 0 {
        "执行中"
    } else {
        "返工执行中"
    }
}

/// 编排器心跳正文：已运行时长 + 执行者 + 阶段
pub fn format_run_heartbeat(elapsed: Duration, executor: &str, phase: &str) -> String {
    format!("仍在{}（{}，已 {} 秒）", phase, executor, elapsed.as_secs())
}

/// 任务期限将至时的提示（`warned` 为 true 时不再返回）
pub fn timeout_warning_message(
    elapsed: Duration,
    task_timeout: Duration,
    warned: bool,
) -> Option<String> {
    if warned || task_timeout.is_zero() {
        return None;
    }
    let threshold = Duration::from_secs_f64(task_timeout.as_secs_f64() * TIMEOUT_WARN_FRACTION);
    if elapsed < threshold {
        return None;
    }
    let remaining = task_timeout.saturating_sub(elapsed);
    Some(format!(
        "即将到达时间上限（剩余约 {} 秒）…",
        remaining.as_secs()
    ))
}

/// 在 agent 运行期间定时发进度；任务结束时应 abort 返回的 handle。
pub fn spawn_orchestrator_heartbeat(
    sink: EventSink,
    executor: String,
    phase: &'static str,
    run_started: std::time::Instant,
    task_timeout: Duration,
) -> JoinHandle<()> {
    let timeout_warned = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // 第一次 tick 立即触发；跳过，首个心跳在间隔之后
        interval.tick().await;
        loop {
            interval.tick().await;
            let elapsed = run_started.elapsed();
            sink.progress(format_run_heartbeat(elapsed, &executor, phase));
            if let Some(msg) = timeout_warning_message(
                elapsed,
                task_timeout,
                timeout_warned.load(Ordering::Acquire),
            ) {
                timeout_warned.store(true, Ordering::Release);
                sink.progress(msg);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_message_includes_phase_executor_and_elapsed() {
        let s = format_run_heartbeat(Duration::from_secs(120), "假 agent", "执行中");
        assert!(s.contains("执行中"));
        assert!(s.contains("假 agent"));
        assert!(s.contains("120"));
    }

    #[test]
    fn timeout_warning_only_after_threshold_and_once() {
        let total = Duration::from_secs(100);
        let early = Duration::from_secs(80);
        assert!(timeout_warning_message(early, total, false).is_none());
        let late = Duration::from_secs(86);
        let msg = timeout_warning_message(late, total, false).unwrap();
        assert!(msg.contains("即将到达时间上限"));
        assert!(timeout_warning_message(late, total, true).is_none());
    }

    #[test]
    fn rework_phase_label() {
        assert_eq!(run_phase_label(0), "执行中");
        assert_eq!(run_phase_label(2), "返工执行中");
    }
}
