//! 后端与路由：把配置里的 `[backends]` / `[routing]` / `[models]` 解析成"每个角色用哪个 LLM 客户端"。
//!
//! 规则（详见 docs/PLAN.md "后端与调度配置化"）：
//! - 没有 `[backends]` 时，用 `[llm]` 合成名为 `default` 的 `openai` 后端（不限并发），旧配置行为不变；
//! - 角色没写路由时：有名为 `default` 的后端就用它，否则只有一个后端时用它，否则报错；
//! - 前台角色（conversation / reply / router）优先级高于后台角色（general / reviewer）；
//! - 后台角色路由到没有后台容量的后端，启动时报错；
//! - 密钥只从 `api_key_env` 指定的环境变量读取，错误信息和日志里都不出现密钥。

use std::collections::BTreeMap;
use std::sync::Arc;

use an_llm::{Backend, BackendConfig, BackendKind, LlmClient, Priority, ServerMetrics, StatsSummary};
use anyhow::{bail, Result};
use serde::Serialize;

use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmRole {
    Conversation,
    Reply,
    Router,
    General,
    Reviewer,
}

impl LlmRole {
    pub const ALL: [LlmRole; 5] = [LlmRole::Conversation, LlmRole::Reply, LlmRole::Router, LlmRole::General, LlmRole::Reviewer];

    pub fn as_str(self) -> &'static str {
        match self {
            LlmRole::Conversation => "conversation",
            LlmRole::Reply => "reply",
            LlmRole::Router => "router",
            LlmRole::General => "general",
            LlmRole::Reviewer => "reviewer",
        }
    }

    /// 前台角色（用户正在等回复）优先于后台角色
    pub fn priority(self) -> Priority {
        match self {
            LlmRole::Conversation | LlmRole::Reply | LlmRole::Router => Priority::Interactive,
            LlmRole::General | LlmRole::Reviewer => Priority::Background,
        }
    }
}

/// 每个角色最终用哪个后端 / 模型 / 限流方式（启动日志和诊断用，不含密钥）
#[derive(Debug, Clone, Serialize)]
pub struct RoleBinding {
    pub role: LlmRole,
    pub backend: String,
    pub kind: &'static str,
    pub base_url: String,
    pub model: String,
    pub priority: &'static str,
    pub limits: String,
}

/// 一个槽位：调度器看到的占用 + 服务端是否正在生成
#[derive(Debug, Clone, Serialize)]
pub struct SlotStatus {
    pub id: u32,
    /// interactive / background / shared / unused / unknown
    pub class: String,
    /// 调度器记录的占用者（角色名）
    pub holder: Option<String>,
    pub held_ms: u64,
    /// 服务端是否正在这个槽位上生成；读不到 `/slots` 时为 None
    pub server_busy: Option<bool>,
    /// 该槽位处理过的任务数（llama.cpp 的 `id_task`；这个版本不提供已生成 token 数）
    pub tasks: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchedStatus {
    pub inflight: u32,
    pub max_inflight: u32,
    pub queue_len: usize,
    pub paused_ms: u64,
}

/// 一个后端的健康状况（`/api/health` 和 `/api/diagnostics` 共用）
#[derive(Debug, Clone, Serialize)]
pub struct BackendStatus {
    pub name: String,
    pub kind: &'static str,
    pub base_url: String,
    pub model: String,
    pub roles: Vec<&'static str>,
    pub ok: bool,
    pub server_models: Vec<String>,
    pub error: Option<String>,
    /// 读 `/slots` 或 `/metrics` 失败的原因
    pub server_error: Option<String>,
    pub slots: Vec<SlotStatus>,
    pub metrics: Option<ServerMetrics>,
    pub scheduler: SchedStatus,
    pub calls: StatsSummary,
}

pub struct Backends {
    backends: BTreeMap<String, Arc<Backend>>,
    clients: BTreeMap<LlmRole, LlmClient>,
    bindings: Vec<RoleBinding>,
}

type EnvFn<'a> = &'a dyn Fn(&str) -> Option<String>;

impl Backends {
    pub fn build(cfg: &AppConfig) -> Result<Self> {
        Self::build_with_env(cfg, &|k| std::env::var(k).ok())
    }

    /// `env` 用来读取 `api_key_env` 指向的环境变量（测试里可以注入）
    pub fn build_with_env(cfg: &AppConfig, env: EnvFn) -> Result<Self> {
        let legacy_urls = &cfg.models.base_urls;
        let has_legacy_urls = [&legacy_urls.conversation, &legacy_urls.reply, &legacy_urls.router, &legacy_urls.general, &legacy_urls.reviewer].iter().any(|u| u.is_some());
        if !cfg.backends.is_empty() && has_legacy_urls {
            bail!("[models.base_urls] 是旧写法，不能和 [backends] 同时使用；请改用 [backends.<名字>] 和 [routing]");
        }

        // 1. 后端：没写就用 [llm] 合成 default
        let mut specs: BTreeMap<String, BackendConfig> = cfg.backends.clone();
        if specs.is_empty() {
            specs.insert("default".into(), BackendConfig { base_url: cfg.llm.base_url.clone(), ..Default::default() });
        }
        for (name, spec) in &specs {
            spec.validate(name).map_err(anyhow::Error::msg)?;
        }

        // 2. 每个后端一个基础客户端（共享 HTTP 连接池）
        let mut backends = BTreeMap::new();
        let mut base_clients = BTreeMap::new();
        for (name, spec) in &specs {
            let mut llm = cfg.llm.clone();
            llm.base_url = spec.base_url.clone();
            if let Some(m) = &spec.model {
                llm.model = m.clone();
            }
            if let Some(mode) = spec.structured_mode {
                llm.structured_mode = mode;
            }
            if let Some(t) = spec.total_secs {
                llm.timeout_secs = t;
            }
            if let Some(var) = spec.api_key_env.as_deref().filter(|v| !v.is_empty()) {
                match env(var) {
                    Some(key) if !key.trim().is_empty() => llm.api_key = key,
                    _ => bail!("[backends.{name}] api_key_env：环境变量 {var} 未设置或为空（密钥不要写进配置文件，请在启动前 export {var}=...）"),
                }
            }
            let backend = Arc::new(Backend::new(name.clone(), spec.clone()));
            backends.insert(name.clone(), backend);
            base_clients.insert(name.clone(), LlmClient::new(llm)?);
        }

        // 3. 角色 → 后端
        let r = &cfg.routing;
        let route_of = |role: LlmRole| -> Option<&Option<String>> {
            Some(match role {
                LlmRole::Conversation => &r.conversation,
                LlmRole::Reply => &r.reply,
                LlmRole::Router => &r.router,
                LlmRole::General => &r.general,
                LlmRole::Reviewer => &r.reviewer,
            })
        };
        let mut clients = BTreeMap::new();
        let mut bindings = vec![];
        for role in LlmRole::ALL {
            let name = match route_of(role).and_then(|o| o.as_deref()) {
                Some(n) => n.to_string(),
                // reply 没写时跟 conversation 一致
                None if role == LlmRole::Reply && r.conversation.is_some() => r.conversation.clone().unwrap_or_default(),
                None => Self::default_backend(&specs, role)?,
            };
            let Some(spec) = specs.get(&name) else {
                let known = specs.keys().cloned().collect::<Vec<_>>().join(", ");
                bail!("[routing] {} = \"{name}\"：没有这个后端（已定义：{known}）", role.as_str());
            };
            if role.priority() == Priority::Background && !spec.accepts_background() {
                bail!("[routing] {} = \"{name}\"：这个后端没有后台容量（后台槽位 / 并发许可都被前台占满），请换一个后端或修改 [backends.{name}]", role.as_str());
            }
            let mut client = base_clients[&name].with_backend(backends[&name].clone(), role.as_str(), role.priority());
            // 旧写法：[models] 覆盖模型名；[models.base_urls] 覆盖地址（只在没有 [backends] 时有效）
            let (model, url) = match role {
                LlmRole::Reply => cfg.models.reply_target(),
                LlmRole::Conversation => (cfg.models.conversation.as_deref(), legacy_urls.conversation.as_deref()),
                LlmRole::Router => (cfg.models.router.as_deref(), legacy_urls.router.as_deref()),
                LlmRole::General => (cfg.models.general.as_deref(), legacy_urls.general.as_deref()),
                LlmRole::Reviewer => (cfg.models.reviewer.as_deref(), legacy_urls.reviewer.as_deref()),
            };
            if let Some(u) = url {
                client = client.with_base_url(u);
            }
            if let Some(m) = model {
                client = client.with_model(m);
            }
            if matches!(role, LlmRole::Conversation | LlmRole::Router) && spec.front_structured_thinking == Some(false) {
                client = client.disable_structured_thinking();
            }
            bindings.push(RoleBinding {
                role,
                backend: name.clone(),
                kind: spec.kind.as_str(),
                base_url: client.config().base_url.clone(),
                model: client.model().to_string(),
                priority: role.priority().as_str(),
                limits: spec.describe_limits(),
            });
            clients.insert(role, client);
        }
        Ok(Self { backends, clients, bindings })
    }

    fn default_backend(specs: &BTreeMap<String, BackendConfig>, role: LlmRole) -> Result<String> {
        if specs.contains_key("default") {
            return Ok("default".into());
        }
        if specs.len() == 1 {
            return Ok(specs.keys().next().cloned().unwrap_or_default());
        }
        let known = specs.keys().cloned().collect::<Vec<_>>().join(", ");
        bail!("[routing] 没有给 {} 指定后端，也没有名为 default 的后端可用（已定义：{known}）；请写 {} = \"后端名\"", role.as_str(), role.as_str())
    }

    pub fn client(&self, role: LlmRole) -> LlmClient {
        self.clients[&role].clone()
    }

    pub fn bindings(&self) -> &[RoleBinding] {
        &self.bindings
    }

    pub fn backend(&self, name: &str) -> Option<&Arc<Backend>> {
        self.backends.get(name)
    }

    pub fn backends(&self) -> impl Iterator<Item = &Arc<Backend>> {
        self.backends.values()
    }

    /// 每个实际在用的后端的健康状况：连通性（缓存 8 秒）、llama.cpp 的 `/slots` 与 `/metrics`、调度器快照、最近调用的 P50/P95
    pub async fn status(&self) -> Vec<BackendStatus> {
        let mut groups: BTreeMap<(String, String), Vec<LlmRole>> = BTreeMap::new();
        for b in &self.bindings {
            groups.entry((b.backend.clone(), b.base_url.clone())).or_default().push(b.role);
        }
        let mut out = Vec::new();
        for ((name, url), roles) in groups {
            let client = self.clients[&roles[0]].clone();
            let (ok, server_models, error) = match client.list_models().await {
                Ok(m) => (true, m, None),
                Err(e) => (false, vec![], Some(format!("{e:#}"))),
            };
            let (view, server_error) = match client.server_view().await {
                None => (None, None),
                Some(Ok(v)) => (Some(v), None),
                Some(Err(e)) => (None, Some(e)),
            };
            let backend = &self.backends[&name];
            let snap = backend.sched.snapshot();
            let mut slots: Vec<SlotStatus> = snap
                .slots
                .iter()
                .map(|s| {
                    let srv = view.as_ref().and_then(|v| v.slots.iter().find(|x| x.id == s.id));
                    SlotStatus { id: s.id, class: s.class.to_string(), holder: s.holder.clone(), held_ms: s.held_ms, server_busy: srv.map(|x| x.busy), tasks: srv.map(|x| x.tasks) }
                })
                .collect();
            if let Some(v) = &view {
                for x in &v.slots {
                    if !slots.iter().any(|s| s.id == x.id) {
                        slots.push(SlotStatus { id: x.id, class: "unknown".into(), holder: None, held_ms: 0, server_busy: Some(x.busy), tasks: Some(x.tasks) });
                    }
                }
            }
            let binding = self.bindings.iter().find(|b| b.backend == name && b.base_url == url);
            out.push(BackendStatus {
                name,
                kind: binding.map(|b| b.kind).unwrap_or("openai"),
                model: binding.map(|b| b.model.clone()).unwrap_or_default(),
                roles: roles.iter().map(|r| r.as_str()).collect(),
                base_url: url,
                ok,
                server_models,
                error,
                server_error,
                slots,
                metrics: view.map(|v| v.metrics),
                scheduler: SchedStatus { inflight: snap.inflight, max_inflight: snap.max_inflight, queue_len: snap.queue_len, paused_ms: snap.paused_ms },
                calls: backend.stats.summary(),
            });
        }
        out
    }

    /// 给 coding agent（pi 等直连推理服务的外部进程）选定要固定槽位的后端，并校验：
    /// 必须是 `llama_cpp` 后端、有后台槽位、`max_concurrent` 不超过后台槽位数（否则多出来的任务只会在租约上排队）
    pub fn slot_backend(&self, name: &str, max_concurrent: u32) -> Result<Arc<Backend>> {
        let Some(b) = self.backends.get(name) else {
            bail!("[agents.coding] backend = \"{name}\"：没有这个后端（已定义：{}）", self.backends.keys().cloned().collect::<Vec<_>>().join("、"));
        };
        if b.cfg.kind != BackendKind::LlamaCpp {
            bail!("[agents.coding] backend = \"{name}\"：只有 kind = \"llama_cpp\" 的后端有槽位可以固定；云端模型请不要配置 backend");
        }
        let background = b.cfg.effective_slots().1.len() as u32;
        if background == 0 {
            bail!("[agents.coding] backend = \"{name}\"：这个后端没有后台槽位（background_slots），coding 任务无处可放");
        }
        if max_concurrent > background {
            bail!("[agents.coding] max_concurrent = {max_concurrent} 超过后端 \"{name}\" 的后台槽位数 {background}，请调小 max_concurrent（建议 ≤ 2，给 general / reviewer 留槽位）");
        }
        Ok(b.clone())
    }

    /// 在后台探测每个本地推理服务（`/props`）：读取上下文总量用于预算裁剪，并核对槽位数。
    /// 服务还没起来时每 5 秒重试，最多 6 次；之后靠健康检查提示。需在 tokio 运行时中调用。
    pub fn spawn_probes(&self) {
        for b in self.backends.values().filter(|b| b.cfg.kind == BackendKind::LlamaCpp) {
            let b = b.clone();
            tokio::spawn(async move {
                for attempt in 1..=6 {
                    match b.probe().await {
                        Ok(_) => return,
                        Err(e) if attempt == 6 => tracing::warn!(event = "llm.backend_probe_failed", code = an_core::codes::LLM_UNREACHABLE, backend = %b.name, error = %format!("{e:#}"), "无法读取后端 /props，上下文预算裁剪暂不生效（超长时仍会由服务端报错后缩减重试）"),
                        Err(_) => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
                    }
                }
            });
        }
    }

    /// 启动时打一条日志：每个角色 → 后端 / 模型 / 限流方式（不含密钥）
    pub fn log_bindings(&self) {
        for b in &self.bindings {
            tracing::info!(event = "config.backends", role = b.role.as_str(), backend = %b.backend, kind = b.kind, base_url = %b.base_url, model = %b.model, priority = b.priority, limits = %b.limits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LLM: &str = "[llm]\nbase_url = \"http://legacy/v1\"\nmodel = \"legacy-model\"\ntemperature = 0.3\n";

    fn cfg(extra: &str) -> AppConfig {
        let (c, unknown) = AppConfig::parse(&format!("{LLM}{extra}")).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        c
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn build(extra: &str) -> Result<Backends> {
        Backends::build_with_env(&cfg(extra), &no_env)
    }

    fn err(extra: &str) -> String {
        match build(extra) {
            Ok(_) => panic!("应该报错"),
            Err(e) => format!("{e:#}"),
        }
    }

    const PROFILE_A: &str = "[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://127.0.0.1:8200/v1\"\nmodel = \"big\"\nslots = 5\n\
        interactive_slots = [0]\nbackground_slots = [1, 2, 3, 4]\n\
        [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"local\"\nreviewer = \"local\"\n";

    const CLOUD: &str = "[backends.cloud]\nkind = \"openai\"\nbase_url = \"https://cloud/v1\"\nmodel = \"cloud-model\"\napi_key_env = \"CLOUD_KEY\"\nmax_inflight = 8\nreserved_interactive = 1\nrpm = 60\n";

    #[test]
    fn coding_backend_must_be_a_llama_cpp_backend_with_enough_background_slots() {
        let b = build(PROFILE_A).unwrap();
        assert_eq!(b.slot_backend("local", 2).unwrap().name, "local");
        assert_eq!(b.slot_backend("local", 4).unwrap().name, "local");
        let e = b.slot_backend("local", 5).unwrap_err().to_string();
        assert!(e.contains("max_concurrent") && e.contains('4'), "{e}");
        let e = b.slot_backend("nope", 1).unwrap_err().to_string();
        assert!(e.contains("nope") && e.contains("local"), "{e}");

        // 云端后端没有槽位可固定
        let both = format!("{PROFILE_A}{CLOUD}");
        let b = Backends::build_with_env(&cfg(&both), &|k: &str| (k == "CLOUD_KEY").then(|| "x".to_string())).unwrap();
        let e = b.slot_backend("cloud", 1).unwrap_err().to_string();
        assert!(e.contains("llama_cpp"), "{e}");
    }

    #[test]
    fn coding_backend_without_background_slots_is_rejected() {
        let front_only = "[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://x/v1\"\nslots = 2\ninteractive_slots = [0, 1]\nbackground_slots = []\n\
            [backends.other]\nbase_url = \"https://o/v1\"\n\
            [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"other\"\nreviewer = \"other\"\n";
        let b = build(front_only).unwrap();
        let e = b.slot_backend("local", 1).unwrap_err().to_string();
        assert!(e.contains("后台槽位"), "{e}");
    }

    #[test]
    fn legacy_config_synthesizes_default_backend() {
        let b = build("").unwrap();
        assert_eq!(b.backends().count(), 1);
        for role in LlmRole::ALL {
            let c = b.client(role);
            assert_eq!(c.config().base_url, "http://legacy/v1");
            assert_eq!(c.model(), "legacy-model");
            assert_eq!(c.backend().unwrap().name, "default");
            assert_eq!(c.role(), role.as_str());
        }
        assert_eq!(b.bindings().len(), 5);
        assert_eq!(b.client(LlmRole::General).priority(), Priority::Background);
        assert_eq!(b.client(LlmRole::Router).priority(), Priority::Interactive);
    }

    #[test]
    fn front_structured_thinking_off_only_applies_to_conversation_and_router() {
        let src = "[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://x/v1\"\nslots = 4\nfront_structured_thinking = false\n\
            [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"local\"\nreviewer = \"local\"\n";
        let b = build(src).unwrap();
        assert!(b.client(LlmRole::Conversation).structured_thinking_disabled());
        assert!(b.client(LlmRole::Router).structured_thinking_disabled());
        assert!(!b.client(LlmRole::Reply).structured_thinking_disabled());
        assert!(!b.client(LlmRole::General).structured_thinking_disabled());
        assert!(!b.client(LlmRole::Reviewer).structured_thinking_disabled());
    }

    #[test]
    fn models_override_names_on_top_of_backend() {
        let b = build("[models]\nconversation = \"small\"\n").unwrap();
        assert_eq!(b.client(LlmRole::Conversation).model(), "small");
        assert_eq!(b.client(LlmRole::Reply).model(), "small"); // reply 跟 conversation
        assert_eq!(b.client(LlmRole::General).model(), "legacy-model");
    }

    #[test]
    fn legacy_base_urls_still_work_without_backends() {
        let b = build("[models]\nconversation = \"m\"\n[models.base_urls]\nconversation = \"http://other/v1\"\n").unwrap();
        assert_eq!(b.client(LlmRole::Conversation).config().base_url, "http://other/v1");
        assert_eq!(b.client(LlmRole::Reply).config().base_url, "http://other/v1");
        assert_eq!(b.client(LlmRole::General).config().base_url, "http://legacy/v1");
    }

    #[test]
    fn base_urls_and_backends_cannot_be_mixed() {
        let e = err(&format!("{PROFILE_A}[models.base_urls]\nconversation = \"http://x/v1\"\n"));
        assert!(e.contains("旧写法"), "{e}");
    }

    #[test]
    fn profile_a_all_local_routes_every_role_to_local() {
        let b = build(PROFILE_A).unwrap();
        for role in LlmRole::ALL {
            let c = b.client(role);
            assert_eq!(c.backend().unwrap().name, "local");
            assert_eq!(c.config().base_url, "http://127.0.0.1:8200/v1");
            assert_eq!(c.model(), "big");
        }
        // reply 没写路由，跟 conversation
        let reply = b.bindings().iter().find(|x| x.role == LlmRole::Reply).unwrap();
        assert_eq!(reply.backend, "local");
        assert!(reply.limits.contains("槽位 5 个"), "{}", reply.limits);
    }

    #[test]
    fn profile_b_local_front_cloud_tasks() {
        let extra = format!(
            "[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://127.0.0.1:8200/v1\"\nmodel = \"big\"\nslots = 2\n\
             interactive_slots = [0, 1]\nbackground_slots = []\n{CLOUD}\
             [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"cloud\"\nreviewer = \"cloud\"\n"
        );
        let c = cfg(&extra);
        let env = |k: &str| (k == "CLOUD_KEY").then(|| "secret-value".to_string());
        let b = Backends::build_with_env(&c, &env).unwrap();
        let g = b.client(LlmRole::General);
        assert_eq!(g.config().base_url, "https://cloud/v1");
        assert_eq!(g.model(), "cloud-model");
        assert_eq!(g.config().api_key, "secret-value");
        assert_eq!(b.client(LlmRole::Conversation).config().base_url, "http://127.0.0.1:8200/v1");
        assert_eq!(b.client(LlmRole::Conversation).config().api_key, "");
        // 绑定描述里不能出现密钥
        let dump = serde_json::to_string(b.bindings()).unwrap();
        assert!(!dump.contains("secret-value"), "{dump}");
    }

    #[test]
    fn profile_c_mixed_roles() {
        let extra = format!("{PROFILE_A}{CLOUD}").replace("general = \"local\"", "general = \"cloud\"");
        let env = |_: &str| Some("k".to_string());
        let b = Backends::build_with_env(&cfg(&extra), &env).unwrap();
        assert_eq!(b.client(LlmRole::General).backend().unwrap().name, "cloud");
        assert_eq!(b.client(LlmRole::Reviewer).backend().unwrap().name, "local");
    }

    #[test]
    fn unknown_backend_in_routing_names_role_and_choices() {
        let e = err(&PROFILE_A.to_string().replace("router = \"local\"", "router = \"loca1\""));
        assert!(e.contains("router") && e.contains("loca1") && e.contains("local"), "{e}");
    }

    #[test]
    fn background_role_on_front_only_backend_is_rejected() {
        let extra = "[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://x/v1\"\nslots = 2\ninteractive_slots = [0, 1]\nbackground_slots = []\n\
            [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"local\"\nreviewer = \"local\"\n";
        let e = err(extra);
        assert!(e.contains("general") && e.contains("没有后台容量"), "{e}");
    }

    #[test]
    fn missing_env_var_names_the_variable_but_not_secrets() {
        let e = err(&format!("{CLOUD}[routing]\nconversation = \"cloud\"\nrouter = \"cloud\"\ngeneral = \"cloud\"\nreviewer = \"cloud\"\n"));
        assert!(e.contains("CLOUD_KEY") && e.contains("未设置"), "{e}");
    }

    #[test]
    fn invalid_slots_are_reported_with_backend_name() {
        let e = err("[backends.local]\nkind = \"llama_cpp\"\nbase_url = \"http://x/v1\"\nslots = 3\ninteractive_slots = [0]\nbackground_slots = [1, 3]\n");
        assert!(e.contains("[backends.local]") && e.contains("超出范围"), "{e}");
    }

    #[test]
    fn unrouted_role_needs_a_default_or_a_single_backend() {
        // 只有一个后端：不写路由也行
        let one = "[backends.only]\nkind = \"llama_cpp\"\nbase_url = \"http://x/v1\"\nslots = 3\n";
        let b = build(one).unwrap();
        assert_eq!(b.client(LlmRole::General).backend().unwrap().name, "only");
        // 两个后端、没有 default、也没写路由：报错并说明该怎么写
        let env = |_: &str| Some("k".to_string());
        let two = format!("{one}{CLOUD}");
        let e = Backends::build_with_env(&cfg(&two), &env).err().map(|e| e.to_string()).unwrap();
        assert!(e.contains("conversation") && e.contains("后端名"), "{e}");
        // 有 default：未写的角色用它
        let with_default = two.replace("[backends.only]", "[backends.default]");
        let b = Backends::build_with_env(&cfg(&with_default), &env).unwrap();
        assert_eq!(b.client(LlmRole::Reviewer).backend().unwrap().name, "default");
    }

    #[test]
    fn unknown_fields_inside_backends_are_reported() {
        let (_, unknown) = AppConfig::parse(&format!("{LLM}[backends.local]\nbase_url = \"x\"\nslotz = 3\n[routing]\nconvo = \"local\"\n")).unwrap();
        assert_eq!(unknown, ["backends.local.slotz", "routing.convo"]);
    }

    #[test]
    fn backend_overrides_structured_mode_total_timeout_and_model() {
        let extra = "[backends.b]\nbase_url = \"http://b/v1\"\nmodel = \"bm\"\nstructured_mode = \"prompt_only\"\ntotal_secs = 33\n";
        let b = build(extra).unwrap();
        let c = b.client(LlmRole::Router);
        assert_eq!(c.config().timeout_secs, 33);
        assert_eq!(c.model(), "bm");
        assert_eq!(c.config().structured_mode, an_llm::StructuredMode::PromptOnly);
        assert_eq!(c.config().temperature, 0.3); // 其余沿用 [llm]
    }
}
