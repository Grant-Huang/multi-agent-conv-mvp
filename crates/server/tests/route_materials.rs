//! RT-RMAT：路由路径会话资料线索（conversation-memory S5-route，REQ-conversation-memory-01 续接）。
//! 用例定义见 docs/testing/CASES.md「路由路径会话资料线索」。
//! 口径（2026-11-24 沿用 S4 拍板）：极简清单并入路由 ctx / 独立开关默认开 / 相关性门只作用于发现。

use std::path::Path;
use std::sync::{Arc, Mutex};

use an_core::TaskRecord;
use an_core::{
    PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskResult, TaskStatus, WorkerStatus,
};
use an_runtime::config::AppConfig;
use an_runtime::store::{DesktopLinkOutcome, Store};
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 假模型：turn_decision → delegate；route_decision → 通用路由；文本 → 固定回答；记录请求体
struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start() -> Self {
        let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = requests.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let log = log.clone();
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    log.lock().unwrap().push(req);
                    let content = match schema.as_str() {
                        "turn_decision" => json!({"action":"delegate","question":"","task_goal":"整理产线报告","task_id":"","follow_up_kind":"","change":""}).to_string(),
                        "route_decision" => json!({"task_class":"L3","execution_role":"general","required_tools":["write_file"],"risk_level":"medium","reason":"受控验证路由","template_id":"","deliverable":"","slots_ok":"no"}).to_string(),
                        _ => "你好，验证成功。".to_string(),
                    };
                    let chunk =
                        json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]});
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                    )
                        .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            requests,
        }
    }

    /// route_decision schema 的请求（路由路径）
    fn route_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"] == json!("route_decision"))
            .cloned()
            .collect()
    }
}

fn runtime(base: &str, conv_extra: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("rmat"));
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(
        r#"
[llm]
base_url = "{base}/v1"
model = "scripted"
max_retries = 0
timeout_secs = 5
[backends.local]
kind = "openai"
base_url = "{base}/v1"
[routing]
conversation = "local"
router = "local"
general = "local"
reviewer = "local"
[agents.general]
workspace_dir = "{}"
[agents.coding]
enabled = false
[conversation]
{conv_extra}
"#,
        dir.join("files").display(),
    );
    let (config, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "未知配置键：{unknown:?}");
    Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap()
}

fn seed_linked_task(rt: &Runtime, conv_id: &str, task_id: &str, summary: &str) {
    let t = TaskRecord {
        id: task_id.into(),
        conversation_id: conv_id.into(),
        goal: "验证任务".into(),
        role: Role::General,
        status: TaskStatus::Completed,
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: Role::General,
            required_tools: vec![],
            risk_level: Risk::Low,
            reason: String::new(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        },
        result: Some(TaskResult {
            status: WorkerStatus::Succeeded,
            summary: summary.into(),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        }),
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: an_core::now(),
        updated_at: an_core::now(),
    };
    rt.store.insert_task(&t, "", None).unwrap();
    match rt
        .store
        .create_desktop_task_link(task_id, conv_id, &format!("op-{task_id}"), &[], "tester")
        .unwrap()
    {
        DesktopLinkOutcome::Created(_) | DesktopLinkOutcome::Replay(_) => {}
        other => panic!("意外的链接结果：{other:?}"),
    }
}

/// RT-RMAT-01：回执出窗 + 相关委派续问 → 路由请求 user 含「会话资料线索」段与任务 id
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_rmat_01_route_context_carries_materials_digest() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-rmat-01",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理 3 号产线那个任务的结论成报告")
        .await
        .unwrap();
    let req = llm.route_calls().last().unwrap().clone();
    let user = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .expect("路由请求应有 user 消息")
        .to_string();
    assert!(
        user.contains("会话资料线索"),
        "路由上下文应含线索段：{user}"
    );
    assert!(
        user.contains("task-rmat-01"),
        "路由上下文应含任务 id：{user}"
    );
}

/// RT-RMAT-02：独立开关关闭后路由上下文不含线索段
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_rmat_02_route_switch_off_keeps_context_clean() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "session_materials_route = false");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-rmat-02", "报废工单集中在 3 号产线。");
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理 3 号产线那个任务的结论成报告")
        .await
        .unwrap();
    let req = llm.route_calls().last().unwrap().clone();
    let user = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        !user.contains("会话资料线索"),
        "开关关闭后路由上下文不得含线索段：{user}"
    );
}

/// RT-RMAT-03：无关发现不进路由上下文（相关性门只作用于发现）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_rmat_03_unrelated_finding_not_in_route_context() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let j = json!({
        "template": "方案采纳模式",
        "hypothesis": "报废工单集中在 3 号产线，方案仍被重复采纳。",
        "case_ids": ["case-r3"],
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&an_runtime::introspect::model::IntrospectionFinding {
            id: "intf-rmat-03".into(),
            job_id: "job-rmat-03".into(),
            topic: "方案采纳模式".into(),
            json: j,
            verification: "supported".into(),
            gate_score: Some(0.8),
            gate_reason: "测试直过".into(),
            pushed: false,
            verdict: None,
            verdict_note: None,
            created_at: ts.clone(),
            updated_at: ts,
        })
        .unwrap();
    rt.conversation
        .handle_message(&conv.id, "你好，帮我把仓库整理成周报")
        .await
        .unwrap();
    let req = llm.route_calls().last().unwrap().clone();
    let user = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        !user.contains("intf-rmat-03"),
        "无关发现不得进路由上下文：{user}"
    );
}

/// RT-RMAT-05（#360）：判断线索段只服务路由模型——路由请求含线索段的同时，
/// 下发的任务上下文（tasks.context，会随外部 A2A 请求出站）不含线索段与资料块。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_rmat_05_digest_stripped_from_task_context() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-rmat-05",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理 3 号产线那个任务的结论成报告")
        .await
        .unwrap();
    // 路由请求仍含线索段（S5-route 行为不变）
    let req = llm.route_calls().last().unwrap().clone();
    let user = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .expect("路由请求应有 user 消息")
        .to_string();
    assert!(
        user.contains("会话资料线索"),
        "路由上下文应含线索段：{user}"
    );
    // 任务上下文不含线索段（#360：不随任务/外部请求出站）
    let tasks = rt.store.list_tasks(Some(&conv.id)).unwrap();
    assert!(!tasks.is_empty(), "委派应产生任务");
    for t in tasks {
        let ctx = rt
            .store
            .get_task_context(&t.id)
            .unwrap()
            .unwrap_or_default();
        assert!(
            !ctx.contains("会话资料线索"),
            "任务上下文不得携带判断线索段（task {}）：{ctx}",
            t.id
        );
        assert!(
            !ctx.contains(an_runtime::session_materials::MATERIALS_MARKER),
            "任务上下文不得携带回答资料块（task {}）：{ctx}",
            t.id
        );
    }
}
