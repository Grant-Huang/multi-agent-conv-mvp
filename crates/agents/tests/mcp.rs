use an_agents::{
    mcp::{connect, McpServerConfig},
    ToolRegistry,
};
use serde_json::json;

fn config(names: &[&str]) -> McpServerConfig {
    serde_json::from_value(json!({
        "id":"fixture", "transport":"stdio", "command":"python3",
        "args":[format!("{}/tests/fixtures/mcp_server.py", env!("CARGO_MANIFEST_DIR"))],
        "timeout_ms":1000,
        "tools":names.iter().map(|name| json!({"name":name,"capability_id":format!("test.{name}"),
            "description":format!("Test {name}"),"effect":"read_only"})).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[tokio::test]
async fn discovers_paginated_allowed_tools_and_reuses_session() {
    let tools = connect(&[config(&["echo", "resource"])]).await.unwrap();
    assert_eq!(tools.names(), ["fixture.echo", "fixture.resource"]);
    let card = tools
        .capabilities()
        .into_iter()
        .find(|c| c.tool_id == "fixture.echo")
        .unwrap();
    assert_eq!(card.capability_id, "test.echo");
    assert_eq!(card.input_schema["required"], json!(["text"]));
    for _ in 0..2 {
        let out = tools
            .call("fixture.echo", &json!({"text":"hello"}))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(value["status"], "success");
        assert_eq!(value["data"]["content"][0]["text"], "hello");
    }
    let out = tools
        .call("fixture.resource", &json!({"text":"x"}))
        .await
        .unwrap();
    assert_eq!(out.artifacts[0].content, "# Report");
    assert!(tools
        .call("fixture.echo", &json!({"text":123}))
        .await
        .unwrap_err()
        .to_string()
        .contains("E_MCP_ARGUMENTS"));
    assert!(tools
        .call("fixture.fail", &json!({"text":"x"}))
        .await
        .is_err());
}

#[tokio::test]
async fn tool_errors_and_invalid_outputs_are_not_success() {
    let tools = connect(&[config(&["fail", "bad_output"])]).await.unwrap();
    for (tool, code) in [("fail", "E_MCP_TOOL"), ("bad_output", "E_MCP_OUTPUT")] {
        assert!(tools
            .call(&format!("fixture.{tool}"), &json!({"text":"x"}))
            .await
            .unwrap_err()
            .to_string()
            .contains(code));
    }
}

#[tokio::test]
async fn timeout_discards_session_without_retry() {
    let mut cfg = config(&["slow", "echo"]);
    cfg.timeout_ms = 150;
    let tools = connect(&[cfg]).await.unwrap();
    let started = std::time::Instant::now();
    assert!(tools
        .call("fixture.slow", &json!({"text":"x"}))
        .await
        .unwrap_err()
        .to_string()
        .contains("E_MCP_TIMEOUT"));
    assert!(started.elapsed().as_secs() < 2);
    assert!(tools
        .call("fixture.echo", &json!({"text":"x"}))
        .await
        .unwrap_err()
        .to_string()
        .contains("E_MCP_UNAVAILABLE"));
}

#[tokio::test]
async fn rejects_ambiguous_duplicate_missing_and_unauthorized_bindings() {
    let mut cfg = config(&["echo"]);
    cfg.id = "bad.id".into();
    assert!(connect(&[cfg]).await.is_err());
    assert!(connect(&[config(&["missing"])]).await.is_err());
    assert!(connect(&[config(&["echo", "echo"])]).await.is_err());
    assert!(connect(&[config(&["echo"]), config(&["echo"])])
        .await
        .is_err());
    let mut cfg = config(&["echo"]);
    cfg.tools[0].effect = an_agents::capability::Effect::ExternalWrite;
    let tools = connect(&[cfg]).await.unwrap();
    assert!(tools
        .call("fixture.echo", &json!({"text":"x"}))
        .await
        .unwrap_err()
        .to_string()
        .contains("E_MCP_DENIED"));
}

#[tokio::test]
async fn builtin_tools_keep_working_through_gateway() {
    let dir = std::env::temp_dir().join(format!("an-mcp-{}", std::process::id()));
    let tools = ToolRegistry::with_builtin(&dir).unwrap();
    let output = tools
        .call("calculator", &json!({"expression":"2+3"}))
        .await
        .unwrap();
    assert!(output.text.contains('5'));
    assert_eq!(tools.capabilities().len(), 5);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn invalid_arguments_never_reach_server_and_cancel_drops_child() {
    let tools = connect(&[config(&["echo", "slow"])]).await.unwrap();
    assert!(tools
        .call("fixture.echo", &json!({"text":123}))
        .await
        .is_err());
    let out = tools
        .call("fixture.echo", &json!({"text":"x"}))
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&out.text).unwrap();
    assert_eq!(result["data"]["structuredContent"]["calls"], 1);
    let pid = result["data"]["structuredContent"]["pid"].as_u64().unwrap();
    let worker = tools.clone();
    let task = tokio::spawn(async move { worker.call("fixture.slow", &json!({"text":"x"})).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(tools
        .call("fixture.echo", &json!({"text":"x"}))
        .await
        .is_err());
    #[cfg(target_os = "linux")]
    {
        let path = format!("/proc/{pid}");
        for _ in 0..100 {
            if !std::path::Path::new(&path).exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("MCP child was not reaped");
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}

#[tokio::test]
async fn streamable_http_handshake_and_schema_validation() {
    use axum::{response::IntoResponse, routing::post, Json, Router};
    use serde_json::Value;
    async fn rpc(Json(req): Json<Value>) -> axum::response::Response {
        let result = match req["method"].as_str().unwrap() {
            "initialize" => json!({"protocolVersion":req["params"]["protocolVersion"],
                "capabilities":{"tools":{}},"serverInfo":{"name":"http-fixture","version":"1"}}),
            "tools/list" => json!({"tools":[{"name":"echo","inputSchema":{
                "type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}),
            "tools/call" => {
                json!({"content":[{"type":"text","text":req["params"]["arguments"]["text"]}]})
            }
            _ => return axum::http::StatusCode::ACCEPTED.into_response(),
        };
        Json(json!({"jsonrpc":"2.0","id":req["id"],"result":result})).into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/mcp", post(rpc)))
            .await
            .unwrap()
    });
    let mut cfg = config(&["echo"]);
    cfg.transport = an_agents::mcp::McpTransport::Http {
        url,
        bearer_env: None,
    };
    let tools = connect(&[cfg]).await.unwrap();
    let out = tools
        .call("fixture.echo", &json!({"text":"HTTP works"}))
        .await
        .unwrap();
    assert!(out.text.contains("HTTP works"));
    drop(tools);
    server.abort();
}

#[tokio::test]
async fn general_selects_mcp_and_uses_observation_without_external_agent() {
    use an_core::{AgentAdapter, EventPublisher, EventSink, TaskEvent, TaskSpec, WorkerStatus};
    use axum::{routing::post, Json, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Noop;
    impl EventPublisher for Noop {
        fn publish(&self, _: TaskEvent) {}
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(req): Json<serde_json::Value>| {
        let seen = seen.clone();
        async move {
            assert!(req["response_format"]["json_schema"]["schema"]["properties"]["tool"]["enum"]
                .as_array().unwrap().contains(&json!("fixture.echo")));
            let step = if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                json!({"thought":"Use discovered tool","action":"tool_call","tool":"fixture.echo","args":{"text":"MCP evidence"},"answer":""})
            } else {
                assert!(req["messages"].as_array().unwrap().iter().any(|m| m["content"].as_str().unwrap_or_default().contains("MCP evidence")));
                json!({"thought":"Done","action":"final","tool":"","args":{},"answer":"Used MCP"})
            };
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":step.to_string()},"finish_reason":"stop"}]})))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let llm = an_llm::LlmClient::new(
        serde_json::from_value(json!({"base_url":base,"model":"mock"})).unwrap(),
    )
    .unwrap();
    let tools = connect(&[config(&["echo"])]).await.unwrap();
    let agent = an_agents::GeneralAgent::new(llm, tools, 3, 1);
    let task: TaskSpec = serde_json::from_value(json!({"task_id":"mcp-task","conversation_id":"c",
        "goal":"Echo evidence", "context":"", "route":{"task_class":"L2","execution_role":"general",
            "required_tools":["fixture.echo"],"risk_level":"low","reason":"test"}}))
    .unwrap();
    let result = agent
        .run(task, EventSink::new(Arc::new(Noop), "c", "mcp-task"))
        .await
        .unwrap();
    assert_eq!(result.status, WorkerStatus::Succeeded);
    assert_eq!(result.summary, "Used MCP");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(result
        .evidence
        .iter()
        .any(|e| e.content.contains("fixture.echo") && e.content.contains("MCP evidence")));
    server.abort();
}

#[tokio::test]
async fn http_rejects_unsafe_urls_and_never_follows_redirects() {
    use axum::{response::Redirect, routing::post, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    for url in [
        "http://example.invalid/mcp",
        "https://user:secret@example.invalid/mcp",
        "https://example.invalid/mcp?token=secret",
    ] {
        let mut cfg = config(&["echo"]);
        cfg.transport = an_agents::mcp::McpTransport::Http {
            url: url.into(),
            bearer_env: None,
        };
        assert!(connect(&[cfg]).await.is_err());
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = Router::new()
        .route("/mcp", post(|| async { Redirect::temporary("/target") }))
        .route(
            "/target",
            post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    "unexpected"
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut cfg = config(&["echo"]);
    cfg.transport = an_agents::mcp::McpTransport::Http {
        url,
        bearer_env: None,
    };
    assert!(connect(&[cfg]).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn expired_http_session_does_not_replay_tool_calls() {
    use axum::{response::IntoResponse, routing::post, Json, Router};
    use serde_json::Value;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let initializations = Arc::new(AtomicUsize::new(0));
    let (seen, inits) = (calls.clone(), initializations.clone());
    let app = Router::new().route(
        "/mcp",
        post(move |Json(req): Json<Value>| {
            let (seen, inits) = (seen.clone(), inits.clone());
            async move {
                let result = match req["method"].as_str().unwrap() {
                    "initialize" => {
                        inits.fetch_add(1, Ordering::SeqCst);
                        json!({
                    "protocolVersion":req["params"]["protocolVersion"],"capabilities":{"tools":{}},
                    "serverInfo":{"name":"expiry","version":"1"}})
                    }
                    "tools/list" => {
                        json!({"tools":[{"name":"echo","inputSchema":{"type":"object"}}]})
                    }
                    "tools/call" => {
                        seen.fetch_add(1, Ordering::SeqCst);
                        return axum::http::StatusCode::NOT_FOUND.into_response();
                    }
                    _ => return axum::http::StatusCode::ACCEPTED.into_response(),
                };
                (
                    [("mcp-session-id", "fixture-session")],
                    Json(json!({"jsonrpc":"2.0","id":req["id"],"result":result})),
                )
                    .into_response()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut cfg = config(&["echo"]);
    cfg.transport = an_agents::mcp::McpTransport::Http {
        url,
        bearer_env: None,
    };
    let tools = connect(&[cfg]).await.unwrap();
    assert!(tools
        .call("fixture.echo", &json!({"text":"x"}))
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(initializations.load(Ordering::SeqCst), 1);
    server.abort();
}
