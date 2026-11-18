//! 外发守卫（必经化方案第一、二阶段）的集成测试：用 tests/fixtures/dlp_server.py 作 DLP 替身。
//!
//! 验收要点：
//! - enforce：有外发绑定的工具只收到脱敏后的参数；REVIEW / BLOCK / DLP 异常时真实工具不被调用
//! - audit：只记录，真实工具收到原参数
//! - off：不启动 DLP，行为与以前完全相同
//! - task_id、channel、destination_id、purpose 由宿主填写，不来自模型
//! - LLM 外发：发给云端的是脱敏文本，Token 映射用于本地恢复
use an_agents::{
    egress::{validate_bindings, EgressConfig, EgressGuard, EgressMode, PathKind},
    Tool, ToolOutput, ToolRegistry,
};
use an_llm::{egress::scope_task, ChatMessage, ErrorKind};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// 记录收到参数的假工具（代表真正的外发工具，如 web_search）
struct Recorder {
    name: String,
    seen: Arc<Mutex<Vec<Value>>>,
}
#[async_trait::async_trait]
impl Tool for Recorder {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "recorder"
    }
    fn args_hint(&self) -> &str {
        "{}"
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        self.seen.lock().unwrap().push(args.clone());
        Ok(ToolOutput {
            text: "ok".into(),
            artifacts: vec![],
        })
    }
}

fn fixture() -> String {
    format!(
        "{}/tests/fixtures/dlp_server.py",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn log_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{tag}.jsonl", an_core::new_id("dlp-log")))
}

fn calls(log: &Path) -> Vec<Value> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn config(mode: &str, log: &Path, fixture_mode: &str, timeout_ms: u64) -> EgressConfig {
    serde_json::from_value(json!({
        "mode": mode,
        "timeout_ms": timeout_ms,
        "dlp_command": "python3",
        "dlp_args": [fixture(), log.to_string_lossy(), fixture_mode],
        "tools": [{"tool":"web_search","destination_id":"public-search","channel":"external_api","purpose":"web_research"}],
        "llm": [{"backend":"cloud","destination_id":"cloud-llm","purpose":"llm_inference"}]
    }))
    .unwrap()
}

async fn registry(
    mode: &str,
    log: &Path,
    fixture_mode: &str,
    timeout_ms: u64,
) -> (
    ToolRegistry,
    Arc<Mutex<Vec<Value>>>,
    Option<Arc<EgressGuard>>,
) {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let mut tools = ToolRegistry::default();
    for name in ["web_search", "current_time"] {
        tools.register(Arc::new(Recorder {
            name: name.into(),
            seen: seen.clone(),
        }));
    }
    let guard = EgressGuard::start(&config(mode, log, fixture_mode, timeout_ms))
        .await
        .unwrap();
    if let Some(g) = &guard {
        tools.set_egress_guard(g.clone());
    }
    (tools, seen, guard)
}

#[tokio::test]
async fn off_mode_starts_nothing_and_changes_nothing() {
    let log = log_path("off");
    let (tools, seen, guard) = registry("off", &log, "normal", 2000).await;
    assert!(guard.is_none());
    tools
        .call("web_search", &json!({"query":"ACME SECRET-1"}))
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap()[0]["query"], "ACME SECRET-1");
    assert!(calls(&log).is_empty(), "off 模式不应调用 DLP");
}

#[tokio::test]
async fn enforce_sends_only_sanitized_args_with_host_context() {
    let log = log_path("enforce");
    let (tools, seen, _g) = registry("enforce", &log, "normal", 5000).await;
    scope_task("task-42".into(), async {
        tools
            .call("web_search", &json!({"query":"ACME 交期"}))
            .await
            .unwrap();
    })
    .await;
    assert_eq!(
        seen.lock().unwrap()[0],
        json!({"query":"[CUSTOMER_001] 交期"})
    );
    let c = &calls(&log)[0];
    assert_eq!(c["tool"], "egress_evaluate");
    assert_eq!(c["args"]["task_id"], "task-42");
    assert_eq!(c["args"]["channel"], "external_api");
    assert_eq!(c["args"]["destination_id"], "public-search");
    assert_eq!(c["args"]["purpose"], "web_research");
    assert_eq!(c["args"]["json_body"], json!({"query":"ACME 交期"}));
}

#[tokio::test]
async fn enforce_blocks_and_reviews_without_calling_the_tool() {
    let log = log_path("block");
    let (tools, seen, _g) = registry("enforce", &log, "normal", 5000).await;
    for (q, code) in [
        ("token SECRET-abc", "E_EGRESS_BLOCKED"),
        ("CONFIDENTIAL price", "E_EGRESS_REVIEW"),
    ] {
        let err = tools
            .call("web_search", &json!({"query":q}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(code), "{err}");
        assert!(!err.contains("SECRET-abc"), "错误信息不应带出原文：{err}");
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "被拦截时真实工具不应被调用"
    );
}

#[tokio::test]
async fn unbound_tools_are_not_checked() {
    let log = log_path("unbound");
    let (tools, seen, _g) = registry("enforce", &log, "normal", 5000).await;
    tools.call("current_time", &json!({})).await.unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(calls(&log).is_empty());
}

#[tokio::test]
async fn audit_records_but_never_blocks() {
    let log = log_path("audit");
    let (tools, seen, _g) = registry("audit", &log, "normal", 5000).await;
    tools
        .call("web_search", &json!({"query":"ACME SECRET-abc"}))
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap()[0]["query"], "ACME SECRET-abc");
    assert_eq!(calls(&log).len(), 1, "audit 模式也要调用 DLP 并记录");
}

#[tokio::test]
async fn enforce_fails_closed_on_invalid_output_and_timeout() {
    for (fixture_mode, timeout_ms) in [("bad_output", 5000), ("slow", 500)] {
        let log = log_path(fixture_mode);
        let started = EgressGuard::start(&config("enforce", &log, fixture_mode, timeout_ms)).await;
        // 输出不合规或超时：要么启动阶段就失败，要么调用时 fail closed；绝不能放行
        let g = match started {
            Err(_) => continue, // 启动失败 = 拒绝以不安全配置运行，可接受
            Ok(None) => panic!("{fixture_mode}: enforce 模式不能没有守卫"),
            Ok(Some(g)) => g,
        };
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let mut tools = ToolRegistry::default();
        tools.register(Arc::new(Recorder {
            name: "web_search".into(),
            seen: seen.clone(),
        }));
        tools.set_egress_guard(g);
        let err = tools
            .call("web_search", &json!({"query":"ACME"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("E_EGRESS_UNAVAILABLE"),
            "{fixture_mode}: {err}"
        );
        assert!(seen.lock().unwrap().is_empty(), "{fixture_mode}");
    }
}

#[tokio::test]
async fn dlp_process_exit_never_leaks_and_reconnects() {
    let log = log_path("crash");
    let (tools, seen, _g) = registry("enforce", &log, "crash_after_first", 5000).await;
    let mut ok = 0;
    for _ in 0..4 {
        if tools
            .call("web_search", &json!({"query":"ACME"}))
            .await
            .is_ok()
        {
            ok += 1;
        }
    }
    // 进程退出后的那次调用可能失败；之后应能重新拉起 DLP。无论如何，工具只见过脱敏参数
    assert!(ok >= 2, "应能在进程退出后重连，成功 {ok} 次");
    for args in seen.lock().unwrap().iter() {
        assert_eq!(args["query"], "[CUSTOMER_001]");
    }
}

#[tokio::test]
async fn llm_outbound_sanitizes_messages_and_returns_restore_map() {
    let log = log_path("llm");
    let (_tools, _seen, g) = registry("enforce", &log, "normal", 5000).await;
    let hook = g.unwrap().llm_hook("cloud").expect("cloud 后端有绑定");
    let msgs = vec![
        ChatMessage::system("你是助手"),
        ChatMessage::user("ACME 的交期是哪天？"),
    ];
    let out = scope_task("task-7".into(), hook.outbound(&msgs))
        .await
        .unwrap();
    assert_eq!(out.messages[1].content, "[CUSTOMER_001] 的交期是哪天？");
    assert_eq!(out.messages[0].role, "system");
    assert_eq!(
        out.restore.get("[CUSTOMER_001]").map(String::as_str),
        Some("ACME")
    );
    let c = &calls(&log)[0];
    assert_eq!(c["args"]["channel"], "external_api");
    assert_eq!(c["args"]["destination_id"], "cloud-llm");
    assert_eq!(c["args"]["task_id"], "task-7");
    let restore = calls(&log)
        .into_iter()
        .find(|c| c["tool"] == "egress_restore")
        .unwrap();
    assert_eq!(restore["args"]["context"], "local_ui");

    let blocked = hook
        .outbound(&[ChatMessage::user("SECRET-1")])
        .await
        .unwrap_err();
    assert_eq!(blocked.kind, ErrorKind::EgressBlocked);
}

#[tokio::test]
async fn llm_without_restore_still_sends_sanitized_text() {
    let log = log_path("norestore");
    let (_t, _s, g) = registry("enforce", &log, "no_restore", 5000).await;
    let hook = g.unwrap().llm_hook("cloud").unwrap();
    let out = hook.outbound(&[ChatMessage::user("ACME")]).await.unwrap();
    assert_eq!(out.messages[0].content, "[CUSTOMER_001]");
    assert!(out.restore.is_empty());
}

#[test]
fn bindings_are_validated_at_startup() {
    let log = log_path("validate");
    let cfg = config("enforce", &log, "normal", 2000);
    // 绑定的工具不存在：任何模式下都报错（防止拼写错误导致漏检）
    let err = validate_bindings(&cfg, &[], &[]).unwrap_err().to_string();
    assert!(err.contains("web_search"), "{err}");
    // enforce：联网工具、HTTP MCP 工具、云端 LLM 后端没有绑定 → 拒绝启动
    let paths = [
        ("web_search".to_string(), PathKind::Tool),
        ("web_fetch".to_string(), PathKind::Tool),
    ];
    let backends = [(
        "cloud".to_string(),
        "https://api.example.com/v1".to_string(),
    )];
    let err = validate_bindings(&cfg, &paths, &backends)
        .unwrap_err()
        .to_string();
    assert!(err.contains("web_fetch"), "{err}");
    let local = [("local".to_string(), "http://127.0.0.1:8098/v1".to_string())];
    let paths = [("web_search".to_string(), PathKind::Tool)];
    let warnings =
        validate_bindings(&cfg, &paths, &[backends[0].clone(), local[0].clone()]).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // 云端后端缺少绑定 → enforce 拒绝启动；audit 只给出警告
    let mut no_llm = cfg.clone();
    no_llm.llm.clear();
    assert!(validate_bindings(&no_llm, &paths, &backends).is_err());
    no_llm.mode = EgressMode::Audit;
    let warnings = validate_bindings(&no_llm, &paths, &backends).unwrap();
    assert!(warnings.iter().any(|w| w.contains("cloud")), "{warnings:?}");
}

/// 与真实 dlp-egress 服务联调（默认跳过）。本地运行：
/// AN_TEST_DLP_COMMAND=tools/dlp-egress/.venv/bin/dlp-egress-mcp AN_TEST_DLP_CONFIG=<dlp.yaml，需 allow_restore: true>
#[tokio::test]
async fn real_dlp_service_roundtrip_when_configured() {
    let (Ok(cmd), Ok(conf)) = (
        std::env::var("AN_TEST_DLP_COMMAND"),
        std::env::var("AN_TEST_DLP_CONFIG"),
    ) else {
        eprintln!("跳过：未设置 AN_TEST_DLP_COMMAND / AN_TEST_DLP_CONFIG");
        return;
    };
    let cfg: EgressConfig = serde_json::from_value(json!({
        "mode": "enforce", "timeout_ms": 10000, "dlp_command": cmd, "dlp_args": ["--config", conf],
        "tools": [{"tool":"web_search","destination_id":"vendor-api","channel":"external_api","purpose":"vendor_diagnostics"}],
        "llm": [{"backend":"cloud","destination_id":"cloud-llm","purpose":"llm_inference"}]
    }))
    .unwrap();
    let g = EgressGuard::start(&cfg).await.unwrap().unwrap();
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let mut tools = ToolRegistry::default();
    tools.register(Arc::new(Recorder {
        name: "web_search".into(),
        seen: seen.clone(),
    }));
    tools.set_egress_guard(g.clone());
    scope_task("real-1".into(), async {
        tools
            .call(
                "web_search",
                &json!({"query":"青川重工 WO-559120 良率97.5%"}),
            )
            .await
            .unwrap();
        let err = tools
            .call(
                "web_search",
                &json!({"query":"Authorization: Bearer DEMO-NOT-A-REAL-TOKEN-0001"}),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("E_EGRESS_BLOCKED"), "{err}");
    })
    .await;
    let sent = seen.lock().unwrap()[0]["query"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        !sent.contains("青川重工") && !sent.contains("WO-559120"),
        "{sent}"
    );
    assert!(sent.contains("良率97.5%"), "{sent}");
    let hook = g.llm_hook("cloud").unwrap();
    let out = scope_task(
        "real-2".into(),
        hook.outbound(&[ChatMessage::user("请总结青川重工的交付情况")]),
    )
    .await
    .unwrap();
    assert!(!out.messages[0].content.contains("青川重工"));
    assert!(
        out.restore.values().any(|v| v == "青川重工"),
        "{:?}",
        out.restore
    );
}
