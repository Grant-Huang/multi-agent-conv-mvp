//! General 写长报告：输出上限、历史里不回放全文、续写用 append。

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use an_core::{AgentAdapter, EventPublisher, EventSink, TaskEvent, TaskSpec, WorkerStatus};
use an_llm::LlmClient;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};

struct Noop;
impl EventPublisher for Noop {
    fn publish(&self, _: TaskEvent) {}
}

#[tokio::test]
async fn rt_rep_04_long_report_appends_instead_of_rewriting_and_requests_8192_tokens() {
    let head = format!("HEAD_MARKER {}", "甲".repeat(120));
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let bodies = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let saved = bodies.clone();
    let head_for_step = head.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(req): Json<Value>| {
            let seen = seen.clone();
            let saved = saved.clone();
            let head_for_step = head_for_step.clone();
            async move {
                saved.lock().await.push(req.clone());
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let opening = format!("# 报告\n{head_for_step}");
                let step = if n == 0 {
                    json!({"thought":"先写开头","action":"tool_call","tool":"write_file","args":{"path":"reports/long.md","content":opening,"mode":"overwrite"},"answer":""})
                } else if n == 1 {
                    json!({"thought":"续写","action":"tool_call","tool":"write_file","args":{"path":"reports/long.md","content":"\nTAIL_MARKER","mode":"append"},"answer":""})
                } else {
                    json!({"thought":"完成","action":"final","tool":"","args":{},"answer":"已写入 reports/long.md"})
                };
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"delta":{"content":step.to_string()},"finish_reason":"stop"}]})
                    ),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = std::env::temp_dir().join(format!("an-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let llm = LlmClient::new(
        serde_json::from_value(json!({"base_url":base,"model":"mock","max_tokens":2048})).unwrap(),
    )
    .unwrap();
    let tools = an_agents::tools::ToolRegistry::with_builtin(&dir).unwrap();
    let agent = an_agents::GeneralAgent::new(llm, tools, 6, 1);
    let task: TaskSpec = serde_json::from_value(json!({"task_id":"report","conversation_id":"c",
        "goal":"写一份完整 Markdown 报告", "context":"", "route":{"task_class":"L4","execution_role":"general",
            "required_tools":["write_file"],"risk_level":"low","reason":"test"}}))
    .unwrap();
    let result = agent
        .run(task, EventSink::new(Arc::new(Noop), "c", "report"))
        .await
        .unwrap();
    assert_eq!(result.status, WorkerStatus::Succeeded);
    let disk = std::fs::read_to_string(dir.join("reports/long.md")).unwrap();
    assert!(disk.starts_with("# 报告\nHEAD_MARKER "));
    assert!(disk.ends_with("TAIL_MARKER"));
    assert_eq!(disk.matches("HEAD_MARKER").count(), 1);

    let bodies = bodies.lock().await;
    assert_eq!(bodies.len(), 3);
    assert_eq!(bodies[0]["max_tokens"], 8192);
    let prompt = bodies[0]["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("mode=append"));
    let second = bodies[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(second.contains("mode=append"));
    assert!(second.contains("content_chars"));
    assert!(
        !second.contains("正文已省略"),
        "历史里不能留下可被抄进正文的占位文字"
    );
    assert!(!second.contains("HEAD_MARKER 甲甲"), "历史不应回放整篇正文");
    assert!(second.contains("末尾："));
    let _ = std::fs::remove_dir_all(&dir);
    server.abort();
}
