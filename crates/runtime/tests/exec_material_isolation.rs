//! #360（R-20261126-M6）：exec 资料块与任务上下文分离后的分流回归。
//!
//! 资料块独立持久化在 tasks.exec_material；执行时只注入本机执行者（kind != "a2a"），
//! 外部 A2A 的请求体（goal/context）不下发——个人记忆不越过外发边界。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::{
    AgentAdapter, AgentCard, EventSink, PolicyVerdict, Risk, Role, RouteDecision, TaskClass,
    TaskResult, TaskSpec, TaskStatus, WorkerStatus,
};
use an_llm::{LlmClient, LlmConfig};
use an_runtime::bus::Bus;
use an_runtime::composer::Composer;
use an_runtime::orchestrator::Orchestrator;
use an_runtime::registry::AgentRegistry;
use an_runtime::store::Store;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

/// 记录收到的 TaskSpec 的本机假 agent（代表内置 General）
struct Spy {
    card: AgentCard,
    seen: Arc<Mutex<Option<TaskSpec>>>,
}

#[async_trait::async_trait]
impl AgentAdapter for Spy {
    fn card(&self) -> &AgentCard {
        &self.card
    }
    async fn run(&self, spec: TaskSpec, _: EventSink) -> anyhow::Result<TaskResult> {
        *self.seen.lock().unwrap() = Some(spec);
        Ok(TaskResult {
            status: WorkerStatus::Succeeded,
            summary: "本机做完了".into(),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        })
    }
}

fn spy(id: &str, seen: Arc<Mutex<Option<TaskSpec>>>) -> Arc<Spy> {
    Arc::new(Spy {
        card: AgentCard {
            id: id.into(),
            kind: "builtin".into(),
            agent_type: "general".into(),
            location: "本机".into(),
            max_concurrent: 1,
            role: Role::General,
            name: id.into(),
            description: String::new(),
            tools: vec![],
            enabled: true,
            disabled_reason: None,
        },
        seen,
    })
}

/// 记录请求体的假外部 agent 服务
async fn serve_capture(seen: Arc<Mutex<Vec<Value>>>) -> String {
    let s = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/run",
        post(move |Json(req): Json<Value>| {
            s.lock().unwrap().push(req);
            async { Json(json!({"status": "succeeded", "summary": "外部做完"})) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn env(
    assign: BTreeMap<String, Vec<String>>,
    agents: Vec<Arc<dyn AgentAdapter>>,
) -> (Arc<Store>, Arc<Orchestrator>) {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let bus = Arc::new(Bus::new(store.clone()));
    let registry = AgentRegistry::new(assign);
    for a in agents {
        registry.register(a);
    }
    // 端口 1 不可连接：交付消息退回模板，不依赖真实模型
    let llm = LlmClient::new(LlmConfig {
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "none".into(),
        max_retries: 0,
        ..Default::default()
    })
    .unwrap();
    let orch = Arc::new(Orchestrator::new(
        store.clone(),
        bus,
        registry,
        None,
        Composer::new(llm),
        0,
        Duration::from_secs(10),
        Arc::new(an_runtime::template::TemplateRegistry::default()),
    ));
    (store, orch)
}

fn route() -> RouteDecision {
    RouteDecision {
        task_class: TaskClass::L1,
        execution_role: Role::General,
        required_tools: vec![],
        risk_level: Risk::Low,
        reason: String::new(),
        template_id: String::new(),
        deliverable: String::new(),
        slots_ok: String::new(),
    }
}

fn auto() -> PolicyVerdict {
    PolicyVerdict {
        auto_execute: true,
        needs_review: false,
        notes: vec![],
        candidates: vec![],
    }
}

fn material() -> &'static str {
    "个人记忆「门禁码」：MATERIAL-360-PLACEHOLDER（#360 测试假数据）"
}

fn wait_terminal(store: &Store, id: &str) -> TaskStatus {
    for _ in 0..150 {
        if let Ok(Some(t)) = store.get_task(id) {
            if matches!(
                t.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            ) {
                return t.status;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("任务 150 轮内未到终态：{id}");
}

/// 外部 A2A general agent 委派：请求体 goal/context 不含 exec 资料块；资料块仍持久化在库
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_execmat_01_a2a_dispatch_withholds_material() {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let endpoint = serve_capture(seen.clone()).await;
    let ext = Arc::new(
        an_agents::a2a::A2aHttpAgent::new(an_agents::a2a::A2aSpec {
            id: "ext".into(),
            name: "外部 general".into(),
            role: Role::General,
            agent_type: "ext".into(),
            endpoint,
            max_concurrent: 1,
            timeout: Duration::from_secs(3),
        })
        .unwrap(),
    );
    let mut assign = BTreeMap::new();
    assign.insert("general".into(), vec!["ext".into()]);
    let (store, orch) = env(assign, vec![ext]);

    let task = orch
        .submit(
            "c1",
            "整理产线数据",
            "近期摘录",
            route(),
            auto(),
            Some(material()),
        )
        .unwrap();
    assert_eq!(wait_terminal(&store, &task.id), TaskStatus::Completed);

    let reqs = seen.lock().unwrap();
    assert_eq!(reqs.len(), 1, "外部 agent 应恰好收到一次请求");
    let req = &reqs[0];
    assert!(
        !req["context"].as_str().unwrap().contains("MATERIAL-360"),
        "外部请求 context 不得含 exec 资料块：{}",
        req["context"]
    );
    assert!(
        !req["goal"].as_str().unwrap().contains("MATERIAL-360"),
        "外部请求 goal 不得含 exec 资料块"
    );
    assert_eq!(req["context"], "近期摘录", "context 本体照常下发");
    // 资料块仍持久化：本机审计可查
    assert_eq!(
        store.get_task_exec_material(&task.id).unwrap().as_deref(),
        Some(material())
    );
}

/// 本机内置 general 执行：spec.context 含资料块（既有行为不变，拼接格式与 #360 前一致）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_execmat_02_internal_executor_receives_material() {
    let seen: Arc<Mutex<Option<TaskSpec>>> = Arc::default();
    let mut assign = BTreeMap::new();
    assign.insert("general".into(), vec!["local-gen".into()]);
    let (store, orch) = env(assign, vec![spy("local-gen", seen.clone())]);

    let task = orch
        .submit(
            "c1",
            "整理产线数据",
            "近期摘录",
            route(),
            auto(),
            Some(material()),
        )
        .unwrap();
    assert_eq!(wait_terminal(&store, &task.id), TaskStatus::Completed);

    let spec = seen.lock().unwrap().clone().expect("本机执行者应收到 spec");
    assert_eq!(
        spec.context,
        format!("近期摘录\n\n{}", material()),
        "本机执行者 context = 上下文 + 资料块（与 #360 之前格式一致）"
    );
}
