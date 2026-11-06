//! 后端（一个推理服务 + 它的限流方式）的配置。
//!
//! - `llama_cpp`：本地 llama.cpp，有槽位；请求体可以带 `id_slot` 固定槽位，支持 `/slots`、`/metrics`、`/props`
//! - `openai`：通用 OpenAI 兼容接口（云端），没有槽位，用并发上限 + 速率限制
//!
//! 调度逻辑（占用槽位 / 许可、排队、借用）见 `scheduler.rs`。

use std::collections::BTreeSet;

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex as AsyncMutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::LlmError;
use crate::policy::{Breaker, Stats};
use crate::scheduler::{Lease, Scheduler, Who};
use crate::StructuredMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// 通用 OpenAI 兼容接口（云端），按并发上限 + 速率限制调度
    #[default]
    Openai,
    /// 本地 llama.cpp，按槽位调度
    LlamaCpp,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Openai => "openai",
            BackendKind::LlamaCpp => "llama_cpp",
        }
    }
}

/// 请求优先级：前台（用户正在等回复）优先于后台任务
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    Interactive,
    Background,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::Interactive => "interactive",
            Priority::Background => "background",
        }
    }
}

/// `[backends.<名字>]`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    #[serde(default)]
    pub kind: BackendKind,
    /// 例如 `http://127.0.0.1:8200/v1`
    pub base_url: String,
    /// 不写则用 `[llm].model`
    #[serde(default)]
    pub model: Option<String>,
    /// 保存密钥的环境变量名（密钥只从环境变量读取，配置文件里不写）
    #[serde(default)]
    pub api_key_env: Option<String>,

    // ── llama_cpp：槽位 ──
    /// 槽位总数，必须和 llama-server 的 `-np` 一致；编号 0..slots-1
    #[serde(default)]
    pub slots: u32,
    /// 前台优先使用的槽位；slots 已填但两个列表都不写时：slots=1 共用槽位 0，否则前台 [0]、后台其余
    #[serde(default)]
    pub interactive_slots: Vec<u32>,
    /// 后台只能使用的槽位；留空（且前台列表有内容）表示该后端不接后台角色
    #[serde(default)]
    pub background_slots: Vec<u32>,
    /// 前台槽位全忙时，是否可以借用空闲的后台槽位（后台永远不借用前台槽位）；默认不借用
    #[serde(default)]
    pub borrow_idle: bool,

    // ── openai：并发与速率 ──
    /// 客户端并发上限，0 = 不限
    #[serde(default)]
    pub max_inflight: u32,
    /// 并发许可里预留给前台的数量
    #[serde(default)]
    pub reserved_interactive: u32,
    /// 每分钟请求数上限（令牌桶），0 = 不限
    #[serde(default)]
    pub rpm: u32,
    /// 每分钟 token 上限，0 = 不限
    #[serde(default)]
    pub tpm: u32,

    // ── 通用 ──
    /// 模型的上下文总量；0 = llama_cpp 启动后从 `/props` 读取
    #[serde(default)]
    pub context_tokens: u32,
    /// 上下文是否被所有槽位 / 请求共享（llama.cpp `--kv-unified`）：共享时每个请求只能用 总量 ÷ 槽位数；
    /// 不写：llama_cpp 默认共享，openai 默认不共享（每个请求独占）
    #[serde(default)]
    pub context_shared: Option<bool>,
    /// 不写则用 `[llm].structured_mode`
    #[serde(default)]
    pub structured_mode: Option<StructuredMode>,
    /// 附加到每个请求体的字段，例如 `{ chat_template_kwargs = { enable_thinking = false } }`。
    /// 这会作用到该后端的每一次调用。只想关掉前台判断的思考时，用 `front_structured_thinking`。
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    /// 前台结构化调用（conversation / router）是否思考。不写则不改请求，沿用模型默认。
    /// `false` 只给这两个角色的结构化请求加上 `enable_thinking=false`，直接回答和后台任务不变。
    #[serde(default)]
    pub front_structured_thinking: Option<bool>,
    #[serde(default)]
    pub connect_secs: Option<u64>,
    #[serde(default)]
    pub first_token_secs: Option<u64>,
    #[serde(default)]
    pub idle_secs: Option<u64>,
    #[serde(default)]
    pub total_secs: Option<u64>,
    /// 前台调用（含重试）的期限，0 = 不限；不写：前台 60 秒、后台不限
    #[serde(default)]
    pub deadline_secs: Option<u64>,
    /// 排队等槽位 / 并发许可的最长时间，超时返回"忙"；不写：前台 30 秒、后台 600 秒
    #[serde(default)]
    pub queue_secs: Option<u64>,
}

impl BackendConfig {
    /// 生效的（前台槽位, 后台槽位）；只对 `llama_cpp` 有意义
    pub fn effective_slots(&self) -> (Vec<u32>, Vec<u32>) {
        if self.kind != BackendKind::LlamaCpp {
            return (vec![], vec![]);
        }
        if self.interactive_slots.is_empty() && self.background_slots.is_empty() && self.slots > 0 {
            return if self.slots == 1 {
                (vec![0], vec![0])
            } else {
                (vec![0], (1..self.slots).collect())
            };
        }
        (
            self.interactive_slots.clone(),
            self.background_slots.clone(),
        )
    }

    /// 能不能接后台角色（general / reviewer / coding）
    pub fn accepts_background(&self) -> bool {
        match self.kind {
            BackendKind::LlamaCpp => !self.effective_slots().1.is_empty(),
            BackendKind::Openai => {
                self.max_inflight == 0 || self.max_inflight > self.reserved_interactive
            }
        }
    }

    /// 一句话描述限流方式（启动日志 / 诊断用，不含密钥）
    pub fn describe_limits(&self) -> String {
        match self.kind {
            BackendKind::LlamaCpp => {
                let (i, b) = self.effective_slots();
                format!(
                    "槽位 {} 个，前台 {:?}，后台 {:?}，借用空闲={}",
                    self.slots, i, b, self.borrow_idle
                )
            }
            BackendKind::Openai => {
                let mut parts = vec![];
                parts.push(if self.max_inflight == 0 {
                    "并发不限".to_string()
                } else {
                    format!(
                        "并发上限 {}（预留前台 {}）",
                        self.max_inflight, self.reserved_interactive
                    )
                });
                if self.rpm > 0 {
                    parts.push(format!("rpm {}", self.rpm));
                }
                if self.tpm > 0 {
                    parts.push(format!("tpm {}", self.tpm));
                }
                parts.join("，")
            }
        }
    }

    /// 校验配置；错误信息说明要改哪一项
    pub fn validate(&self, name: &str) -> Result<(), String> {
        let at = |field: &str, msg: &str| format!("[backends.{name}] {field}：{msg}");
        if self.base_url.trim().is_empty() {
            return Err(at("base_url", "不能为空"));
        }
        match self.kind {
            BackendKind::LlamaCpp => {
                if self.slots == 0 {
                    return Err(at(
                        "slots",
                        "llama_cpp 后端必须填写槽位总数（与 llama-server 的 -np 一致）",
                    ));
                }
                let (interactive, background) = self.effective_slots();
                for (field, list) in [
                    ("interactive_slots", &interactive),
                    ("background_slots", &background),
                ] {
                    let mut seen = BTreeSet::new();
                    for s in list {
                        if *s >= self.slots {
                            return Err(at(
                                field,
                                &format!(
                                    "槽位 {s} 超出范围（slots = {}，编号 0..{}）",
                                    self.slots,
                                    self.slots - 1
                                ),
                            ));
                        }
                        if !seen.insert(*s) {
                            return Err(at(field, &format!("槽位 {s} 重复")));
                        }
                    }
                }
                if interactive.is_empty() {
                    return Err(at("interactive_slots", "至少要有一个前台槽位"));
                }
                if self.max_inflight != 0
                    || self.reserved_interactive != 0
                    || self.rpm != 0
                    || self.tpm != 0
                {
                    return Err(at(
                        "max_inflight / reserved_interactive / rpm / tpm",
                        "llama_cpp 后端用槽位调度，请删除这些字段",
                    ));
                }
            }
            BackendKind::Openai => {
                if self.slots != 0
                    || !self.interactive_slots.is_empty()
                    || !self.background_slots.is_empty()
                {
                    return Err(at(
                        "slots / interactive_slots / background_slots",
                        "openai 后端没有槽位概念，请删除这些字段，改用 max_inflight",
                    ));
                }
                if self.max_inflight > 0 && self.reserved_interactive > self.max_inflight {
                    return Err(at(
                        "reserved_interactive",
                        &format!("不能大于 max_inflight（{}）", self.max_inflight),
                    ));
                }
                if self.max_inflight == 0 && self.reserved_interactive > 0 {
                    return Err(at(
                        "reserved_interactive",
                        "没有设置 max_inflight 时预留数没有意义，请同时设置 max_inflight",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 运行时的后端：名字 + 配置 + 同一后端所有客户端共享的熔断器和调用统计（槽位调度在 pre-M3.3 加入）
#[derive(Debug)]
pub struct Backend {
    pub name: String,
    pub cfg: BackendConfig,
    pub breaker: Breaker,
    pub stats: Stats,
    /// 槽位 / 并发许可 / 限速调度
    pub sched: Scheduler,
    /// 上下文总量（token）：配置里写了就用配置值，否则等 `probe()` 从服务读取；0 = 未知，不做预算裁剪
    ctx: AtomicU32,
    /// 健康检查缓存（模型列表、`/slots` + `/metrics`），多个浏览器共用，避免每次刷新都打到推理服务
    probe_cache: AsyncMutex<ProbeCache>,
}

/// 健康检查结果缓存多久
pub const PROBE_TTL: Duration = Duration::from_secs(8);

#[derive(Debug, Default)]
struct ProbeCache {
    models: Option<(Instant, std::result::Result<Vec<String>, String>)>,
    server: Option<(
        Instant,
        std::result::Result<crate::status::ServerView, String>,
    )>,
}

/// llama-server `/props` 里我们关心的字段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerProps {
    pub n_ctx: u32,
    pub total_slots: u32,
}

pub fn parse_props(v: &Value) -> Option<ServerProps> {
    Some(ServerProps {
        n_ctx: v["default_generation_settings"]["n_ctx"].as_u64()? as u32,
        total_slots: v["total_slots"].as_u64()? as u32,
    })
}

impl Backend {
    pub fn new(name: impl Into<String>, cfg: BackendConfig) -> Self {
        let name = name.into();
        let sched = Scheduler::new(&name, &cfg);
        let ctx = AtomicU32::new(cfg.context_tokens);
        Self {
            name,
            cfg,
            breaker: Breaker::default_policy(),
            stats: Stats::new(500),
            sched,
            ctx,
            probe_cache: AsyncMutex::new(ProbeCache {
                models: None,
                server: None,
            }),
        }
    }

    /// 模型列表：缓存未过期就直接返回，否则调用 `fetch` 并记住结果（成功和失败都缓存）。
    /// 持锁期间完成请求，所以并发的健康检查只会真正请求一次
    pub async fn models_cached<F>(&self, fetch: F) -> std::result::Result<Vec<String>, String>
    where
        F: std::future::Future<Output = anyhow::Result<Vec<String>>>,
    {
        let mut g = self.probe_cache.lock().await;
        if let Some((at, cached)) = &g.models {
            if at.elapsed() < PROBE_TTL {
                return cached.clone();
            }
        }
        let r = fetch.await.map_err(|e| format!("{e:#}"));
        g.models = Some((Instant::now(), r.clone()));
        r
    }

    /// `/slots` + `/metrics`：同样缓存。只对 `llama_cpp` 后端有意义
    pub async fn server_cached<F>(
        &self,
        fetch: F,
    ) -> std::result::Result<crate::status::ServerView, String>
    where
        F: std::future::Future<Output = std::result::Result<crate::status::ServerView, String>>,
    {
        let mut g = self.probe_cache.lock().await;
        if let Some((at, cached)) = &g.server {
            if at.elapsed() < PROBE_TTL {
                return cached.clone();
            }
        }
        let r = fetch.await;
        g.server = Some((Instant::now(), r.clone()));
        r
    }

    /// 给不经过本客户端的外部进程（pi 等直连推理服务的 coding agent）申请一个**后台**租约：
    /// 租约持有期间槽位归它，调用方把 `lease.slot()` 告诉外部进程（环境变量 `AN_LLM_SLOT`），结束时丢弃租约即归还。
    /// 排队有期限（`queue_secs`，默认 600 秒），超时返回 `E_LLM_BUSY`
    pub async fn lease_background(&self, role: &str) -> Result<Lease, LlmError> {
        let who = Who {
            role,
            priority: Priority::Background,
            req_id: crate::next_req_id(),
        };
        let wait = Duration::from_secs(self.cfg.queue_secs.unwrap_or(600));
        self.sched.acquire(&who, 0, wait).await
    }

    pub fn context_tokens(&self) -> u32 {
        self.ctx.load(Ordering::Relaxed)
    }

    pub fn set_context_tokens(&self, n: u32) {
        self.ctx.store(n, Ordering::Relaxed);
    }

    /// 同时占用这个上下文的请求数：共享池（llama.cpp 的 `--kv-unified`）按槽位数分摊，否则每个请求独占
    pub fn context_divisor(&self) -> u32 {
        if self
            .cfg
            .context_shared
            .unwrap_or(self.cfg.kind == BackendKind::LlamaCpp)
        {
            self.cfg.slots.max(1)
        } else {
            1
        }
    }

    /// 向 llama-server 读取 `/props`：未配置 `context_tokens` 时记下服务的上下文总量，并核对槽位数。
    /// 只对 `llama_cpp` 后端有意义。
    pub async fn probe(&self) -> anyhow::Result<ServerProps> {
        use anyhow::Context;
        let base = self.cfg.base_url.trim_end_matches('/');
        let url = format!("{}/props", base.strip_suffix("/v1").unwrap_or(base));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let v: Value = http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("无法连接 {url}"))?
            .error_for_status()?
            .json()
            .await?;
        let props = parse_props(&v).with_context(|| {
            format!("{url} 的返回里没有 n_ctx / total_slots（不是 llama-server？）")
        })?;
        if self.cfg.context_tokens == 0 {
            self.set_context_tokens(props.n_ctx);
        }
        if self.cfg.kind == BackendKind::LlamaCpp && props.total_slots != self.cfg.slots {
            tracing::warn!(event = "llm.slots_mismatch", code = an_core::codes::LLM_SLOTS_MISMATCH, backend = %self.name, configured = self.cfg.slots, server = props.total_slots, "[backends.{}] slots = {}，但 llama-server 实际是 {} 个槽位（-np）；请让两边一致，否则 id_slot 可能指向不存在的槽位", self.name, self.cfg.slots, props.total_slots);
        }
        tracing::info!(event = "llm.backend_probe", backend = %self.name, n_ctx = props.n_ctx, total_slots = props.total_slots, context_tokens = self.context_tokens());
        Ok(props)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(slots: u32) -> BackendConfig {
        BackendConfig {
            kind: BackendKind::LlamaCpp,
            base_url: "http://x/v1".into(),
            slots,
            ..Default::default()
        }
    }

    #[test]
    fn llama_defaults_front_slot_zero_and_back_the_rest() {
        let (i, b) = local(5).effective_slots();
        assert_eq!(i, [0]);
        assert_eq!(b, [1, 2, 3, 4]);
        // 只有 1 个槽位：前后台共用
        let (i, b) = local(1).effective_slots();
        assert_eq!((i, b), (vec![0], vec![0]));
        assert!(local(5).validate("l").is_ok());
        assert!(local(1).validate("l").is_ok());
    }

    #[test]
    fn explicit_lists_win_and_empty_background_means_front_only() {
        let c = BackendConfig {
            interactive_slots: vec![0, 1],
            background_slots: vec![],
            ..local(2)
        };
        // 两个列表里只要有一个非空，就不再套用默认值
        assert_eq!(c.effective_slots(), (vec![0, 1], vec![]));
        assert!(!c.accepts_background());
        assert!(c.validate("l").is_ok());
        assert!(local(5).accepts_background());
    }

    #[test]
    fn slot_errors_name_the_field() {
        let e = BackendConfig {
            interactive_slots: vec![0],
            background_slots: vec![1, 5],
            ..local(5)
        }
        .validate("l")
        .unwrap_err();
        assert!(
            e.contains("background_slots") && e.contains("超出范围"),
            "{e}"
        );
        let e = BackendConfig {
            interactive_slots: vec![0, 0],
            background_slots: vec![1],
            ..local(5)
        }
        .validate("l")
        .unwrap_err();
        assert!(e.contains("interactive_slots") && e.contains("重复"), "{e}");
        let e = BackendConfig {
            interactive_slots: vec![],
            background_slots: vec![1],
            ..local(5)
        }
        .validate("l")
        .unwrap_err();
        assert!(e.contains("至少要有一个前台槽位"), "{e}");
        let e = local(0).validate("l").unwrap_err();
        assert!(e.contains("slots"), "{e}");
    }

    #[test]
    fn llama_cpp_rejects_remote_limit_fields_and_openai_rejects_slots() {
        let e = BackendConfig {
            max_inflight: 4,
            ..local(5)
        }
        .validate("l")
        .unwrap_err();
        assert!(e.contains("max_inflight"), "{e}");
        let cloud = BackendConfig {
            base_url: "https://c/v1".into(),
            slots: 2,
            ..Default::default()
        };
        assert!(cloud.validate("c").unwrap_err().contains("没有槽位概念"));
    }

    #[test]
    fn openai_reserved_must_fit_inflight() {
        let ok = BackendConfig {
            base_url: "https://c/v1".into(),
            max_inflight: 8,
            reserved_interactive: 1,
            ..Default::default()
        };
        assert!(ok.validate("c").is_ok());
        assert!(ok.accepts_background());
        let bad = BackendConfig {
            reserved_interactive: 9,
            ..ok.clone()
        };
        assert!(bad
            .validate("c")
            .unwrap_err()
            .contains("reserved_interactive"));
        let bad = BackendConfig {
            max_inflight: 0,
            reserved_interactive: 1,
            ..ok.clone()
        };
        assert!(bad.validate("c").unwrap_err().contains("max_inflight"));
        // 全部并发都预留给前台：不接后台
        let all = BackendConfig {
            max_inflight: 2,
            reserved_interactive: 2,
            ..ok
        };
        assert!(!all.accepts_background());
    }

    #[test]
    fn empty_base_url_is_rejected_and_describe_hides_secrets() {
        assert!(BackendConfig::default()
            .validate("x")
            .unwrap_err()
            .contains("base_url"));
        let c = BackendConfig {
            base_url: "https://c/v1".into(),
            api_key_env: Some("MY_KEY".into()),
            max_inflight: 8,
            rpm: 60,
            ..Default::default()
        };
        let d = c.describe_limits();
        assert!(d.contains("并发上限 8") && d.contains("rpm 60"), "{d}");
        assert!(!d.contains("MY_KEY"));
    }
}
