//! 后端调度器：每个后端一个，所有模型调用在发请求前先经过它。
//!
//! - `llama_cpp`：按槽位调度。前台只用 `interactive_slots`，后台只用 `background_slots`；
//!   `borrow_idle = true` 时前台可借用空闲的后台槽位（默认不借用）。槽位由客户端挑选空闲的，
//!   并通过请求体 `id_slot` 固定（指定的槽位被占时 llama.cpp 会死等，所以不能让服务端挑）。
//! - `openai`：按并发许可 + 令牌桶（rpm / tpm）调度；`reserved_interactive` 预留给前台；
//!   429 之后按 `Retry-After` 暂停整个后端。
//!
//! 同一优先级内先来先得；后台不会超过自己的容量挤占前台；等待有期限，超时返回"忙"（`E_LLM_BUSY`）。
//! 租约（`Lease`）在调用结束或被取消（future 被丢弃）时自动归还。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::backend::{BackendConfig, BackendKind, Priority};
use crate::error::{ErrorKind, LlmError};

/// 谁在申请（日志 / 诊断用）
pub struct Who<'a> {
    pub role: &'a str,
    pub priority: Priority,
    pub req_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub id: u32,
    /// interactive / background / shared / unused
    pub class: &'static str,
    pub holder: Option<String>,
    pub held_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedSnapshot {
    pub kind: &'static str,
    pub slots: Vec<SlotInfo>,
    pub inflight: u32,
    pub max_inflight: u32,
    pub queue_len: usize,
    /// 因 429 / 限速而暂停的剩余时间
    pub paused_ms: u64,
}

struct Holder {
    role: String,
    since: Instant,
}

/// 令牌桶：容量 = 每分钟额度，匀速补充
struct Bucket {
    capacity: f64,
    tokens: f64,
    per_sec: f64,
    last: Instant,
}

impl Bucket {
    fn new(per_minute: u32) -> Self {
        let c = per_minute as f64;
        Self { capacity: c, tokens: c, per_sec: c / 60.0, last: Instant::now() }
    }

    fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * self.per_sec).min(self.capacity);
        self.last = now;
    }

    /// 取 `n` 个需要等多久（0 = 现在就够）；单次需求超过容量时按容量算，否则永远等不到
    fn wait_for(&mut self, n: f64, now: Instant) -> Duration {
        self.refill(now);
        let n = n.min(self.capacity);
        if self.tokens >= n {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((n - self.tokens) / self.per_sec)
        }
    }

    fn take(&mut self, n: f64) {
        self.tokens -= n.min(self.capacity);
    }

    /// 实际用量和预估不同时修正：`delta > 0` 退还，`< 0` 补扣（允许为负，下次自然要多等）
    fn adjust(&mut self, delta: f64) {
        self.tokens = (self.tokens + delta).min(self.capacity);
    }
}

#[derive(Default)]
struct State {
    slots: Vec<Option<Holder>>,
    inflight: u32,
    queue: VecDeque<(u64, Priority)>,
    next_ticket: u64,
    rpm: Option<Bucket>,
    tpm: Option<Bucket>,
    paused_until: Option<Instant>,
}

struct Inner {
    name: String,
    cfg: BackendConfig,
    interactive: Vec<u32>,
    background: Vec<u32>,
    state: Mutex<State>,
    notify: Notify,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

enum Wait {
    /// 等别人释放
    Notify,
    /// 等到时间到（限速 / 429 暂停）
    Until(Duration),
}

pub struct Scheduler {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scheduler({})", self.inner.name)
    }
}

/// 一次占用：持有期间槽位 / 并发许可不会分给别人，丢弃时自动归还
pub struct Lease {
    inner: Arc<Inner>,
    slot: Option<u32>,
    priority: Priority,
    role: String,
    req_id: u64,
    since: Instant,
    est_tokens: u32,
    queue_wait: Duration,
    stolen: bool,
}

impl Lease {
    /// 要放进请求体 `id_slot` 的槽位号（只有 `llama_cpp` 后端有）
    pub fn slot(&self) -> Option<u32> {
        self.slot
    }

    pub fn queue_wait_ms(&self) -> u64 {
        self.queue_wait.as_millis() as u64
    }

    /// 前台借用了后台槽位
    pub fn stolen(&self) -> bool {
        self.stolen
    }

    /// 调用结束后用真实 token 数修正 tpm 额度
    pub fn report_usage(&self, actual_tokens: u64) {
        if let Some(b) = self.inner.lock().tpm.as_mut() {
            b.adjust(self.est_tokens as f64 - actual_tokens as f64);
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        {
            let mut st = self.inner.lock();
            match self.slot {
                Some(s) => {
                    if let Some(x) = st.slots.get_mut(s as usize) {
                        *x = None;
                    }
                }
                None => st.inflight = st.inflight.saturating_sub(1),
            }
        }
        self.inner.notify.notify_waiters();
        tracing::debug!(event = "llm.slot_release", req_id = self.req_id, backend = %self.inner.name, role = %self.role, priority = self.priority.as_str(), slot = ?self.slot, held_ms = self.since.elapsed().as_millis() as u64);
    }
}

/// 排队登记：等待的 future 被丢弃时自动退出队伍，免得挡住后面的人
struct QueueGuard<'a> {
    inner: &'a Inner,
    ticket: u64,
}

impl Drop for QueueGuard<'_> {
    fn drop(&mut self) {
        let removed = {
            let mut st = self.inner.lock();
            let before = st.queue.len();
            st.queue.retain(|(t, _)| *t != self.ticket);
            st.queue.len() != before
        };
        if removed {
            self.inner.notify.notify_waiters();
        }
    }
}

impl Scheduler {
    pub fn new(name: &str, cfg: &BackendConfig) -> Self {
        let (interactive, background) = cfg.effective_slots();
        let state = State {
            slots: (0..cfg.slots).map(|_| None).collect(),
            rpm: (cfg.kind == BackendKind::Openai && cfg.rpm > 0).then(|| Bucket::new(cfg.rpm)),
            tpm: (cfg.kind == BackendKind::Openai && cfg.tpm > 0).then(|| Bucket::new(cfg.tpm)),
            ..Default::default()
        };
        Self { inner: Arc::new(Inner { name: name.to_string(), cfg: cfg.clone(), interactive, background, state: Mutex::new(state), notify: Notify::new() }) }
    }

    /// 申请一个槽位 / 并发许可，最多等 `max_wait`；超时返回 `Busy`
    pub async fn acquire(&self, who: &Who<'_>, est_tokens: u32, max_wait: Duration) -> Result<Lease, LlmError> {
        let started = Instant::now();
        let deadline = started + max_wait;
        let inner = &self.inner;
        let ticket = {
            let mut st = inner.lock();
            let t = st.next_ticket;
            st.next_ticket += 1;
            st.queue.push_back((t, who.priority));
            t
        };
        let _guard = QueueGuard { inner, ticket };
        loop {
            // 先登记通知再检查，避免检查之后、等待之前错过释放信号
            let notified = inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let attempt = self.try_take(who, ticket, est_tokens, Instant::now());
            let wait = match attempt {
                Ok((slot, stolen)) => {
                    let lease = Lease { inner: inner.clone(), slot, priority: who.priority, role: who.role.to_string(), req_id: who.req_id, since: Instant::now(), est_tokens, queue_wait: started.elapsed(), stolen };
                    let waited = lease.queue_wait_ms();
                    let queue_depth = inner.lock().queue.len();
                    if waited >= 100 || stolen {
                        tracing::info!(event = "llm.slot_acquire", req_id = who.req_id, backend = %inner.name, role = who.role, priority = who.priority.as_str(), got_slot = ?slot, wait_ms = waited, stolen, queue_depth);
                    } else {
                        tracing::debug!(event = "llm.slot_acquire", req_id = who.req_id, backend = %inner.name, role = who.role, priority = who.priority.as_str(), got_slot = ?slot, wait_ms = waited, stolen, queue_depth);
                    }
                    return Ok(lease);
                }
                Err(w) => w,
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let (kind, depth) = (if inner.cfg.kind == BackendKind::LlamaCpp { "槽位" } else { "并发许可" }, inner.lock().queue.len());
                return Err(LlmError::new(ErrorKind::Busy, format!("后端 {} 繁忙：等了 {} 秒仍没有空闲{kind}（排队 {depth} 个）", inner.name, max_wait.as_secs())));
            }
            let sleep_for = match wait {
                Wait::Notify => remaining,
                Wait::Until(d) => d.min(remaining),
            };
            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep(sleep_for) => {}
            }
        }
    }

    fn try_take(&self, who: &Who<'_>, ticket: u64, est_tokens: u32, now: Instant) -> Result<(Option<u32>, bool), Wait> {
        let inner = &self.inner;
        let cfg = &inner.cfg;
        let mut st = inner.lock();
        // 同一优先级内先来先得
        if st.queue.iter().find(|(_, p)| *p == who.priority).map(|(t, _)| *t) != Some(ticket) {
            return Err(Wait::Notify);
        }
        if let Some(until) = st.paused_until {
            if until > now {
                return Err(Wait::Until(until - now));
            }
            st.paused_until = None;
        }
        let background = who.priority == Priority::Background;
        let grant = if cfg.kind == BackendKind::LlamaCpp {
            let free = |st: &State, s: u32| st.slots.get(s as usize).is_some_and(|x| x.is_none());
            let own = if background { &inner.background } else { &inner.interactive };
            if let Some(s) = own.iter().copied().find(|s| free(&st, *s)) {
                (Some(s), false)
            } else if !background && cfg.borrow_idle {
                match inner.background.iter().copied().find(|s| free(&st, *s) && !inner.interactive.contains(s)) {
                    Some(s) => (Some(s), true),
                    None => return Err(Wait::Notify),
                }
            } else {
                return Err(Wait::Notify);
            }
        } else {
            if cfg.max_inflight > 0 {
                let limit = if background { cfg.max_inflight.saturating_sub(cfg.reserved_interactive) } else { cfg.max_inflight };
                // 后台让着前台：有前台在排队时，后台不抢
                if st.inflight >= limit || (background && st.queue.iter().any(|(_, p)| *p == Priority::Interactive)) {
                    return Err(Wait::Notify);
                }
            }
            let mut wait = Duration::ZERO;
            if let Some(b) = st.rpm.as_mut() {
                wait = wait.max(b.wait_for(1.0, now));
            }
            if let Some(b) = st.tpm.as_mut() {
                wait = wait.max(b.wait_for(est_tokens as f64, now));
            }
            if !wait.is_zero() {
                return Err(Wait::Until(wait));
            }
            if let Some(b) = st.rpm.as_mut() {
                b.take(1.0);
            }
            if let Some(b) = st.tpm.as_mut() {
                b.take(est_tokens as f64);
            }
            (None, false)
        };
        // 提交：退出队伍，占用
        st.queue.retain(|(t, _)| *t != ticket);
        match grant.0 {
            Some(s) => st.slots[s as usize] = Some(Holder { role: who.role.to_string(), since: now }),
            None => st.inflight += 1,
        }
        Ok(grant)
    }

    /// 收到 429（或其他"请慢一点"）后，整个后端暂停 `d`，期间所有申请都在这里等
    pub fn pause_for(&self, d: Duration, reason: &str) {
        let until = Instant::now() + d;
        let mut st = self.inner.lock();
        if st.paused_until.is_none_or(|u| u < until) {
            st.paused_until = Some(until);
        }
        drop(st);
        tracing::warn!(event = "llm.ratelimit", code = an_core::codes::LLM_RATE_LIMITED, backend = %self.inner.name, reason, wait_ms = d.as_millis() as u64, "后端要求放慢，暂停发送新请求");
    }

    pub fn snapshot(&self) -> SchedSnapshot {
        let inner = &self.inner;
        let st = inner.lock();
        let now = Instant::now();
        let slots = st
            .slots
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let id = i as u32;
                let class = match (inner.interactive.contains(&id), inner.background.contains(&id)) {
                    (true, true) => "shared",
                    (true, false) => "interactive",
                    (false, true) => "background",
                    _ => "unused",
                };
                SlotInfo { id, class, holder: h.as_ref().map(|h| h.role.clone()), held_ms: h.as_ref().map_or(0, |h| now.duration_since(h.since).as_millis() as u64) }
            })
            .collect();
        SchedSnapshot {
            kind: inner.cfg.kind.as_str(),
            slots,
            inflight: st.inflight,
            max_inflight: inner.cfg.max_inflight,
            queue_len: st.queue.len(),
            paused_ms: st.paused_until.map_or(0, |u| u.saturating_duration_since(now).as_millis() as u64),
        }
    }

    #[cfg(test)]
    fn drain_rpm(&self) {
        if let Some(b) = self.inner.lock().rpm.as_mut() {
            b.tokens = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn local(interactive: &[u32], background: &[u32], borrow: bool) -> Scheduler {
        let slots = interactive.iter().chain(background).max().map_or(1, |m| m + 1);
        let cfg = BackendConfig { kind: BackendKind::LlamaCpp, base_url: "http://x/v1".into(), slots, interactive_slots: interactive.to_vec(), background_slots: background.to_vec(), borrow_idle: borrow, ..Default::default() };
        Scheduler::new("local", &cfg)
    }

    fn cloud(max_inflight: u32, reserved: u32, rpm: u32) -> Scheduler {
        let cfg = BackendConfig { base_url: "https://c/v1".into(), max_inflight, reserved_interactive: reserved, rpm, ..Default::default() };
        Scheduler::new("cloud", &cfg)
    }

    fn who(p: Priority) -> Who<'static> {
        Who { role: if p == Priority::Interactive { "conversation" } else { "general" }, priority: p, req_id: 1 }
    }

    const SHORT: Duration = Duration::from_millis(80);
    const LONG: Duration = Duration::from_secs(5);

    async fn get(s: &Scheduler, p: Priority) -> Lease {
        s.acquire(&who(p), 100, LONG).await.unwrap()
    }

    #[tokio::test]
    async fn interactive_uses_only_interactive_slots_by_default() {
        let s = local(&[0, 1], &[2, 3, 4, 5], false);
        let a = get(&s, Priority::Interactive).await;
        let b = get(&s, Priority::Interactive).await;
        assert_eq!((a.slot(), b.slot()), (Some(0), Some(1)));
        // 前台槽位用完：不借用后台槽位，等到期返回"忙"
        let e = s.acquire(&who(Priority::Interactive), 100, SHORT).await.err().unwrap();
        assert_eq!(e.kind, ErrorKind::Busy);
        assert!(e.to_string().contains("E_LLM_BUSY") && e.message.contains("槽位"), "{e}");
        drop(a);
        // 归还后立刻可用
        assert_eq!(get(&s, Priority::Interactive).await.slot(), Some(0));
    }

    #[tokio::test]
    async fn background_never_takes_interactive_slots() {
        let s = local(&[0, 1], &[2, 3, 4, 5], false);
        let mut held = vec![];
        for _ in 0..4 {
            held.push(get(&s, Priority::Background).await);
        }
        let mut slots: Vec<_> = held.iter().map(|l| l.slot().unwrap()).collect();
        slots.sort();
        assert_eq!(slots, [2, 3, 4, 5]);
        // 前台槽位全空，后台第 5 个也不能占
        assert_eq!(s.acquire(&who(Priority::Background), 100, SHORT).await.err().unwrap().kind, ErrorKind::Busy);
        assert!(s.snapshot().slots.iter().filter(|x| x.class == "interactive").all(|x| x.holder.is_none()));
    }

    #[tokio::test]
    async fn borrowing_is_opt_in_and_one_way() {
        let s = local(&[0], &[1, 2], true);
        let a = get(&s, Priority::Interactive).await;
        let b = get(&s, Priority::Interactive).await; // 借用后台槽位
        assert_eq!((a.slot(), b.slot().map(|x| x >= 1), b.stolen()), (Some(0), Some(true), true));
        // 后台只剩 1 个槽位，被借走 1 个
        let c = get(&s, Priority::Background).await;
        assert!(s.acquire(&who(Priority::Background), 100, SHORT).await.is_err());
        // 后台永远不借前台槽位：释放前台槽位 0 后，后台仍然拿不到
        drop(a);
        assert!(s.acquire(&who(Priority::Background), 100, SHORT).await.is_err());
        drop((b, c));
    }

    #[tokio::test]
    async fn waiters_are_served_in_arrival_order_and_cancelled_waiters_leave_the_queue() {
        let s = Arc::new(local(&[0], &[1], false));
        let first = get(&s, Priority::Interactive).await;
        let order = Arc::new(Mutex::new(vec![]));
        let mut handles = vec![];
        for i in 0..3u32 {
            let (s2, o2) = (s.clone(), order.clone());
            handles.push(tokio::spawn(async move {
                let l = s2.acquire(&Who { role: "conversation", priority: Priority::Interactive, req_id: i as u64 }, 10, LONG).await.unwrap();
                o2.lock().unwrap().push(i);
                tokio::time::sleep(Duration::from_millis(30)).await;
                drop(l);
            }));
            tokio::time::sleep(Duration::from_millis(20)).await; // 保证到达顺序
        }
        assert_eq!(s.snapshot().queue_len, 3);
        // 排在中间的放弃：不能挡住后面的
        handles[1].abort();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(s.snapshot().queue_len, 2);
        drop(first);
        for h in handles {
            let _ = h.await;
        }
        assert_eq!(*order.lock().unwrap(), [0, 2]);
        assert_eq!(s.snapshot().queue_len, 0);
    }

    #[tokio::test]
    async fn ten_concurrent_interactive_requests_never_exceed_two_slots() {
        let s = Arc::new(local(&[0, 1], &[2, 3, 4, 5], false));
        let (now, peak) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
        let mut handles = vec![];
        for i in 0..10u64 {
            let (s2, n2, p2) = (s.clone(), now.clone(), peak.clone());
            handles.push(tokio::spawn(async move {
                let l = s2.acquire(&Who { role: "conversation", priority: Priority::Interactive, req_id: i }, 10, LONG).await.unwrap();
                assert!(l.slot().unwrap() < 2);
                let cur = n2.fetch_add(1, Ordering::SeqCst) + 1;
                p2.fetch_max(cur, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                n2.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert!(s.snapshot().slots.iter().all(|x| x.holder.is_none()));
    }

    #[tokio::test]
    async fn snapshot_shows_holders_and_classes() {
        let s = local(&[0, 1], &[2], false);
        let l = s.acquire(&Who { role: "router", priority: Priority::Interactive, req_id: 7 }, 10, LONG).await.unwrap();
        let snap = s.snapshot();
        assert_eq!(snap.kind, "llama_cpp");
        assert_eq!(snap.slots[0].holder.as_deref(), Some("router"));
        assert_eq!(snap.slots.iter().map(|x| x.class).collect::<Vec<_>>(), ["interactive", "interactive", "background"]);
        drop(l);
        assert!(s.snapshot().slots[0].holder.is_none());
    }

    #[tokio::test]
    async fn cloud_reserves_permits_for_interactive() {
        let s = cloud(3, 1, 0);
        let b1 = get(&s, Priority::Background).await;
        let b2 = get(&s, Priority::Background).await;
        // 后台最多占 max - reserved = 2 个
        let e = s.acquire(&who(Priority::Background), 10, SHORT).await.err().unwrap();
        assert!(e.message.contains("并发许可"), "{e}");
        // 预留的那个只给前台
        let i1 = get(&s, Priority::Interactive).await;
        assert!(s.acquire(&who(Priority::Interactive), 10, SHORT).await.is_err());
        assert_eq!(s.snapshot().inflight, 3);
        assert!(i1.slot().is_none());
        drop((b1, b2, i1));
        assert_eq!(s.snapshot().inflight, 0);
    }

    #[tokio::test]
    async fn interactive_waiter_beats_an_earlier_background_waiter_for_a_freed_permit() {
        let s = Arc::new(cloud(2, 0, 0));
        let (a, b) = (get(&s, Priority::Background).await, get(&s, Priority::Background).await);
        let got = Arc::new(Mutex::new(vec![]));
        let spawn_waiter = |p: Priority, tag: &'static str| {
            let (s2, g2) = (s.clone(), got.clone());
            tokio::spawn(async move {
                let l = s2.acquire(&Who { role: tag, priority: p, req_id: 1 }, 10, LONG).await.unwrap();
                g2.lock().unwrap().push(tag);
                tokio::time::sleep(Duration::from_millis(40)).await;
                drop(l);
            })
        };
        let hb = spawn_waiter(Priority::Background, "background");
        tokio::time::sleep(Duration::from_millis(20)).await;
        let hi = spawn_waiter(Priority::Interactive, "interactive");
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(a);
        let _ = (hb.await, hi.await);
        drop(b);
        assert_eq!(*got.lock().unwrap(), ["interactive", "background"]);
    }

    #[tokio::test]
    async fn unlimited_cloud_backend_never_blocks() {
        let s = Arc::new(cloud(0, 0, 0));
        let mut held = vec![];
        for _ in 0..100 {
            held.push(s.acquire(&who(Priority::Background), 10, SHORT).await.unwrap());
        }
        assert_eq!(s.snapshot().inflight, 100);
    }

    #[tokio::test]
    async fn rpm_bucket_delays_requests_when_exhausted() {
        let s = cloud(0, 0, 60); // 每秒 1 个
        s.drain_rpm();
        let started = Instant::now();
        let l = s.acquire(&who(Priority::Interactive), 10, LONG).await.unwrap();
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(900) && waited < Duration::from_millis(2500), "{waited:?}");
        assert!(l.queue_wait_ms() >= 900);
    }

    #[tokio::test]
    async fn pause_after_429_blocks_everyone_until_it_expires() {
        let s = cloud(0, 0, 0);
        s.pause_for(Duration::from_millis(300), "429");
        assert!(s.snapshot().paused_ms > 0);
        let started = Instant::now();
        let _l = s.acquire(&who(Priority::Interactive), 10, LONG).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(250), "{:?}", started.elapsed());
        // 等待期限比暂停短：返回"忙"
        s.pause_for(Duration::from_secs(5), "429");
        assert_eq!(s.acquire(&who(Priority::Interactive), 10, SHORT).await.err().unwrap().kind, ErrorKind::Busy);
    }

    #[test]
    fn bucket_math_and_usage_correction() {
        let t0 = Instant::now();
        let mut b = Bucket::new(6000); // 每秒 100
        assert_eq!(b.wait_for(1000.0, t0), Duration::ZERO);
        b.take(6000.0);
        // 空了：要 1000 个得等 10 秒
        let w = b.wait_for(1000.0, t0);
        assert!(w >= Duration::from_millis(9900) && w <= Duration::from_millis(10100), "{w:?}");
        // 单次需求超过容量按容量算，不会永远等不到
        assert!(b.wait_for(1_000_000.0, t0) <= Duration::from_secs(61));
        // 一秒后补 100
        assert_eq!(b.wait_for(90.0, t0 + Duration::from_secs(1)), Duration::ZERO);
        // 预估 1000、实际只用 200：退还 800；实际 3000：补扣 2000
        let mut b = Bucket::new(6000);
        b.take(1000.0);
        b.adjust(800.0);
        assert!((b.tokens - 5800.0).abs() < 1e-6);
        b.adjust(-2000.0);
        assert!((b.tokens - 3800.0).abs() < 1e-6);
    }
}
