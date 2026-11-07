//! Agent 连接状态（健康接口与界面的圆点颜色）
//!
//! - 绿（connected）：最近 `ACTIVE_WINDOW` 内有过一次成功的真实调用；或最近一次探测在 `PROBE_FRESH` 内握手成功
//! - 灰（configured）：已安装 / 已配置，但没有成功调用的记录，探测失败或尚未探测
//! - 红（unavailable）：未安装（命令找不到）
//!
//! 探测只做握手（不调用模型），真实任务成功完成时记录一次成功调用。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// 一次成功调用之后保持绿色的时长
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(10 * 60);
/// 后台探测间隔（只做握手，不调用模型）
pub const PROBE_INTERVAL: Duration = Duration::from_secs(60);
/// 探测结果的有效期：超过它仍未刷新，就不再算绿色
pub const PROBE_FRESH: Duration = Duration::from_secs(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Connected,
    Configured,
    Unavailable,
}

#[derive(Debug, Clone, Default)]
pub struct LinkState {
    /// 最近一次真实任务成功完成的时间
    pub last_success: Option<Instant>,
    /// 最近一次探测的时间和结果（Err 为失败原因）
    pub last_probe: Option<(Instant, Result<(), String>)>,
}

/// 根据安装情况和记录算出状态
pub fn link_status(now: Instant, installed: bool, state: &LinkState) -> LinkStatus {
    if !installed {
        return LinkStatus::Unavailable;
    }
    // 最近一次真实成功调用仍在有效期内：绿色
    if let Some(t) = state.last_success {
        if now.saturating_duration_since(t) <= ACTIVE_WINDOW {
            return LinkStatus::Connected;
        }
    }
    // 最近一次探测成功且结果仍新鲜：绿色
    if let Some((t, Ok(()))) = &state.last_probe {
        if now.saturating_duration_since(*t) <= PROBE_FRESH {
            return LinkStatus::Connected;
        }
    }
    LinkStatus::Configured
}

/// 按 agent id 记录连接状态；由编排器（成功调用）和后台探测写入
#[derive(Default)]
pub struct LinkRegistry {
    states: Mutex<HashMap<String, LinkState>>,
}

impl LinkRegistry {
    pub fn mark_success(&self, id: &str) {
        self.states
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .last_success = Some(Instant::now());
    }

    pub fn record_probe(&self, id: &str, result: Result<(), String>) {
        self.states
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .last_probe = Some((Instant::now(), result));
    }

    pub fn state(&self, id: &str) -> LinkState {
        self.states
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(now: Instant, d: Duration) -> Instant {
        now.checked_sub(d).expect("时间足够早")
    }

    #[test]
    fn not_installed_is_unavailable() {
        let now = Instant::now();
        assert_eq!(
            link_status(now, false, &LinkState::default()),
            LinkStatus::Unavailable
        );
    }

    #[test]
    fn installed_without_any_record_is_configured() {
        let now = Instant::now();
        assert_eq!(
            link_status(now, true, &LinkState::default()),
            LinkStatus::Configured
        );
    }

    #[test]
    fn fresh_successful_probe_is_connected() {
        let now = Instant::now();
        let s = LinkState {
            last_probe: Some((ago(now, Duration::from_secs(30)), Ok(()))),
            ..Default::default()
        };
        assert_eq!(link_status(now, true, &s), LinkStatus::Connected);
    }

    #[test]
    fn stale_successful_probe_is_not_connected() {
        let now = Instant::now();
        let s = LinkState {
            last_probe: Some((ago(now, PROBE_FRESH + Duration::from_secs(30)), Ok(()))),
            ..Default::default()
        };
        assert_eq!(link_status(now, true, &s), LinkStatus::Configured);
    }

    #[test]
    fn failed_probe_is_configured_not_connected() {
        let now = Instant::now();
        let s = LinkState {
            last_probe: Some((ago(now, Duration::from_secs(5)), Err("握手超时".into()))),
            ..Default::default()
        };
        assert_eq!(link_status(now, true, &s), LinkStatus::Configured);
    }

    #[test]
    fn successful_call_keeps_connected_for_ten_minutes() {
        let now = Instant::now();
        // 5 分钟前成功调用过，最近探测失败：仍然绿色
        let s = LinkState {
            last_success: Some(ago(now, Duration::from_secs(5 * 60))),
            last_probe: Some((ago(now, Duration::from_secs(5)), Err("握手超时".into()))),
        };
        assert_eq!(link_status(now, true, &s), LinkStatus::Connected);
    }

    #[test]
    fn successful_call_expires_after_ten_minutes() {
        let now = Instant::now();
        let s = LinkState {
            last_success: Some(ago(now, ACTIVE_WINDOW + Duration::from_secs(60))),
            last_probe: Some((ago(now, Duration::from_secs(5)), Err("握手超时".into()))),
        };
        assert_eq!(link_status(now, true, &s), LinkStatus::Configured);
    }

    #[test]
    fn registry_records_success_and_probe_per_agent() {
        let r = LinkRegistry::default();
        assert!(r.state("opencode").last_success.is_none());
        r.mark_success("opencode");
        r.record_probe("opencode", Err("x".into()));
        let s = r.state("opencode");
        assert!(s.last_success.is_some());
        assert!(matches!(s.last_probe, Some((_, Err(_)))));
        // 其他 agent 不受影响
        assert!(r.state("pi").last_success.is_none());
    }
}
