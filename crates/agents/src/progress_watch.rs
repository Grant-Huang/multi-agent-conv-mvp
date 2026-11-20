//! ACP 等长连接 agent：长时间没有进度事件时发一条空闲提示。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::EventSink;
use tokio::task::JoinHandle;
use tokio::time::{self, Instant, MissedTickBehavior};

/// 无进度超过此时长则发一次提示
pub const IDLE_NUDGE_AFTER: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(15);

pub const IDLE_NUDGE_MESSAGE: &str = "仍在等待 agent 响应…";

/// 记录最近一次 progress，并驱动空闲提示
pub struct ProgressWatchdog {
    last: Arc<Mutex<Instant>>,
    nudged: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressWatchdog {
    pub fn start(events: EventSink) -> Self {
        let last = Arc::new(Mutex::new(Instant::now()));
        let nudged = Arc::new(AtomicBool::new(false));
        let last_w = last.clone();
        let nudged_w = nudged.clone();
        let handle = tokio::spawn(async move {
            let mut tick = time::interval(POLL_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let idle = last_w.lock().unwrap().elapsed();
                if idle >= IDLE_NUDGE_AFTER {
                    if !nudged_w.swap(true, Ordering::AcqRel) {
                        events.progress(IDLE_NUDGE_MESSAGE);
                    }
                } else {
                    nudged_w.store(false, Ordering::Release);
                }
            }
        });
        Self {
            last,
            nudged,
            handle: Some(handle),
        }
    }

    pub fn touch(&self) {
        *self.last.lock().unwrap() = Instant::now();
        self.nudged.store(false, Ordering::Release);
    }

    pub fn progress(&self, events: &EventSink, message: impl Into<String>) {
        self.touch();
        events.progress(message);
    }
}

impl Drop for ProgressWatchdog {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;
    impl an_core::EventPublisher for Noop {
        fn publish(&self, _: an_core::TaskEvent) {}
    }

    #[test]
    fn idle_nudge_message_is_stable() {
        assert_eq!(IDLE_NUDGE_MESSAGE, "仍在等待 agent 响应…");
        assert!(IDLE_NUDGE_AFTER >= Duration::from_secs(120));
    }

    #[tokio::test]
    async fn touch_resets_nudge_flag() {
        let sink = EventSink::new(Arc::new(Noop), "c", "t");
        let wd = ProgressWatchdog::start(sink);
        wd.nudged.store(true, Ordering::Release);
        wd.touch();
        assert!(!wd.nudged.load(Ordering::Acquire));
    }
}
