//! RT-ASEL-02：每轮「带了哪些记忆、为什么」事件记录（conversation-memory #346 片4b，REQ-conversation-memory-06）。
//! 用例定义见 docs/testing/CASES.md「分层记忆细化」。
//! 独立测试二进制以获得专属 tracing 订阅器（set_global_default 每进程一次）。

use std::path::Path;
use std::sync::{Arc, Mutex};

use an_core::TaskRecord;
use an_core::{
    PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskResult, TaskStatus, WorkerStatus,
};
use an_runtime::config::AppConfig;
use an_runtime::introspect::model::IntrospectionFinding;
use an_runtime::store::{DesktopLinkOutcome, Store};
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 假模型：turn_decision → respond；其余文本请求返回固定回答（SSE）
struct MockLlm {
    base: String,
}

impl MockLlm {
    async fn start() -> Self {
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
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
        }
    }
}

fn runtime(base: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("asel"));
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
"#,
        dir.join("files").display(),
    );
    let (config, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "未知配置键：{unknown:?}");
    Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap()
}

fn seed_completed_linked_task(store: &Store, conv_id: &str, task_id: &str, summary: &str) {
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
        DesktopLinkOutcome::Created(_) | DesktopLinkOutcome::Replay(_) => {}
        other => panic!("意外的链接结果：{other:?}"),
    }
}

fn seed_gated_finding(rt: &Runtime, id: &str, hypothesis: &str) {
    let json = json!({
        "template": "方案采纳模式",
        "hypothesis": hypothesis,
        "next_step": "复核原始记录",
        "case_ids": ["case-a2"],
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&IntrospectionFinding {
            id: id.into(),
            job_id: format!("job-{id}"),
            topic: "方案采纳模式".into(),
            json,
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
}

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

/// RT-ASEL-02（片 4b，REQ-06）：每轮「带了哪些记忆、为什么」——session.materials
/// 事件记录 occasion（程序按状态判断：历史空 = 新会话，否则 = 会话续问）与
/// why（各段入选原因码），可还原本轮带了什么、为什么带；仍不含资料正文。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_asel_02_event_records_occasion_and_why() {
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
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_completed_linked_task(&rt.store, &conv.id, "task-asel-02", "产线摘要 ASEL02。");
    seed_gated_finding(
        &rt,
        "intf-asel-02",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    rt.store
        .create_desktop_memory(
            "local",
            "偏好摘要格式",
            "记忆正文标记 ASEL02BODY。",
            "2099-01-01",
            "fact",
        )
        .unwrap();
    for i in 0..12 {
        rt.conversation
            .handle_message(&conv.id, &format!("你好，填充消息{i}"))
            .await
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，3 号产线 CPK 那个任务的结论是什么？")
        .await
        .unwrap();

    let captured = lines.lock().unwrap().clone();
    let events: Vec<&String> = captured
        .iter()
        .filter(|l| l.contains("session.materials") && l.contains(&conv.id))
        .collect();
    assert!(
        events.len() >= 13,
        "每轮都应记录事件（含填充轮）：共 {} 条",
        events.len()
    );
    // 填充轮：新会话首轮 occasion=新会话，之后为会话续问
    assert!(
        events[0].contains("occasion=新会话"),
        "首轮应为新会话：{}",
        events[0]
    );
    assert!(
        events
            .iter()
            .filter(|l| !l.contains("occasion=新会话"))
            .count()
            >= 12,
        "后续轮应为会话续问"
    );
    // 本轮带了回执 + 记忆 + 相关发现，why 逐段可还原
    let last = events.last().unwrap();
    assert!(last.contains("occasion=会话续问"), "{last}");
    assert!(last.contains("receipts:in_window"), "回执段原因码：{last}");
    assert!(
        last.contains("memories:effective_fact"),
        "记忆段原因码：{last}"
    );
    assert!(
        last.contains("findings:related_gate"),
        "发现段原因码（相关性门达标）：{last}"
    );
    assert!(
        !last.contains("style:effective_style"),
        "未注入的段不得出现在 why：{last}"
    );
    assert!(!last.contains("ASEL02BODY"), "事件不得含资料正文：{last}");
    assert!(last.contains("intf-asel-02"), "来源 id 可追溯：{last}");
}
