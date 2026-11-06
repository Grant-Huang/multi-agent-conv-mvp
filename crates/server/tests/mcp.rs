use an_runtime::{config::AppConfig, store::Store, Runtime};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

#[tokio::test]
async fn runtime_discovers_mcp_and_exposes_sanitized_cards() {
    let dir = std::env::temp_dir().join(an_core::new_id("mcp-api"));
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../agents/tests/fixtures/mcp_server.py");
    let text = format!(
        r#"
[llm]
base_url = "http://127.0.0.1:1/v1"
model = "mock"
[agents.general]
workspace_dir = {:?}
[agents.coding]
enabled = false
[nodes]
discovery = false
[[mcp.servers]]
id = "fixture"
transport = "stdio"
command = "python3"
args = [{:?}]
[[mcp.servers.tools]]
name = "echo"
capability_id = "test.echo"
description = "Echo a test message"
effect = "read_only"
"#,
        dir.to_string_lossy(),
        fixture.to_string_lossy()
    );
    let (cfg, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(Runtime::new(cfg.clone(), Store::open(Path::new(":memory:")).unwrap()).is_err());
    let rt = Runtime::with_mcp_diagnostics(
        cfg,
        Store::open(Path::new(":memory:")).unwrap(),
        an_observe::diagnostics_only(),
    )
    .await
    .unwrap();
    assert!(rt.capabilities.iter().any(|c| c.tool_id == "fixture.echo"));
    assert!(rt
        .registry
        .cards()
        .iter()
        .any(|c| c.tools.iter().any(|t| t.name == "fixture.echo")));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/capabilities", listener.local_addr().unwrap());
    let app = an_server::api(Arc::new(rt));
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let cards: Value = reqwest::get(url).await.unwrap().json().await.unwrap();
    assert!(cards
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["tool_id"] == "fixture.echo"));
    assert!(!cards.to_string().contains("mcp_server.py"));
    assert_eq!(
        cards
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["tool_id"] == "fixture.echo")
            .unwrap()["input_schema"]["required"],
        json!(["text"])
    );
    handle.abort();
    let _ = handle.await;
    std::fs::remove_dir_all(dir).unwrap();
}
