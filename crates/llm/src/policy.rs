//! 调用策略：分段超时、指数退避、熔断器、调用统计。

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::Priority;

/// 分段超时。`total` 是单次请求的总时长上限；`deadline` 是整个调用（含重试）等待响应开始的预算
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub connect: Duration,
    /// 等第一个 token（含排队和提示词预填充）
    pub first_token: Duration,
    /// 开始生成后，两个 token 之间最长间隔
    pub idle: Duration,
    pub total: Duration,
    /// 整个调用（含重试）的期限：超过后不再发起新的重试，首 token 等待也不会超过剩余时间；None = 不限
    pub deadline: Option<Duration>,
}

impl Timeouts {
    /// 默认值：连接 5s、首 token 120s、间隔 60s；前台调用期限 60s，后台不限
    pub fn defaults(total_secs: u64, priority: Priority) -> Self {
        Self {
            connect: Duration::from_secs(5),
            first_token: Duration::from_secs(120),
            idle: Duration::from_secs(60),
            total: Duration::from_secs(total_secs.max(1)),
            deadline: (priority == Priority::Interactive).then(|| Duration::from_secs(60)),
        }
    }
}

/// 第 `attempt` 次失败后的退避时间（attempt 从 1 开始）：0.5s、1s、2s… 最长 8s，带 ±25% 抖动。
/// `jitter` 取 0..1；`retry_after` 是服务端要求的等待，取两者较大者
pub fn backoff_delay(attempt: u32, jitter: f64, retry_after: Option<Duration>) -> Duration {
    let base = 500u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(5)).min(8000);
    let factor = 0.75 + 0.5 * jitter.clamp(0.0, 1.0);
    let d = Duration::from_millis((base as f64 * factor) as u64);
    retry_after.map_or(d, |r| d.max(r))
}

/// 0..1 的伪随机数（只用于退避抖动，不需要密码学强度）
pub fn jitter01() -> f64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0x9E37_79B9);
    let n = COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(0);
    let mut x = n ^ t.rotate_left(17);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x % 10_000) as f64 / 10_000.0
}

/// 熔断器：连续失败 `threshold` 次后，`open_for` 内快速失败；到期后放行试探，成功即恢复，再失败立刻重新熔断
#[derive(Debug)]
pub struct Breaker {
    threshold: u32,
    open_for: Duration,
    state: Mutex<BreakerState>,
}

#[derive(Debug, Default)]
struct BreakerState {
    consecutive: u32,
    open_until: Option<Instant>,
}

impl Breaker {
    pub fn new(threshold: u32, open_for: Duration) -> Self {
        Self { threshold: threshold.max(1), open_for, state: Mutex::new(BreakerState::default()) }
    }

    pub fn default_policy() -> Self {
        Self::new(3, Duration::from_secs(10))
    }

    /// 熔断中返回 `Err(剩余时间)`
    pub fn check(&self) -> Result<(), Duration> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match s.open_until {
            Some(t) if t > Instant::now() => Err(t - Instant::now()),
            _ => Ok(()),
        }
    }

    /// 返回 true 表示刚从熔断状态恢复
    pub fn record_success(&self) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let was_open = s.consecutive >= self.threshold;
        s.consecutive = 0;
        s.open_until = None;
        was_open
    }

    /// 返回 true 表示这次失败触发了熔断
    pub fn record_failure(&self) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.consecutive += 1;
        if s.consecutive >= self.threshold {
            s.open_until = Some(Instant::now() + self.open_for);
            return true;
        }
        false
    }

    pub fn is_open(&self) -> bool {
        self.check().is_err()
    }
}

/// 一次模型调用（一次尝试）的记录
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub role: String,
    pub priority: Priority,
    pub outcome: &'static str,
    pub attempt: u32,
    pub queue_wait_ms: u64,
    pub ttft_ms: Option<u64>,
    pub total_ms: u64,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsSummary {
    pub total: u64,
    pub outcomes: BTreeMap<String, u64>,
    pub total_ms_p50: u64,
    pub total_ms_p95: u64,
    pub ttft_ms_p50: u64,
    pub ttft_ms_p95: u64,
}

/// 最近 N 次调用的环形缓冲，诊断页用
#[derive(Debug)]
pub struct Stats {
    cap: usize,
    ring: Mutex<VecDeque<CallRecord>>,
}

impl Stats {
    pub fn new(cap: usize) -> Self {
        Self { cap: cap.max(1), ring: Mutex::new(VecDeque::new()) }
    }

    pub fn record(&self, r: CallRecord) {
        let mut q = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= self.cap {
            q.pop_front();
        }
        q.push_back(r);
    }

    pub fn recent(&self) -> Vec<CallRecord> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    pub fn summary(&self) -> StatsSummary {
        let recs = self.recent();
        let mut s = StatsSummary { total: recs.len() as u64, ..Default::default() };
        for r in &recs {
            *s.outcomes.entry(r.outcome.to_string()).or_default() += 1;
        }
        let ok_total: Vec<u64> = recs.iter().filter(|r| r.outcome == "ok").map(|r| r.total_ms).collect();
        let ttft: Vec<u64> = recs.iter().filter_map(|r| r.ttft_ms).collect();
        (s.total_ms_p50, s.total_ms_p95) = (percentile(&ok_total, 50), percentile(&ok_total, 95));
        (s.ttft_ms_p50, s.ttft_ms_p95) = (percentile(&ttft, 50), percentile(&ttft, 95));
        s
    }
}

fn percentile(values: &[u64], p: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    v[((v.len() - 1) * p).div_ceil(100)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_exponentially_with_cap_and_jitter() {
        assert_eq!(backoff_delay(1, 0.5, None), Duration::from_millis(500));
        assert_eq!(backoff_delay(2, 0.5, None), Duration::from_millis(1000));
        assert_eq!(backoff_delay(3, 0.5, None), Duration::from_millis(2000));
        assert_eq!(backoff_delay(20, 0.5, None), Duration::from_millis(8000));
        // 抖动范围 ±25%
        assert_eq!(backoff_delay(1, 0.0, None), Duration::from_millis(375));
        assert_eq!(backoff_delay(1, 1.0, None), Duration::from_millis(625));
        // 零退避不可能：第一次也至少 375ms
        assert!(backoff_delay(1, 0.0, None) > Duration::from_millis(300));
    }

    #[test]
    fn retry_after_wins_when_longer() {
        assert_eq!(backoff_delay(1, 0.5, Some(Duration::from_secs(3))), Duration::from_secs(3));
        assert_eq!(backoff_delay(3, 0.5, Some(Duration::from_millis(100))), Duration::from_millis(2000));
    }

    #[test]
    fn jitter_stays_in_unit_interval() {
        for _ in 0..200 {
            let j = jitter01();
            assert!((0.0..1.0).contains(&j), "{j}");
        }
    }

    #[test]
    fn default_timeouts_follow_priority() {
        let i = Timeouts::defaults(180, Priority::Interactive);
        assert_eq!((i.connect.as_secs(), i.first_token.as_secs(), i.idle.as_secs(), i.total.as_secs()), (5, 120, 60, 180));
        assert_eq!(i.deadline, Some(Duration::from_secs(60)));
        assert_eq!(Timeouts::defaults(180, Priority::Background).deadline, None);
    }

    #[test]
    fn breaker_opens_after_consecutive_failures_and_recovers() {
        let b = Breaker::new(3, Duration::from_millis(60));
        assert!(!b.record_failure());
        assert!(!b.record_failure());
        assert!(b.check().is_ok());
        assert!(b.record_failure()); // 第 3 次触发熔断
        assert!(b.check().is_err());
        std::thread::sleep(Duration::from_millis(80));
        assert!(b.check().is_ok()); // 到期：放行试探
        assert!(b.record_failure()); // 试探失败：立刻重新熔断
        assert!(b.is_open());
        std::thread::sleep(Duration::from_millis(80));
        assert!(b.record_success()); // 试探成功：恢复
        assert!(!b.is_open());
        // 成功之后计数清零
        assert!(!b.record_failure());
    }

    #[test]
    fn success_between_failures_resets_the_count() {
        let b = Breaker::new(3, Duration::from_secs(10));
        b.record_failure();
        b.record_failure();
        assert!(!b.record_success());
        assert!(!b.record_failure());
        assert!(!b.is_open());
    }

    fn rec(outcome: &'static str, total_ms: u64, ttft: Option<u64>) -> CallRecord {
        CallRecord { role: "router".into(), priority: Priority::Interactive, outcome, attempt: 1, queue_wait_ms: 0, ttft_ms: ttft, total_ms, prompt_tokens: None, completion_tokens: None }
    }

    #[test]
    fn stats_ring_keeps_latest_and_summarizes() {
        let s = Stats::new(4);
        for i in 1..=6u64 {
            s.record(rec("ok", i * 100, Some(i * 10)));
        }
        s.record(rec("timeout_first_token", 120_000, None));
        let recent = s.recent();
        assert_eq!(recent.len(), 4);
        assert_eq!(recent[0].total_ms, 400); // 最早的被挤掉
        let sum = s.summary();
        assert_eq!(sum.total, 4);
        assert_eq!(sum.outcomes["ok"], 3);
        assert_eq!(sum.outcomes["timeout_first_token"], 1);
        // 耗时分位只统计成功的调用
        assert_eq!(sum.total_ms_p50, 500);
        assert_eq!(sum.total_ms_p95, 600);
        assert_eq!(Stats::new(2).summary(), StatsSummary::default());
    }
}
