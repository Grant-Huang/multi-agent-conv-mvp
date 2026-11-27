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

pub mod admission;
pub mod agent_catalog;
pub mod agent_chat;
pub mod agent_config;
pub mod agent_diagnose;
pub mod backends;
pub mod builtin_templates;
pub mod bus;
pub mod composer;
pub mod config;
pub mod connectors;
pub mod conv_lock;
pub mod conversation;
pub mod dream;
pub mod introspect;
pub mod link;
pub mod mom;
pub mod nodes;
pub mod orchestrator;
pub mod policy;
pub mod prompts;
pub mod registry;
pub mod router;
pub mod run_progress;
pub mod session_materials;
pub mod store;
pub mod template;

use std::sync::Arc;
use std::time::Duration;

use an_agents::{acp, AcpAgent, CodingAgent, GeneralAgent, ReviewerAgent, ToolRegistry};
use an_core::Role;
use an_llm::LlmClient;
use anyhow::{Context, Result};

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
    /// Fixed startup snapshot shared by discovery, prompts and the capability API.
    pub capabilities: Vec<an_agents::capability::CapabilityCard>,
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
    /// 需要后台探测连通性的 agent（ACP 与 CLI）
    pub probers: Arc<ProberSet>,
    /// agent 准入账本（`[admission] enabled = true` 时才有）
    pub admission: Option<Arc<admission::AdmissionLedger>>,
    /// 握手检测接口的并发闸门（AG.1）
    pub probe_gate: Arc<agent_catalog::ProbeGate>,
    /// 托管配置（AG.2）
    pub agent_config: Arc<agent_config::AgentConfigManager>,
    /// 管理助手（AG.4）：HTTP 接口和对话里的向导共用
    pub agents: Arc<agent_chat::AgentAdmin>,
    /// 外发检查守卫（`[egress]` 的 mode 不是 off 时才有）；AG.5 排查第 3、4 层也用它评估、还原、撤销
    pub egress: Option<Arc<an_agents::egress::EgressGuard>>,
    /// 排查第 4 层的同意凭证、并发闸门和时限（AG.5）
    pub diagnose_send: Arc<agent_diagnose::SendState>,
}

/// 连通性探测的目标：只做握手或命令检查，不调用模型（探测结果写入 `registry.links`）
#[derive(Clone)]
pub enum Prober {
    Acp(Arc<AcpAgent>),
    /// CLI agent（pi）；`backend` 为它依赖的本地模型后端名，有的话还要求该后端已连通
    Cli {
        agent: Arc<CodingAgent>,
        backend: Option<String>,
    },
}

impl Prober {
    fn id(&self) -> String {
        match self {
            Prober::Acp(a) => an_core::AgentAdapter::card(a.as_ref()).id.clone(),
            Prober::Cli { agent, .. } => an_core::AgentAdapter::card(agent.as_ref()).id.clone(),
        }
    }
}

/// 需要后台探测的 agent 列表。AG.2 起可以在运行中增删（接入 / 停用托管 agent），
/// 后台探测循环每轮取一次快照。
#[derive(Default)]
pub struct ProberSet {
    list: std::sync::RwLock<Vec<Prober>>,
}

impl ProberSet {
    pub fn new(list: Vec<Prober>) -> Self {
        Self {
            list: std::sync::RwLock::new(list),
        }
    }
    pub fn snapshot(&self) -> Vec<Prober> {
        self.list.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn add(&self, p: Prober) {
        let id = p.id();
        let mut l = self.list.write().unwrap_or_else(|e| e.into_inner());
        l.retain(|x| x.id() != id);
        l.push(p);
    }
    pub fn remove(&self, id: &str) {
        self.list
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|x| x.id() != id);
    }
}

/// 对所有探测目标各探测一次，结果写入 `registry.links`
pub async fn probe_agents(registry: &AgentRegistry, backends: &Backends, probers: &[Prober]) {
    let status = backends.status().await;
    for p in probers {
        let (id, result) = match p {
            Prober::Acp(a) => {
                let id = an_core::AgentAdapter::card(a.as_ref()).id.clone();
                (id, a.probe().await)
            }
            Prober::Cli { agent, backend } => {
                let id = an_core::AgentAdapter::card(agent.as_ref()).id.clone();
                let mut result = agent.probe().await;
                if result.is_ok() {
                    if let Some(name) = backend {
                        // pi 依赖的模型服务也要可用，否则不能算已连通
                        let up = status
                            .iter()
                            .find(|b| &b.name == name)
                            .is_some_and(|b| b.ok);
                        if !up {
                            result = Err(format!("模型服务 {name} 未连通"));
                        }
                    }
                }
                (id, result)
            }
        };
        registry.links.record_probe(&id, result);
    }
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
        anyhow::ensure!(
            config.mcp.servers.is_empty(),
            "MCP requires Runtime::with_mcp_diagnostics().await"
        );
        anyhow::ensure!(
            config.egress.mode == an_agents::egress::EgressMode::Off,
            "[egress] requires Runtime::with_mcp_diagnostics().await"
        );
        Self::assemble(config, store, diagnostics, ToolRegistry::default(), None)
    }

    pub async fn with_mcp_diagnostics(
        config: AppConfig,
        store: Store,
        diagnostics: Arc<an_observe::Diagnostics>,
    ) -> Result<Self> {
        anyhow::ensure!(
            config.agents.general.enabled || config.mcp.servers.is_empty(),
            "MCP requires General to be enabled"
        );
        let tools = an_agents::mcp::connect(&config.mcp.servers).await?;
        // 外发守卫：off 时不启动任何进程；enforce 下 DLP 不可用则拒绝启动
        let egress = an_agents::egress::EgressGuard::start(&config.egress).await?;
        Self::assemble(config, store, diagnostics, tools, egress)
    }

    /// WP7：重启失败恢复——写含原因的 StatusChanged 事件与失败交付（同步落库；启动期无 SSE 消费者）。
    fn write_restart_failure(store: &Arc<Store>, task: &an_core::TaskRecord) {
        let event = an_core::TaskEvent {
            id: an_core::new_id("evt"),
            conversation_id: task.conversation_id.clone(),
            task_id: Some(task.id.clone()),
            kind: an_core::EventKind::StatusChanged,
            status: Some(an_core::TaskStatus::Failed),
            message: "任务因服务重启中断".to_string(),
            stream_id: None,
            created_at: an_core::now(),
        };
        if let Err(e) = store.insert_event(&event) {
            tracing::error!(error = %e, task_id = %task.id, "写入重启中断事件失败");
        }
        if let Err(e) = store.add_message(
            &task.conversation_id,
            "assistant",
            "任务因服务重启中断，可重新委派",
            Some(&task.id),
        ) {
            tracing::error!(error = %e, task_id = %task.id, "写入重启失败交付失败");
        }
    }

    fn assemble(
        config: AppConfig,
        store: Store,
        diagnostics: Arc<an_observe::Diagnostics>,
        mcp_tools: ToolRegistry,
        egress: Option<Arc<an_agents::egress::EgressGuard>>,
    ) -> Result<Self> {
        let store = Arc::new(store);
        // WP7 / G08：启动恢复——上次运行中断的任务置 FAILED，并写含原因的状态事件与失败交付。
        // 不做断点续跑（无步骤级执行状态，重跑会重复副作用）；此时尚无 SSE 消费者，同步落库即可。
        let interrupted = store.recover_interrupted_tasks()?;
        if !interrupted.is_empty() {
            tracing::warn!(
                event = "task.interrupted",
                count = interrupted.len(),
                "{} 个任务在上次运行中被中断，已标记为 FAILED",
                interrupted.len()
            );
        }
        for task in &interrupted {
            Self::write_restart_failure(&store, task);
        }
        let bus = Arc::new(Bus::new(store.clone()));

        // 按角色创建 LLM 客户端：角色 → 后端（[routing]）→ 地址 / 模型 / 限流方式（[backends]、[models]）
        let backends = Arc::new(Backends::build(&config)?);
        backends.log_bindings();
        // 云端后端登记了外发绑定时，该后端的客户端在每次请求前先经外发检查
        let llm_for = |role: LlmRole| {
            let client = backends.client(role);
            let hook = match (&egress, client.backend()) {
                (Some(g), Some(b)) => g.llm_hook(&b.name),
                _ => None,
            };
            match hook {
                Some(h) => client.with_egress(h),
                None => client,
            }
        };
        // 启动校验用：每个角色实际使用的 (后端名, 地址)。旧写法中角色可以覆盖地址，所以按角色列出
        let llm_paths: Vec<(String, String)> = LlmRole::ALL
            .iter()
            .map(|r| {
                let c = backends.client(*r);
                let name = c
                    .backend()
                    .map_or("default".to_string(), |b| b.name.clone());
                (name, c.config().base_url.clone())
            })
            .collect();
        let mut egress_checked = false;

        // 注册 Agent（顺序即未配置执行组时的默认选择顺序：内置 → Codex → 本机 ACP → 节点）
        // 托管配置（AG.2）：读不出来时不影响启动，只记录原因
        let agent_config = Arc::new(agent_config::AgentConfigManager::load(&config));
        let managed = agent_config.managed();
        let registry = AgentRegistry::new(agent_config::merged_assign(
            &config.agents.assign_map()?,
            &managed,
        ));
        // 准入（E3）：开启时打开账本（参数非法则启动失败），registry 按准入等级分配
        let admission = if config.admission.enabled {
            let ledger = Arc::new(admission::AdmissionLedger::open(
                store.clone(),
                config.admission.rules(),
            )?);
            registry.set_admission(ledger.clone());
            // 服务重启：已激活的外部 agent 按登记的接入信息重新注册
            for rec in ledger.list() {
                if rec.stage == admission::Stage::Active {
                    if let Some(agent) =
                        a2a_agent(&rec, config.orchestrator.task_timeout_secs, egress.clone())?
                    {
                        registry.register(agent);
                    }
                }
            }
            Some(ledger)
        } else {
            None
        };
        // 需要后台探测连通性的 agent：ACP 与 CLI（pi）
        let mut probers: Vec<Prober> = Vec::new();
        let a = &config.agents;
        let mut tool_names = vec![];
        let mut capabilities = vec![];
        // 工具清单也要给 Turn Controller 看，否则它不知道"现在几点"可以交给 current_time
        let mut tool_listing = "  （General 未启用，没有可用工具）".to_string();
        if a.general.enabled {
            let mut tools = ToolRegistry::with_builtin(&a.general.workspace_dir)?;
            tools.extend(mcp_tools)?;
            let timeout = std::time::Duration::from_secs(a.general.websearch_timeout_secs);
            if !tools.register_websearch(&a.general.websearch_command, timeout) {
                tracing::info!("未注册联网工具：{} 不可用", a.general.websearch_command);
            }
            if let Some(g) = &egress {
                check_egress(&config, &tools, &llm_paths)?;
                egress_checked = true;
                tools.set_egress_guard(g.clone());
            }
            tool_names = tools.names();
            capabilities = tools.capabilities();
            tool_listing = tools
                .infos()
                .iter()
                .map(|t| format!("  - {}：{}", t.name, t.description))
                .collect::<Vec<_>>()
                .join("\n");
            registry.register(Arc::new(
                GeneralAgent::new(
                    llm_for(LlmRole::General),
                    tools,
                    a.general.max_steps,
                    a.general.max_concurrent,
                )
                .with_report_max_searches(a.general.report_max_searches)
                .with_report_parallel(a.general.report_parallel),
            ));
        }
        if egress.is_some() && !egress_checked {
            // General 未启用时没有工具，只校验 LLM 路径
            check_egress(&config, &ToolRegistry::default(), &llm_paths)?;
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
            let coding = Arc::new(coding);
            probers.push(Prober::Cli {
                agent: coding.clone(),
                backend: a.coding.cfg.backend.clone(),
            });
            registry.register(coding);
        }
        let (acp_probers, dynamic) = register_acp_agents(&registry, &config, &managed);
        probers.extend(acp_probers);
        agent_config.set_dynamic(dynamic);
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
        let router = Arc::new(
            Router::new(
                llm_for(LlmRole::Router),
                registry.clone(),
                tool_names,
                Policy {
                    approval_risk: o.approval_risk,
                },
            )
            .with_risk_rules(config.conversation.risk_rules)
            .with_class_questions(config.conversation.route_class_questions)
            .with_class_from_tools(config.conversation.route_class_from_tools),
        );
        // WP4：内置模板；容量与后台槽位对齐（至少 4，覆盖 quote/release 三路）
        let templates = Arc::new(
            builtin_templates::builtin_registry(4)
                .map_err(|errs| anyhow::anyhow!("内置模板加载失败：{}", errs.join("；")))?,
        );
        let orchestrator = Arc::new(Orchestrator::new(
            store.clone(),
            bus.clone(),
            registry.clone(),
            reviewer,
            Composer::new(llm_for(LlmRole::Conversation)),
            o.max_review_rounds,
            Duration::from_secs(o.task_timeout_secs),
            templates,
        ));
        if let Some(l) = &admission {
            orchestrator.set_admission(l.clone());
        }
        // WP7：重启后处理仍未起跑的任务。开关开：单任务（非模板家族）重新排队执行（方案 C）；
        // 开关关：全部走 T-81 失败恢复。两种情况下，未重排者都写原因事件与失败交付。
        // 必须在 orchestrator 构建之后（重驱走 spawn）、对外 serve 前完成。
        let resumed_ids = if o.resume_queued_on_restart {
            match orchestrator.resume_queued_on_restart() {
                Ok(ids) if !ids.is_empty() => {
                    tracing::info!(
                        event = "task.resume_queued",
                        count = ids.len(),
                        "{} 个未起跑任务已重新排队执行",
                        ids.len()
                    );
                    ids
                }
                Ok(ids) => ids,
                Err(e) => {
                    tracing::error!(error = %e, "重启重排未起跑任务失败");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        match store.fail_unstarted_tasks(&resumed_ids) {
            Ok(unstarted) if !unstarted.is_empty() => {
                tracing::warn!(
                    event = "task.interrupted",
                    count = unstarted.len(),
                    "{} 个未起跑任务重启后标记为 FAILED",
                    unstarted.len()
                );
                for task in &unstarted {
                    Self::write_restart_failure(&store, task);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "重启失败恢复（未起跑任务）出错"),
        }
        let probers = Arc::new(ProberSet::new(probers));
        let agents = Arc::new(agent_chat::AgentAdmin {
            config: config.clone(),
            registry: registry.clone(),
            probers: probers.clone(),
            manager: agent_config.clone(),
            store: store.clone(),
            bus: bus.clone(),
        });
        let conversation = Arc::new(ConversationRuntime::new(conversation::ConversationDeps {
            // 对话里管理助手（AG.4）：开关打开时才把向导交给前台
            agents: config.conversation.agent_management.then(|| agents.clone()),
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
            session_materials: config.conversation.session_materials,
            session_materials_max_tokens: config.conversation.session_materials_max_tokens,
            session_materials_timeout_ms: config.conversation.session_materials_timeout_ms,
            session_materials_style_max_tokens: config
                .conversation
                .session_materials_style_max_tokens,
            session_materials_findings_max: config.conversation.session_materials_findings_max,
            session_materials_judge: config.conversation.session_materials_judge,
            session_materials_route: config.conversation.session_materials_route,
            session_materials_exec: config.conversation.session_materials_exec,
            session_materials_exec_max_tokens: config
                .conversation
                .session_materials_exec_max_tokens,
        }));
        tracing::info!(
            event = "conversation.config",
            review_requests = config.conversation.review_requests.as_str(),
            stream_replies = config.conversation.stream_replies
        );

        Ok(Self {
            capabilities,
            config,
            store,
            bus,
            registry,
            router,
            orchestrator,
            conversation,
            nodes,
            diagnostics,
            llm: llm_for(LlmRole::Conversation),
            backends: backends.clone(),
            probers,
            admission,
            probe_gate: Arc::default(),
            agent_config,
            agents,
            egress,
            diagnose_send: Arc::default(),
        })
    }

    /// 把已激活的外部 agent 注册进 registry（准入 HTTP 接口激活后调用；已注册时不重复）
    pub fn register_a2a(&self, agent_id: &str) -> Result<()> {
        let ledger = self
            .admission
            .as_ref()
            .context("准入未开启（[admission] enabled = false）")?;
        let rec = ledger
            .get(agent_id)
            .with_context(|| format!("未知 agent：{agent_id}"))?;
        anyhow::ensure!(rec.stage == admission::Stage::Active, "agent 尚未激活");
        let agent = a2a_agent(
            &rec,
            self.config.orchestrator.task_timeout_secs,
            self.egress.clone(),
        )?
        .context("该 agent 没有登记接入信息")?;
        self.registry.register(agent);
        Ok(())
    }

    /// 启动后台任务：节点心跳、局域网发现。由 main 在 tokio 运行时中调用
    /// 立即对所有 ACP / CLI agent 做一次连通性探测（测试与手动刷新用）
    pub async fn refresh_links(&self) {
        probe_agents(&self.registry, &self.backends, &self.probers.snapshot()).await;
    }

    pub fn start_background(&self) {
        self.backends.spawn_probes();
        // 连通性探测：启动时一次，之后每 PROBE_INTERVAL 一次；只做握手或命令检查，不调用模型
        let (registry, backends, probers) = (
            self.registry.clone(),
            self.backends.clone(),
            self.probers.clone(),
        );
        tokio::spawn(async move {
            loop {
                probe_agents(&registry, &backends, &probers.snapshot()).await;
                tokio::time::sleep(link::PROBE_INTERVAL).await;
            }
        });
        self.nodes
            .start_heartbeat(Duration::from_secs(self.config.nodes.refresh_secs.max(3)));
        if self.config.nodes.discovery {
            self.nodes.start_discovery();
        }
        // 业务内省：默认关闭；打开后按 schedule_hours 调用 run_once（本期无 Engine 挂载时用空来源）
        if self.config.introspection.enabled {
            introspect::operator::log_operator_role_status(&self.config.introspection);
            let store = self.store.clone();
            let bus = self.bus.clone();
            let backends = self.backends.clone();
            let cfg = self.config.introspection.clone();
            tokio::spawn(async move {
                let interval = Duration::from_secs(cfg.schedule_hours.max(1) * 3600);
                let mut ticker = tokio::time::interval(interval);
                loop {
                    ticker.tick().await;
                    let src = introspect::MemorySource {
                        source_id: "mom".into(),
                        cases: vec![],
                    };
                    let opts = introspect::RunOnceOptions::from_config(&cfg);
                    let wm = store
                        .introspection_latest_watermark()
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "{}".into());
                    let explainer = if cfg.model_explain {
                        introspect::slot_explainer_from_backends(&backends)
                    } else {
                        None
                    };
                    if let Err(e) = introspect::run_once(introspect::RunOnceRequest {
                        store: &store,
                        source: &src,
                        cfg: &cfg,
                        trigger: "schedule",
                        watermark_json: &wm,
                        input_hash: "schedule",
                        analysis_now: 0,
                        opts,
                        bus: Some(bus.as_ref()),
                        explainer,
                    })
                    .await
                    {
                        tracing::warn!(
                            event = "introspection.job_failed",
                            error = %e,
                            "定时内省运行失败"
                        );
                    }
                }
            });
        }
    }
}

/// 注册 ACP agent，顺序：config.toml 的 [[acp.agents]] → 托管的 → 本机已安装的预设（同 id 先到先得）。
/// 未安装的预设不注册，避免界面里出现一堆不可用的 agent；自定义的即使未安装也注册，方便看到原因。
/// 返回注册成功的 ACP agent 的探测目标，以及由"托管 / 自动发现"注册的部分（AG.2 热更新时对比用）。
fn register_acp_agents(
    registry: &AgentRegistry,
    config: &AppConfig,
    managed: &agent_config::ManagedConfig,
) -> (
    Vec<Prober>,
    std::collections::BTreeMap<String, acp::AcpConfig>,
) {
    let mut probers = Vec::new();
    for cfg in &config.acp.agents {
        let agent = Arc::new(AcpAgent::new(
            cfg.clone(),
            agent_config::default_workdir(config, cfg.role),
        ));
        if !agent.is_installed() {
            tracing::warn!(event = "agent.not_installed", code = an_core::codes::AGENT_NOT_INSTALLED, agent_id = %cfg.id, "ACP agent 不可用：{}", agent_reason(&agent));
        }
        if registry.register(agent.clone()) {
            probers.push(Prober::Acp(agent));
        }
    }
    let mut dynamic = std::collections::BTreeMap::new();
    let managed_ids: Vec<&str> = managed.acp.agents.iter().map(|a| a.id.as_str()).collect();
    for cfg in agent_config::dynamic_desired(config, managed) {
        let agent = Arc::new(AcpAgent::new(
            agent_config::resolve_env(&cfg),
            agent_config::default_workdir(config, cfg.role),
        ));
        if registry.register(agent.clone()) {
            if managed_ids.contains(&cfg.id.as_str()) {
                tracing::info!(event = "agent.registered", agent_id = %cfg.id, source = "managed", "注册托管 ACP agent");
            } else {
                tracing::info!(event = "agent.detected", agent_id = %cfg.id, role = cfg.role.as_str(), "自动发现 ACP agent");
            }
            probers.push(Prober::Acp(agent));
            dynamic.insert(cfg.id.clone(), cfg);
        }
    }
    (probers, dynamic)
}

fn agent_reason(agent: &AcpAgent) -> String {
    an_core::AgentAdapter::card(agent)
        .disabled_reason
        .clone()
        .unwrap_or_default()
}

/// 由准入记录中的接入信息构造外部 agent 适配器；没有接入信息时返回 None
fn a2a_agent(
    rec: &admission::AdmissionRecord,
    timeout_secs: u64,
    egress: Option<Arc<an_agents::egress::EgressGuard>>,
) -> Result<Option<Arc<an_agents::a2a::A2aHttpAgent>>> {
    let Some(b) = &rec.binding else {
        return Ok(None);
    };
    let role = match b.role.as_str() {
        "coding" => Role::Coding,
        "reviewer" => Role::Reviewer,
        _ => Role::General,
    };
    let name = rec
        .claim
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| rec.agent_id.clone());
    let mut agent = an_agents::a2a::A2aHttpAgent::new(an_agents::a2a::A2aSpec {
        id: rec.agent_id.clone(),
        name,
        role,
        agent_type: b.agent_type.clone(),
        endpoint: b.endpoint.clone(),
        max_concurrent: b.max_concurrent,
        timeout: Duration::from_secs(timeout_secs.max(1)),
    })?;
    // #360 防线二：外部 agent 出站前过外发检查（与 tools/LLM 同一守卫；off 时为 None）
    if let Some(g) = egress {
        agent.set_egress_guard(g);
    }
    Ok(Some(Arc::new(agent)))
}

/// 外发守卫的启动校验：绑定的工具 / 后端必须存在；enforce 下所有外发路径必须登记，否则拒绝启动
fn check_egress(
    config: &AppConfig,
    tools: &ToolRegistry,
    llm_paths: &[(String, String)],
) -> Result<()> {
    use an_agents::{capability::Effect, egress::PathKind, mcp::McpTransport};
    let http_servers: Vec<&str> = config
        .mcp
        .servers
        .iter()
        .filter(|s| matches!(s.transport, McpTransport::Http { .. }))
        .map(|s| s.id.as_str())
        .collect();
    let paths: Vec<(String, PathKind)> = tools
        .capabilities()
        .into_iter()
        .map(|c| {
            let kind = if matches!(c.tool_id.as_str(), "web_search" | "web_fetch") {
                PathKind::Tool
            } else if http_servers.contains(&c.provider.as_str()) {
                PathKind::HttpMcpTool
            } else if matches!(c.effect, Effect::ExternalWrite) {
                PathKind::ExternalWriteTool
            } else {
                PathKind::Local
            };
            (c.tool_id, kind)
        })
        .collect();
    for w in an_agents::egress::validate_bindings(&config.egress, &paths, llm_paths)? {
        tracing::warn!(event = "config.egress", code = an_core::codes::EGRESS_UNBOUND, warning = %w, "外发路径未登记（audit 模式只提示，enforce 模式会拒绝启动）");
    }
    Ok(())
}
