//! R-FIRST-05：补充要求路由失败时保留已有任务。
use an_core::TaskStatus;
use an_runtime::{config::AppConfig, store::Store, Runtime};
use axum::{response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

async fn scenario(mode: &'static str) {
    let old_id = Arc::new(Mutex::new(String::new()));
    let route_calls = Arc::new(AtomicUsize::new(0));
    let id_for_model = old_id.clone();
    let calls = route_calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(req): Json<Value>| {
        let id = id_for_model.clone();
        let calls = calls.clone();
        async move {
            let schema = req["response_format"]["json_schema"]["name"].as_str().unwrap_or("");
            let content = if schema == "turn_decision" {
                let id = id.lock().unwrap().clone();
                if id.is_empty() {
                    json!({"action":"delegate","task_goal":"写原始报告","question":"","task_id":"","follow_up_kind":"","change":""}).to_string()
                } else {
                    json!({"action":"follow_up","task_goal":"","question":"","task_id":id,"follow_up_kind":"supplement","change":"增加附录"}).to_string()
                }
            } else {
                assert_eq!(schema, "route_decision");
                let replacement = calls.fetch_add(1, Ordering::SeqCst) > 0;
                if replacement && mode == "transport" {
                    return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "unavailable").into_response();
                }
                if replacement && mode == "format" {
                    "this is not a route decision".to_string()
                } else {
                    json!({"task_class":"L3","execution_role":if replacement && mode == "policy" {"coding"} else {"general"},"required_tools":[],"risk_level":"high","reason":"回归测试"}).to_string()
                }
            };
            if req["stream"] == true {
                ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],format!("data: {}\n\ndata: [DONE]\n\n",json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]}))).into_response()
            } else {
                Json(json!({"choices":[{"message":{"content":content}}]})).into_response()
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = std::env::temp_dir().join(an_core::new_id("supplement-routing"));
    let cfg: AppConfig = toml::from_str(&format!(
        r#"
[llm]
base_url = "http://{addr}/v1"
model = "scripted"
max_retries = 0
timeout_secs = 5
[agents.general]
workspace_dir = "{}"
[agents.coding]
command = "no-such-agent-for-supplement-regression"
[acp]
auto_detect = false
[nodes]
enabled = false
"#,
        dir.display()
    ))
    .unwrap();
    let rt = Runtime::new(cfg, Store::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let conv = rt.store.create_conversation("回归").unwrap();
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        rt.conversation.handle_message(&conv.id, "写原始报告"),
    )
    .await
    .unwrap()
    .unwrap();
    let old = first.task.unwrap();
    assert_eq!(old.status, TaskStatus::NeedsHuman);
    *old_id.lock().unwrap() = old.id.clone();
    let before = serde_json::to_value(rt.store.get_task(&old.id).unwrap().unwrap()).unwrap();
    let events = serde_json::to_value(rt.store.list_events(&old.id).unwrap()).unwrap();
    let follow = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        rt.conversation.handle_message(&conv.id, "增加附录"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(route_calls.load(Ordering::SeqCst), 2, "补充要求只路由一次");
    if mode == "success" {
        let replacement = follow.task.unwrap();
        assert_ne!(replacement.id, old.id);
        assert_eq!(replacement.status, TaskStatus::NeedsHuman);
        assert_eq!(
            rt.store.get_task(&old.id).unwrap().unwrap().status,
            TaskStatus::Cancelled
        );
        assert!(replacement.goal.contains("写原始报告"));
        assert!(replacement.goal.contains("增加附录"));
        assert_eq!(rt.store.list_tasks(Some(&conv.id)).unwrap().len(), 2);
    } else {
        assert!(follow.task.is_none());
        assert_eq!(
            serde_json::to_value(rt.store.get_task(&old.id).unwrap().unwrap()).unwrap(),
            before,
            "路由失败不能取消旧任务：{mode}"
        );
        assert_eq!(
            serde_json::to_value(rt.store.list_events(&old.id).unwrap()).unwrap(),
            events
        );
        assert_eq!(rt.store.list_tasks(Some(&conv.id)).unwrap().len(), 1);
    }
    drop(rt);
    server.abort();
    let _ = server.await;
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn rt_conv_01_failed_supplement_preserves_existing_task() {
    for mode in ["transport", "format", "policy"] {
        scenario(mode).await;
    }
}

#[tokio::test]
async fn rt_conv_02_successful_supplement_replaces_task_after_routing() {
    scenario("success").await;
}
