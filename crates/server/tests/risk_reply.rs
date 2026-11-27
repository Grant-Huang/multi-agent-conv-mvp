//! RT-G2C（CH-G2c，Issue #402）：风险信息不可省略——回答后程序检查与补齐。
//! 风格"只要结论"生效 + 资料含"不可逆"风险项，模型返回不含风险的简短回答时，
//! 最终回答仍含一句风险提示（程序补齐），事件 conversation.risk_restored 记录。

use std::path::Path;
use std::sync::{Arc, Mutex};

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

/// 假模型：turn_decision → respond；其余（回答生成）→ 不含任何风险词的简短结论
struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start() -> Self {
        Self::start_with_reply("结论：3 号产线抽检合格。".into()).await
    }

    /// 回答生成（无 schema 的普通 chat）返回自定义文本
    async fn start_with_reply(reply: String) -> Self {
        let reply_text = Arc::new(reply);
        let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = requests.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let log = log.clone();
                let reply_text = Arc::clone(&reply_text);
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    log.lock().unwrap().push(req);
                    let content = match schema.as_str() {
                        "turn_decision" => json!({"action":"respond","question":""}).to_string(),
                        _ => (*reply_text).clone(),
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

    /// 回答生成请求（无 schema 的普通 chat）：断言风格卡护栏句仍在提示词里
    fn reply_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"].is_null())
            .cloned()
            .collect()
    }
}

fn runtime(base: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("g2c"));
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
"#,
        dir.join("files").display(),
    );
    let (config, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "未知配置键：{unknown:?}");
    Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap()
}

fn seed_linked_task(rt: &Runtime, conv_id: &str, task_id: &str, summary: &str) {
    let t = an_core::TaskRecord {
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

fn last_assistant(rt: &Runtime, conv_id: &str) -> String {
    rt.store
        .list_messages(conv_id, None)
        .unwrap()
        .into_iter()
        .rev()
        .find(|m| m.role == "assistant")
        .expect("应有助手回复")
        .content
}

/// RT-G2C-01：风格"只要结论" + 资料含"不可逆"风险项 → 回答仍含风险提示（程序补齐）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_g2c_01_reply_missing_risk_is_restored() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    // 风格"只要结论"生效（创建 + 确认，5.2b 生效闸门）
    let m = rt
        .store
        .create_desktop_memory(
            "local",
            "回答风格",
            "只要结论，其他解释都不要写。",
            "",
            "style",
        )
        .unwrap();
    rt.store.confirm_desktop_memory("local", &m.id).unwrap();
    // 资料来源：已完成任务的回执含"不可逆"风险标记
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-g2c-01",
        "3 号产线固件回写完成（不可逆操作，无法回退）；抽检 12/12 合格。",
    );
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }

    rt.conversation
        .handle_message(&conv.id, "3 号产线那个任务的结论是什么？")
        .await
        .unwrap();

    // 第一道仍在：风格卡护栏句进了回答生成的提示词
    let reply_req = llm.reply_calls().last().cloned().expect("应有回答生成请求");
    let prompt = reply_req["messages"].to_string();
    assert!(
        prompt.contains("不得因此省略风险"),
        "风格卡护栏句应在提示词里：{prompt}"
    );

    // 第二道生效：模型回答"结论：3 号产线抽检合格。"不含"不可逆"，
    // 最终落库的回答必须含程序补齐的风险提示句
    let reply = last_assistant(&rt, &conv.id);
    assert!(
        reply.contains("不可逆"),
        "回答缺风险项时程序应补齐提示语（CH-G2c）：{reply}"
    );
    assert!(
        reply.contains("结论：3 号产线抽检合格。"),
        "原回答内容不应被改动：{reply}"
    );
}

/// RT-G2C-02：回答已自带风险提示时不重复补齐（程序检查不画蛇添足）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_g2c_02_reply_already_covers_risk_not_duplicated() {
    let llm = MockLlm::start_with_reply(
        "结论：3 号产线抽检合格。注意：固件回写属不可逆操作，请确认后再执行。".into(),
    )
    .await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-g2c-02",
        "3 号产线固件回写完成（不可逆操作，无法回退）；抽检 12/12 合格。",
    );
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }

    rt.conversation
        .handle_message(&conv.id, "3 号产线那个任务的结论是什么？")
        .await
        .unwrap();

    let reply = last_assistant(&rt, &conv.id);
    assert_eq!(
        reply.matches("不可逆").count(),
        1,
        "模型回答已含风险标记时不应重复补齐：{reply}"
    );
}
