//! AgentNexus 编排运行时：把配置组装成可运行的系统。
//!
//! ```text
//!  ConversationRuntime ──delegate──▶ Router(LLM) ──▶ Policy(规则)
//!          ▲                                            │
//!          │ 交付消息(事件)                               ▼
//!      Composer ◀──────────── Orchestrator ──▶ AgentRegistry ─┬─ GeneralAgent
//!                                  │                           ├─ CodingAgent
//!                                  ▼                           └─ ReviewerAgent
//!                          Store(SQLite) + Bus(事件)
//! ```

pub mod backends;
pub mod bus;
pub mod composer;
pub mod config;
pub mod conv_lock;
pub mod conversation;
pub mod dream;
pub mod nodes;
pub mod orchestrator;
pub mod policy;
pub mod prompts;
pub mod registry;
pub mod router;
pub mod store;

use std::sync::Arc;
use std::time::Duration;

use an_agents::{acp, AcpAgent, CodingAgent, GeneralAgent, ReviewerAgent, ToolRegistry};
use an_core::Role;
use an_llm::LlmClient;
use anyhow::Result;

use crate::backends::{Backends, LlmRole};
use crate::bus::Bus;
use crate::composer::Composer;
use crate::config::AppConfig;
use crate::conversation::ConversationRuntime;
use crate::nodes::NodeManager;
use crate::orchestrator::Orchestrator;
use crate::policy::Policy;
use crate::registry::AgentRegistry;
use crate::router::Router;
use crate::store::Store;

/// 组装好的整个系统
pub struct Runtime {
    pub config: AppConfig,
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    pub registry: Arc<AgentRegistry>,
    pub router: Arc<Router>,
    pub orchestrator: Arc<Orchestrator>,
    pub conversation: Arc<ConversationRuntime>,
    /// 局域网节点：发现、配对、心跳
    pub nodes: Arc<NodeManager>,
    /// 最近的 WARN / ERROR（诊断接口用）
    pub diagnostics: Arc<an_observe::Diagnostics>,
    /// 前台（conversation 角色）的 LLM 客户端（健康检查用）
    pub llm: LlmClient,
    /// 后端与路由（角色 → 后端 → 地址 / 模型 / 限流方式）
    pub backends: Arc<Backends>,
}

impl Runtime {
    /// 根据配置创建运行时；`store` 由调用方打开（测试时可以用内存库）
    pub fn new(config: AppConfig, store: Store) -> Result<Self> {
        Self::with_diagnostics(config, store, an_observe::diagnostics_only())
    }

    /// 同上，并接入日志系统的诊断缓冲（`an_observe::init` 返回的那个）
    pub fn with_diagnostics(
        config: AppConfig,
        store: Store,
        diagnostics: Arc<an_observe::Diagnostics>,
    ) -> Result<Self> {
        let store = Arc::new(store);
        let n = store.fail_interrupted_tasks()?;
        if n > 0 {
            tracing::warn!(
                event = "task.interrupted",
                count = n,
                "{n} 个任务在上次运行中被中断，已标记为 FAILED"
            );
        }
        let bus = Arc::new(Bus::new(store.clone()));

        // 按角色创建 LLM 客户端：角色 → 后端（[routing]）→ 地址 / 模型 / 限流方式（[backends]、[models]）
        let backends = Arc::new(Backends::build(&config)?);
        backends.log_bindings();
        let llm_for = |role: LlmRole| backends.client(role);

        // 注册 Agent（顺序即未配置执行组时的默认选择顺序：内置 → Codex → 本机 ACP → 节点）
        let registry = AgentRegistry::new(config.agents.assign_map()?);
        let a = &config.agents;
        let mut tool_names = vec![];
        // 工具清单也要给 Turn Controller 看，否则它不知道"现在几点"可以交给 current_time
        let mut tool_listing = "  （General 未启用，没有可用工具）".to_string();
        if a.general.enabled {
            let mut tools = ToolRegistry::with_builtin(&a.general.workspace_dir)?;
            let timeout = std::time::Duration::from_secs(a.general.websearch_timeout_secs);
            if !tools.register_websearch(&a.general.websearch_command, timeout) {
                tracing::info!("未注册联网工具：{} 不可用", a.general.websearch_command);
            }
            tool_names = tools.names();
            tool_listing = tools
                .infos()
                .iter()
                .map(|t| format!("  - {}：{}", t.name, t.description))
                .collect::<Vec<_>>()
                .join("\n");
            registry.register(Arc::new(GeneralAgent::new(
                llm_for(LlmRole::General),
                tools,
                a.general.max_steps,
                a.general.max_concurrent,
            )));
        }
        let reviewer = a
            .reviewer
            .enabled
            .then(|| Arc::new(ReviewerAgent::new(llm_for(LlmRole::Reviewer))));
        if let Some(r) = &reviewer {
            registry.register(r.clone());
        }
        if a.coding.enabled {
            let mut coding = CodingAgent::new(a.coding.cfg.clone());
            // 直连本地推理服务的 coding agent（pi）：每个任务先申请后台槽位，再通过 AN_LLM_SLOT 交给子进程
            if let Some(name) = &a.coding.cfg.backend {
                coding = coding
                    .with_slot_backend(backends.slot_backend(name, a.coding.cfg.max_concurrent)?);
            }
            let card = an_core::AgentAdapter::card(&coding);
            if let Some(reason) = &card.disabled_reason {
                tracing::warn!(event = "agent.not_installed", code = an_core::codes::AGENT_NOT_INSTALLED, agent_id = %card.id, "{} 不可用：{reason}", card.name);
            }
            registry.register(Arc::new(coding));
        }
        register_acp_agents(&registry, &config);
        // 执行组配置立即校验一次（节点上的 agent 可能稍后才连上，所以只对已知成员校验）
        for role in Role::ALL {
            if let Err(e) = registry.group_ids(role) {
                tracing::error!(
                    event = "pool.invalid",
                    code = e.code,
                    role = role.as_str(),
                    "{}",
                    e.message
                );
            }
        }
        let nodes = NodeManager::new(
            store.clone(),
            registry.clone(),
            format!("AgentNexus@{}", an_core::hostname()),
        );
        nodes.load_saved()?;

        let o = &config.orchestrator;
        let router = Arc::new(Router::new(
            llm_for(LlmRole::Router),
            registry.clone(),
            tool_names,
            Policy {
                approval_risk: o.approval_risk,
            },
        ));
        let orchestrator = Arc::new(Orchestrator::new(
            store.clone(),
            bus.clone(),
            registry.clone(),
            reviewer,
            Composer::new(llm_for(LlmRole::Conversation)),
            o.max_review_rounds,
            Duration::from_secs(o.task_timeout_secs),
        ));
        let conversation = Arc::new(ConversationRuntime::new(conversation::ConversationDeps {
            llm: llm_for(LlmRole::Conversation),
            // 回复模型：没单独配置时与前台判断用同一个模型（含服务地址）
            reply_llm: llm_for(LlmRole::Reply),
            store: store.clone(),
            bus: bus.clone(),
            router: router.clone(),
            orchestrator: orchestrator.clone(),
            history_messages: o.history_messages,
            tools: tool_listing,
            review_mode: config.conversation.review_requests,
            stream_replies: config.conversation.stream_replies,
        }));
        tracing::info!(
            event = "conversation.config",
            review_requests = config.conversation.review_requests.as_str(),
            stream_replies = config.conversation.stream_replies
        );

        Ok(Self {
            config,
            store,
            bus,
            registry,
            router,
            orchestrator,
            conversation,
            nodes,
            diagnostics,
            llm: backends.client(LlmRole::Conversation),
            backends: backends.clone(),
        })
    }

    /// 启动后台任务：节点心跳、局域网发现。由 main 在 tokio 运行时中调用
    pub fn start_background(&self) {
        self.backends.spawn_probes();
        self.nodes
            .start_heartbeat(Duration::from_secs(self.config.nodes.refresh_secs.max(3)));
        if self.config.nodes.discovery {
            self.nodes.start_discovery();
        }
    }
}

/// 注册 ACP agent：先注册自定义配置，再注册本机已安装的预设（id 相同时自定义优先）。
/// 未安装的预设不注册，避免界面里出现一堆不可用的 agent；自定义的即使未安装也注册，方便看到原因。
fn register_acp_agents(registry: &AgentRegistry, config: &AppConfig) {
    // 未指定工作目录时：coding 角色用代码目录，其他角色用文件工作目录
    let default_workdir = |role: Role| match role {
        Role::Coding => config.agents.coding.cfg.workdir.clone(),
        _ => config.agents.general.workspace_dir.clone(),
    };
    for cfg in &config.acp.agents {
        let agent = AcpAgent::new(cfg.clone(), default_workdir(cfg.role));
        if !agent.is_installed() {
            tracing::warn!(event = "agent.not_installed", code = an_core::codes::AGENT_NOT_INSTALLED, agent_id = %cfg.id, "ACP agent 不可用：{}", agent_reason(&agent));
        }
        registry.register(Arc::new(agent));
    }
    if config.acp.auto_detect {
        for cfg in acp::presets() {
            let agent = AcpAgent::new(cfg.clone(), default_workdir(cfg.role));
            if agent.is_installed() && registry.register(Arc::new(agent)) {
                tracing::info!(event = "agent.detected", agent_id = %cfg.id, role = cfg.role.as_str(), "自动发现 ACP agent");
            }
        }
    }
}

fn agent_reason(agent: &AcpAgent) -> String {
    an_core::AgentAdapter::card(agent)
        .disabled_reason
        .clone()
        .unwrap_or_default()
}
