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

pub mod bus;
pub mod composer;
pub mod config;
pub mod conversation;
pub mod orchestrator;
pub mod policy;
pub mod registry;
pub mod router;
pub mod store;

use std::sync::Arc;
use std::time::Duration;

use an_agents::{acp, AcpAgent, CodingAgent, GeneralAgent, ReviewerAgent, ToolRegistry};
use an_core::Role;
use an_llm::LlmClient;
use anyhow::Result;

use crate::bus::Bus;
use crate::composer::Composer;
use crate::config::AppConfig;
use crate::conversation::ConversationRuntime;
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
    /// 默认 LLM 客户端（健康检查用）
    pub llm: LlmClient,
}

impl Runtime {
    /// 根据配置创建运行时；`store` 由调用方打开（测试时可以用内存库）
    pub fn new(config: AppConfig, store: Store) -> Result<Self> {
        let store = Arc::new(store);
        let n = store.fail_interrupted_tasks()?;
        if n > 0 {
            tracing::warn!("{n} 个任务在上次运行中被中断，已标记为 FAILED");
        }
        let bus = Arc::new(Bus::new(store.clone()));

        // 按角色创建 LLM 客户端（未单独配置的角色使用默认模型）
        let base = LlmClient::new(config.llm.clone())?;
        let llm_for = |o: &Option<String>| o.as_deref().map(|m| base.with_model(m)).unwrap_or_else(|| base.clone());
        let m = &config.models;

        // 注册 Agent（顺序即默认优先级：内置 / Codex 在前，ACP agent 在后）
        let mut registry = AgentRegistry::new(config.agents.prefer.clone());
        let a = &config.agents;
        let mut tool_names = vec![];
        if a.general.enabled {
            let tools = ToolRegistry::with_builtin(&a.general.workspace_dir)?;
            tool_names = tools.names();
            registry.register(Arc::new(GeneralAgent::new(llm_for(&m.general), tools, a.general.max_steps)));
        }
        let reviewer = a.reviewer.enabled.then(|| Arc::new(ReviewerAgent::new(llm_for(&m.reviewer))));
        if let Some(r) = &reviewer {
            registry.register(r.clone());
        }
        if a.coding.enabled {
            let coding = CodingAgent::new(a.coding.cfg.clone());
            if let Some(reason) = &an_core::AgentAdapter::card(&coding).disabled_reason {
                tracing::warn!("Coding Agent 不可用：{reason}");
            }
            registry.register(Arc::new(coding));
        }
        register_acp_agents(&mut registry, &config);
        for (role, id) in registry.prefer() {
            if !registry.cards().iter().any(|c| &c.id == id && c.role.as_str() == role) {
                tracing::warn!("[agents.prefer] {role} = \"{id}\" 没有匹配的 agent，将按默认顺序选择");
            }
        }
        let registry = Arc::new(registry);

        let o = &config.orchestrator;
        let router = Arc::new(Router::new(llm_for(&m.router), registry.clone(), tool_names, Policy { approval_risk: o.approval_risk }));
        let orchestrator = Arc::new(Orchestrator::new(
            store.clone(),
            bus.clone(),
            registry.clone(),
            reviewer,
            Composer::new(llm_for(&m.conversation)),
            o.max_review_rounds,
            Duration::from_secs(o.task_timeout_secs),
        ));
        let conversation = Arc::new(ConversationRuntime::new(
            llm_for(&m.conversation),
            store.clone(),
            router.clone(),
            orchestrator.clone(),
            o.history_messages,
        ));

        Ok(Self { config, store, bus, registry, router, orchestrator, conversation, llm: base })
    }
}

/// 注册 ACP agent：先注册自定义配置，再注册本机已安装的预设（id 相同时自定义优先）。
/// 未安装的预设不注册，避免界面里出现一堆不可用的 agent；自定义的即使未安装也注册，方便看到原因。
fn register_acp_agents(registry: &mut AgentRegistry, config: &AppConfig) {
    // 未指定工作目录时：coding 角色用代码目录，其他角色用文件工作目录
    let default_workdir = |role: Role| match role {
        Role::Coding => config.agents.coding.cfg.workdir.clone(),
        _ => config.agents.general.workspace_dir.clone(),
    };
    for cfg in &config.acp.agents {
        let agent = AcpAgent::new(cfg.clone(), default_workdir(cfg.role));
        if !agent.is_installed() {
            tracing::warn!("ACP agent {} 不可用：{}", cfg.id, agent_reason(&agent));
        }
        registry.register(Arc::new(agent));
    }
    if config.acp.auto_detect {
        for cfg in acp::presets() {
            let agent = AcpAgent::new(cfg.clone(), default_workdir(cfg.role));
            if agent.is_installed() && registry.register(Arc::new(agent)) {
                tracing::info!("自动发现 ACP agent：{}（角色 {}）", cfg.id, cfg.role.as_str());
            }
        }
    }
}

fn agent_reason(agent: &AcpAgent) -> String {
    an_core::AgentAdapter::card(agent).disabled_reason.clone().unwrap_or_default()
}
