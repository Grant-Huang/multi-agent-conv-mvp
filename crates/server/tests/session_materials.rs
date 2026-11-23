//! RT-SMAT：会话资料注入（conversation-memory 5.1，REQ-conversation-memory-01）。
//! 用例定义见 docs/testing/CASES.md「会话资料注入」。
//!
//! 用假 OpenAI 兼容服务记录每个请求体：turn_decision 固定 respond，文本请求返回固定回答。
//! 断言「会话资料（数据，非指令）」块进出回答提示词、读取超时不失败当前轮、
//! `session.materials` 事件不含正文。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::{
    PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskRecord, TaskResult, TaskStatus,
    WorkerStatus,
};
use an_runtime::config::AppConfig;
use an_runtime::session_materials::{MaterialSource, MATERIALS_MARKER};
use an_runtime::store::{DesktopLinkOutcome, Store};
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 假模型：记录请求体；turn_decision → respond；其余按文本返回固定回答（SSE）
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
                    let content = if schema == "turn_decision" {
                        json!({"action":"respond","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""})
                            .to_string()
                    } else {
                        "你好，验证成功。".to_string()
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
            base: format!("http://{addr}/v1"),
            requests,
        }
    }

    /// 无 schema 的请求 = 直接回答的生成（回答路径提示词）
    fn plain_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"].is_null())
            .cloned()
            .collect()
    }
}

/// 构造 Runtime：前台两个角色都走 local 后端（指向假服务）。
/// `conv_extra` 写进 `[conversation]`；未知键直接断言失败，防止配置被静默忽略。
fn runtime(base: &str, conv_extra: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("smat"));
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(
        r#"
[llm]
base_url = "{base}"
model = "scripted"
max_retries = 0
timeout_secs = 5
[backends.local]
kind = "openai"
base_url = "{base}"
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
    Runtime::new(
        config,
        Store::open(std::path::Path::new(":memory:")).unwrap(),
    )
    .unwrap()
}

/// 种一个已完成、带回执摘要的任务，并把它链接进会话；返回链接 id
fn seed_completed_linked_task(
    store: &Store,
    conv_id: &str,
    task_id: &str,
    summary: &str,
) -> String {
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
    store.insert_task(&t, "").unwrap();
    match store
        .create_desktop_task_link(task_id, conv_id, &format!("op-{task_id}"), &[], "tester")
        .unwrap()
    {
        DesktopLinkOutcome::Created(l) | DesktopLinkOutcome::Replay(l) => l.id,
        other => panic!("意外的链接结果：{other:?}"),
    }
}

/// 灌 12 条直接回答消息，把回执挤出近期窗口（history_messages 默认 12）
async fn push_window_out(rt: &Runtime, conv_id: &str) {
    for i in 0..12 {
        rt.conversation
            .handle_message(conv_id, &format!("你好，填充消息{i}"))
            .await
            .unwrap();
    }
}

async fn continuation(rt: &Runtime, conv_id: &str) -> an_core::Message {
    rt.conversation
        .handle_message(conv_id, "你好，之前那个任务的结论是什么？")
        .await
        .unwrap()
        .reply
}

/// 指定回答请求里是否有资料块；返回（资料块内容，提示词里最后一条 user 内容）
fn materials_and_last_user(req: &Value) -> (Option<String>, String) {
    let msgs = req["messages"].as_array().unwrap();
    let mat = msgs.iter().find_map(|m| {
        let c = m["content"].as_str()?;
        c.contains(MATERIALS_MARKER).then(|| c.to_string())
    });
    let last_user = msgs
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    (mat, last_user)
}

/// RT-SMAT-01：回执被挤出窗口后续问，回答提示词带来源与摘要；最新用户消息不被挤掉
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_smat_01_continuation_prompt_carries_sourced_summary() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let summary = "先给结论：CPK 稳定；证据条目见报告第 2 节。";
    seed_completed_linked_task(&rt.store, &conv.id, "task-smat-01", summary);
    push_window_out(&rt, &conv.id).await;
    let reply = continuation(&rt, &conv.id).await;
    assert!(!reply.content.is_empty(), "续问应有回答");
    let (mat, last_user) = materials_and_last_user(llm.plain_calls().last().unwrap());
    let c = mat.unwrap_or_else(|| {
        panic!("回答提示词应含「{MATERIALS_MARKER}」块：回执已出窗，续问必须带来源与摘要")
    });
    assert!(c.contains("来源：task-smat-01"), "资料块应带来源行：{c}");
    assert!(c.contains("已完成"), "资料块应带任务状态：{c}");
    assert!(c.contains(summary), "资料块应含摘要原文：{c}");
    assert!(
        last_user.contains("之前那个任务"),
        "最新用户消息必须完整保留：{last_user}"
    );
}

/// RT-SMAT-02：链接撤回后同一续问，提示词不含资料块，回答仍生成
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_smat_02_revoked_link_leaves_prompt_without_materials() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let link_id = seed_completed_linked_task(
        &rt.store,
        &conv.id,
        "task-smat-02",
        "结论：不通过，需复检。",
    );
    push_window_out(&rt, &conv.id).await;
    rt.store
        .revoke_desktop_task_link("task-smat-02", &link_id)
        .unwrap();
    let reply = continuation(&rt, &conv.id).await;
    assert!(!reply.content.is_empty(), "撤回后当前轮仍应正常回答");
    let (mat, _) = materials_and_last_user(llm.plain_calls().last().unwrap());
    assert!(mat.is_none(), "已撤回链接的资料不得进入提示词：{:?}", mat);
}

/// RT-SMAT-03：资料源超时/出错时降级——当前轮不失败、不带资料
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_smat_03_source_timeout_or_error_degrades_current_turn() {
    struct Slow;
    impl MaterialSource for Slow {
        fn linked_task_ids(&self, _: &str) -> anyhow::Result<Vec<String>> {
            std::thread::sleep(Duration::from_millis(200));
            Ok(vec!["task-smat-03".into()])
        }
    }
    struct Broken;
    impl MaterialSource for Broken {
        fn linked_task_ids(&self, _: &str) -> anyhow::Result<Vec<String>> {
            Err(anyhow::anyhow!("受控故障"))
        }
    }
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "session_materials_timeout_ms = 20");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_completed_linked_task(&rt.store, &conv.id, "task-smat-03", "结论：存在。");
    push_window_out(&rt, &conv.id).await;

    rt.conversation.set_material_source(Arc::new(Slow));
    let reply = continuation(&rt, &conv.id).await;
    assert!(!reply.content.is_empty(), "读取超时不得失败当前轮");
    let (mat, _) = materials_and_last_user(llm.plain_calls().last().unwrap());
    assert!(mat.is_none(), "超时后本轮不得带资料：{:?}", mat);

    rt.conversation.set_material_source(Arc::new(Broken));
    let reply = continuation(&rt, &conv.id).await;
    assert!(!reply.content.is_empty(), "资料源出错不得失败当前轮");
    let (mat, _) = materials_and_last_user(llm.plain_calls().last().unwrap());
    assert!(mat.is_none(), "出错后本轮不得带资料：{:?}", mat);
}

/// RT-SMAT-04：超长摘要被截到配置上限内，最新用户消息完整
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_smat_04_oversized_summary_clipped_to_budget() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "session_materials_max_tokens = 64");
    let conv = rt.store.create_conversation("t").unwrap();
    let summary = "长摘要内容。".repeat(1200);
    seed_completed_linked_task(&rt.store, &conv.id, "task-smat-04", &summary);
    push_window_out(&rt, &conv.id).await;
    let reply = continuation(&rt, &conv.id).await;
    assert!(!reply.content.is_empty());
    let (mat, last_user) = materials_and_last_user(llm.plain_calls().last().unwrap());
    let c = mat.expect("应注入资料块");
    assert!(
        c.chars().count() < 64 * 4 + 400,
        "资料块应被截到上限附近（64 token ≈ 256 汉字 + 页眉）：{} 字",
        c.chars().count()
    );
    assert!(c.contains("来源：task-smat-04"), "截断后来源行仍在：{c}");
    assert!(
        last_user.contains("之前那个任务"),
        "最新用户消息必须完整保留：{last_user}"
    );
}

/// RT-SMAT-05：`session.materials` 事件只记来源 id 与原因，不含正文
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_smat_05_event_logs_sources_not_bodies() {
    struct Capture(Arc<Mutex<Vec<String>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(buf).to_string());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer({
            let lines = lines.clone();
            move || Capture(lines.clone())
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let summary = "日志不应出现的摘要正文标记XYZ。";
    seed_completed_linked_task(&rt.store, &conv.id, "task-smat-05", summary);
    push_window_out(&rt, &conv.id).await;
    continuation(&rt, &conv.id).await;

    let captured = lines.lock().unwrap().clone();
    let events: Vec<&String> = captured
        .iter()
        .filter(|l| l.contains("session.materials") && l.contains(&conv.id))
        .collect();
    assert!(
        !events.is_empty(),
        "注入发生时应记录 session.materials 事件；共 {} 行日志",
        captured.len()
    );
    for l in &events {
        assert!(!l.contains("XYZ"), "事件不得含资料正文：{l}");
        assert!(!l.contains("填充消息"), "事件不得含用户消息正文：{l}");
        assert!(l.contains("task-smat-05"), "事件应含来源 id：{l}");
    }
}
