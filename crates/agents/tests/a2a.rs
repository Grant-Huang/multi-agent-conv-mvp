//! RT-ADM-15：外部 agent 的 HTTP 适配器（用例见 docs/testing/CASES.md）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_agents::a2a::{A2aHttpAgent, A2aSpec};
use an_core::{AgentAdapter, Risk, Role, RouteDecision, TaskClass, TaskSpec, WorkerStatus};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn spec(endpoint: &str, timeout_ms: u64) -> A2aSpec {
    A2aSpec {
        id: "ext".into(),
        name: "外部质量 agent".into(),
        role: Role::General,
        agent_type: "ext".into(),
        endpoint: endpoint.into(),
        max_concurrent: 2,
        timeout: Duration::from_millis(timeout_ms),
    }
}

fn task() -> TaskSpec {
    TaskSpec {
        task_id: "task-1".into(),
        conversation_id: "c1".into(),
        goal: "分析 B77 批次 CPK 下降".into(),
        context: "Line-B".into(),
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: Role::General,
            required_tools: vec![],
            risk_level: Risk::Medium,
            reason: String::new(),
        },
    }
}

struct Noop;
impl an_core::EventPublisher for Noop {
    fn publish(&self, _: an_core::TaskEvent) {}
}

fn sink() -> an_core::EventSink {
    an_core::EventSink::new(Arc::new(Noop), "c1", "task-1")
}

#[tokio::test]
async fn rt_adm_15_a2a_http_adapter() {
    // 成功：请求字段齐全，结果映射为 Succeeded，并标注未经核实
    let seen: Arc<Mutex<Option<Value>>> = Arc::default();
    let s = seen.clone();
    let ok = serve(Router::new().route(
        "/run",
        post(move |Json(req): Json<Value>| {
            *s.lock().unwrap() = Some(req);
            async { Json(json!({"status": "succeeded", "summary": "根因可能是主轴振动"})) }
        }),
    ))
    .await;
    let agent = A2aHttpAgent::new(spec(&ok, 2_000)).unwrap();
    assert_eq!(agent.card().kind, "a2a");
    assert_eq!(agent.card().max_concurrent, 2);
    let r = agent.run(task(), sink()).await.unwrap();
    assert_eq!(r.status, WorkerStatus::Succeeded);
    assert_eq!(r.summary, "根因可能是主轴振动");
    assert!(r.evidence.iter().any(|e| e.content.contains("未经核实")));
    let req = seen.lock().unwrap().clone().unwrap();
    assert_eq!(req["task_id"], "task-1");
    assert_eq!(req["goal"], "分析 B77 批次 CPK 下降");
    assert_eq!(req["context"], "Line-B");
    assert_eq!(req["risk"], "medium");

    // 外部 agent 自己报告失败
    let failed = serve(Router::new().route(
        "/run",
        post(|| async {
            Json(json!({"status": "failed", "summary": "", "errors": ["缺数据"]}))
        }),
    ))
    .await;
    let r = A2aHttpAgent::new(spec(&failed, 2_000))
        .unwrap()
        .run(task(), sink())
        .await
        .unwrap();
    assert_eq!(r.status, WorkerStatus::Failed);
    assert!(r.errors.iter().any(|e| e.contains("缺数据")));

    // HTTP 错误、无效 JSON、超时：返回带错误码的失败
    let bad = serve(Router::new().route(
        "/run",
        post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
    ))
    .await;
    let e = A2aHttpAgent::new(spec(&bad, 2_000))
        .unwrap()
        .run(task(), sink())
        .await
        .unwrap_err();
    assert!(
        format!("{e:#}").contains(an_core::codes::AGENT_RUNTIME),
        "{e:#}"
    );
    let junk = serve(Router::new().route("/run", post(|| async { "not json" }))).await;
    let e = A2aHttpAgent::new(spec(&junk, 2_000))
        .unwrap()
        .run(task(), sink())
        .await
        .unwrap_err();
    assert!(
        format!("{e:#}").contains(an_core::codes::AGENT_RUNTIME),
        "{e:#}"
    );
    let slow = serve(Router::new().route(
        "/run",
        post(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Json(json!({"status": "succeeded", "summary": "太慢"}))
        }),
    ))
    .await;
    let e = A2aHttpAgent::new(spec(&slow, 200))
        .unwrap()
        .run(task(), sink())
        .await
        .unwrap_err();
    assert!(
        format!("{e:#}").contains(an_core::codes::AGENT_TIMEOUT),
        "{e:#}"
    );

    // 非 http(s) 地址被拒
    assert!(A2aHttpAgent::new(spec("ftp://x", 1_000)).is_err());
}
