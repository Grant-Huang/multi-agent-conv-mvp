//! 阶段⑤a-1：经 stdio 挂载的模拟工厂（`an-des-mcp`）与进程内服务的一致性。
//!
//! 用例见 `docs/testing/CASES.md`「模拟工厂与 MCP 挂载（阶段⑤a-1）」。每个等待都有超时，
//! 服务未实现或卡住时测试失败而不是挂起。

use an_des::{tool_catalog, FactoryService, ServiceConfig, ToolCategory};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, PaginatedRequestParams, Tool},
    service::RunningService,
    transport::async_rw::AsyncRwTransport,
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::{Child, Command};

const START: u64 = 497;
const ADMIN: &str = "mcp-test-admin";
const WAIT: Duration = Duration::from_secs(20);

struct Server {
    client: RunningService<RoleClient, ()>,
    child: Child,
}

async fn start() -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_an-des-mcp"))
        .env("AN_DES_ADMIN_TOKEN", ADMIN)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn an-des-mcp");
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let client = tokio::time::timeout(WAIT, ().serve(AsyncRwTransport::new(stdout, stdin)))
        .await
        .expect("MCP handshake timed out")
        .expect("MCP handshake must succeed");
    Server { client, child }
}

async fn list(server: &Server) -> Vec<Tool> {
    let mut out = vec![];
    let mut cursor = None;
    loop {
        let page = tokio::time::timeout(
            WAIT,
            server
                .client
                .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor))),
        )
        .await
        .expect("list_tools timed out")
        .expect("list_tools");
        out.extend(page.tools);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return out;
        }
    }
}

async fn raw_call(server: &Server, tool: &str, args: Value) -> CallToolResult {
    tokio::time::timeout(
        WAIT,
        server.client.peer().call_tool(
            CallToolRequestParams::new(tool.to_string())
                .with_arguments(args.as_object().expect("object args").clone()),
        ),
    )
    .await
    .expect("call_tool timed out")
    .expect("call_tool transport")
}

/// 成功调用的结构化结果。
async fn call(server: &Server, tool: &str, args: Value) -> Value {
    let r = raw_call(server, tool, args.clone()).await;
    assert_ne!(r.is_error, Some(true), "{tool} {args} failed: {r:?}");
    r.structured_content
        .unwrap_or_else(|| panic!("{tool} must return structuredContent"))
}

/// 同一组调用分别经进程内服务和 MCP 执行。
fn script() -> Vec<(&'static str, Value)> {
    let sim = "DES-1";
    let write = |extra: Value, key: &str| {
        let mut a = json!({"simulation_id": sim, "idempotency_key": key, "approval_ref": "CASE-s1-1:rev1:production"});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    };
    vec![
        (
            "simulation_create",
            json!({"name": "p0", "factory": null, "scenario": {"seed": 1, "pattern": {"kind": "script"}, "start_minute": START, "horizon_minutes": 240, "mechanisms": null}}),
        ),
        (
            "simulation_advance",
            json!({"simulation_id": sim, "to_minute": START + 16}),
        ),
        (
            "events_poll",
            json!({"simulation_id": sim, "cursor": 0, "limit": 100}),
        ),
        ("equipment_status", json!({"simulation_id": sim})),
        (
            "equipment_trend",
            json!({"simulation_id": sim, "asset": "CNC-XX"}),
        ),
        ("quality_capability", json!({"simulation_id": sim})),
        ("inventory_tooling", json!({"simulation_id": sim})),
        (
            "maintenance_order_create",
            write(
                json!({"machine": "CNC-XX", "start_minute": START + 16}),
                "m1",
            ),
        ),
        (
            "maintenance_order_create",
            write(
                json!({"machine": "CNC-XX", "start_minute": START + 16}),
                "m1",
            ),
        ),
        (
            "tooling_transfer_create",
            write(json!({"machine": "CNC-XZ"}), "t1"),
        ),
        (
            "simulation_advance",
            json!({"simulation_id": sim, "to_minute": START + 300}),
        ),
        (
            "events_poll",
            json!({"simulation_id": sim, "cursor": 0, "limit": 100}),
        ),
        ("equipment_status", json!({"simulation_id": sim})),
        (
            "simulation_ground_truth",
            json!({"simulation_id": sim, "admin_token": ADMIN}),
        ),
    ]
}

#[tokio::test]
async fn rt_desm_01_mcp_lists_the_factory_catalog() {
    let server = start().await;
    let tools = list(&server).await;
    let catalog = tool_catalog();
    assert_eq!(tools.len(), catalog.len(), "same number of tools");
    for spec in &catalog {
        let t = tools
            .iter()
            .find(|t| t.name == spec.name)
            .unwrap_or_else(|| panic!("{} missing over MCP", spec.name));
        assert_eq!(
            serde_json::to_value(&*t.input_schema).unwrap(),
            spec.input_schema,
            "{} schema",
            spec.name
        );
        assert_eq!(
            t.description.as_deref(),
            Some(spec.description.as_str()),
            "{} description",
            spec.name
        );
        let ann = t
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("{} needs annotations", spec.name));
        match spec.category {
            ToolCategory::Observe => assert_eq!(ann.read_only_hint, Some(true), "{}", spec.name),
            ToolCategory::Execute => {
                assert_eq!(ann.read_only_hint, Some(false), "{}", spec.name);
                assert_eq!(
                    ann.idempotent_hint,
                    Some(true),
                    "{} uses idempotency keys",
                    spec.name
                );
            }
            ToolCategory::Control | ToolCategory::Admin => {
                assert_eq!(
                    ann.read_only_hint,
                    Some(spec.category == ToolCategory::Admin),
                    "{}",
                    spec.name
                )
            }
        }
    }
}

#[tokio::test]
async fn rt_desm_02_mcp_results_match_the_in_process_service() {
    let server = start().await;
    let mut local = FactoryService::new(ServiceConfig {
        admin_token: Some(ADMIN.into()),
    });
    for (tool, args) in script() {
        let expected = local
            .call(tool, args.clone())
            .unwrap_or_else(|e| panic!("in-process {tool} failed: {e}"));
        let remote = call(&server, tool, args.clone()).await;
        assert_eq!(
            serde_json::to_string(&remote).unwrap(),
            serde_json::to_string(&expected).unwrap(),
            "{tool} {args}: MCP and in-process results must be byte-identical"
        );
    }
}

#[tokio::test]
async fn rt_desm_03_errors_are_tool_errors_and_stdin_close_exits() {
    let server = start().await;
    call(&server, "simulation_create", script()[0].1.clone()).await;
    for (tool, args) in [
        ("no_such_tool", json!({})),
        ("equipment_status", json!({"simulation_id": "DES-404"})),
        (
            "simulation_ground_truth",
            json!({"simulation_id": "DES-1", "admin_token": "wrong"}),
        ),
        (
            "maintenance_order_create",
            json!({"simulation_id": "DES-1", "machine": "CNC-XX", "start_minute": START, "idempotency_key": "k"}),
        ),
    ] {
        let r = tokio::time::timeout(
            WAIT,
            server.client.peer().call_tool(
                CallToolRequestParams::new(tool.to_string())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .expect("error call timed out");
        // 未知工具可以是协议错误；其余必须是工具错误（isError），不能让服务退出。
        if let Ok(result) = r {
            assert_eq!(result.is_error, Some(true), "{tool} must fail: {result:?}");
            assert!(
                result.structured_content.is_none(),
                "{tool}: no data on error"
            );
        } else {
            assert_eq!(
                tool, "no_such_tool",
                "{tool} must be a tool error, not a protocol error"
            );
        }
    }
    let alive = call(
        &server,
        "equipment_status",
        json!({"simulation_id": "DES-1"}),
    )
    .await;
    assert_eq!(
        alive["provenance"], "simulation",
        "the service still answers"
    );

    // 关闭连接（stdin）后进程按时退出。
    let Server { client, mut child } = server;
    let _ = tokio::time::timeout(WAIT, client.cancel()).await;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("an-des-mcp must exit after stdin closes")
        .expect("wait");
    assert!(status.success(), "clean exit: {status:?}");
}
