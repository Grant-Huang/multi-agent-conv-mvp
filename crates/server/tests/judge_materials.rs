//! RT-JMAT：判断路径会话资料清单（conversation-memory S4，REQ-conversation-memory-01 续接）。
//! 用例定义见 docs/testing/CASES.md「判断路径会话资料清单」。
//! 口径（2026-11-24 用户确认）：极简清单（≤3 行、要点≤40 字）/ 默认开 /
//! 仅判断路径 / 先合入后跑 E0 同口径对比。每轮收集一次，判断与回答两路共享。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::TaskRecord;
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

/// 假模型：记录请求体；turn_decision → respond；文本 → 固定回答（SSE）
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
            base: format!("http://{addr}"),
            requests,
        }
    }

    /// turn_decision schema 的请求（判断路径提示词）
    fn judge_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"] == json!("turn_decision"))
            .cloned()
            .collect()
    }
}

fn runtime(base: &str, conv_extra: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("jmat"));
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
[conversation]
{conv_extra}
"#,
        dir.join("files").display(),
    );
    let (config, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "未知配置键：{unknown:?}");
    Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap()
}

/// 种一个已完成任务（回执）并链接进会话
fn seed_linked_task(rt: &Runtime, conv_id: &str, task_id: &str, summary: &str) {
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

/// 灌 12 条直接回答消息，把回执挤出近期窗口
async fn push_window_out(rt: &Runtime, conv_id: &str) {
    for i in 0..12 {
        rt.conversation
            .handle_message(conv_id, &format!("你好，填充消息{i}"))
            .await
            .unwrap();
    }
}

/// 判断请求里是否有清单行（含指定 id）
fn judge_digest_contains(req: &Value, id: &str) -> bool {
    req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["content"].as_str().is_some_and(|c| c.contains(id)))
}

/// RT-JMAT-01：回执出窗后续问，判断请求含清单行（任务 id + 截断要点）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_01_judge_prompt_carries_materials_digest() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-jmat-01",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    push_window_out(&rt, &conv.id).await;
    rt.conversation
        .handle_message(&conv.id, "你好，3 号产线那个任务的结论是什么？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    assert!(
        judge_digest_contains(&req, "task-jmat-01"),
        "判断请求应含清单行（任务 id）：{:?}",
        req["messages"]
    );
}

/// RT-JMAT-02：清单同时含记忆与发现行（带来源 id）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_02_digest_includes_memories_and_findings() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-jmat-02", "任务回执要点。");
    rt.store
        .create_desktop_memory("local", "现行记忆", "现行的口径ABC。", "2099-01-01", "fact")
        .unwrap();
    let j = json!({
        "template": "方案采纳模式",
        "hypothesis": "报废工单集中在 3 号产线。",
        "case_ids": ["case-j2"],
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&an_runtime::introspect::model::IntrospectionFinding {
            id: "intf-jmat-02".into(),
            job_id: "job-jmat-02".into(),
            topic: "方案采纳模式".into(),
            json: j,
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
    // 记忆与发现不受消息窗口影响；回执需出窗——用直接消息填充
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，3 号产线的方案采纳和现行口径是什么？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    assert!(
        judge_digest_contains(&req, "task-jmat-02"),
        "清单应含任务行"
    );
    assert!(judge_digest_contains(&req, "现行记忆"), "清单应含记忆行");
    assert!(
        judge_digest_contains(&req, "intf-jmat-02"),
        "清单应含发现行"
    );
}

/// RT-JMAT-03：内省发现的相关性门——无关提问不注入发现行
/// （任务回执与记忆不受相关性门限制，为 5.1/5.2 既定语义；门只作用于发现）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_03_unrelated_question_no_digest() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let j = json!({
        "template": "方案采纳模式",
        "hypothesis": "报废工单集中在 3 号产线，方案仍被重复采纳。",
        "case_ids": ["case-j3"],
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&an_runtime::introspect::model::IntrospectionFinding {
            id: "intf-jmat-03".into(),
            job_id: "job-jmat-03".into(),
            topic: "方案采纳模式".into(),
            json: j,
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
    rt.conversation
        .handle_message(&conv.id, "你好，公司食堂今天吃什么？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    assert!(
        !judge_digest_contains(&req, "intf-jmat-03"),
        "无关提问不得注入发现行"
    );
}

/// RT-JMAT-04：资料源超时降级——判断照常（无清单、不失败）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_04_source_timeout_degrades_judge() {
    struct Slow;
    impl an_runtime::session_materials::MaterialSource for Slow {
        fn linked_task_ids(&self, _: &str) -> anyhow::Result<Vec<String>> {
            std::thread::sleep(Duration::from_millis(200));
            Ok(vec![])
        }
        fn personal_memories(
            &self,
            _: &str,
        ) -> anyhow::Result<Vec<an_runtime::store::DesktopMemory>> {
            std::thread::sleep(Duration::from_millis(200));
            Ok(vec![])
        }
        fn introspection_findings(
            &self,
        ) -> anyhow::Result<Vec<an_runtime::introspect::model::IntrospectionFinding>> {
            std::thread::sleep(Duration::from_millis(200));
            Ok(vec![])
        }
    }
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "session_materials_timeout_ms = 20");
    let conv = rt.store.create_conversation("t").unwrap();
    rt.conversation.set_material_source(Arc::new(Slow));
    rt.conversation
        .handle_message(&conv.id, "你好，怎么回答？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    assert!(
        !judge_digest_contains(&req, "task-jmat"),
        "超时后判断不得带清单"
    );
}

/// RT-JMAT-05：清单行要点截断 ≤40 字（+ 标记）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_05_digest_lines_clipped() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(
        &rt,
        &conv.id,
        "task-jmat-05",
        &"很长的研究结论内容。".repeat(300),
    );
    push_window_out(&rt, &conv.id).await;
    rt.conversation
        .handle_message(&conv.id, "你好，那个很长的研究任务结论是什么？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    assert!(
        judge_digest_contains(&req, "task-jmat-05"),
        "清单应含任务行"
    );
    let msgs = req["messages"].as_array().unwrap();
    let line = msgs
        .iter()
        .find_map(|m| {
            m["content"]
                .as_str()
                .and_then(|c| c.lines().find(|l| l.contains("task-jmat-05")))
        })
        .expect("应有清单行");
    let gist = line.split('：').next_back().expect("清单行应有要点段");
    assert!(
        gist.chars().count() <= 60,
        "清单行要点应截断（≤40 字 + 标记余量）：{line}"
    );
}

/// RT-JMAT-06（#363 / R-20261126-M2）：万字标题的记忆进判断清单时标题截断，
/// 来源 id 与要点仍在（清单行不被超长标题撑爆）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_jmat_06_digest_truncates_long_title() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let mut title = String::new();
    for i in 0..3000 {
        title.push_str(&format!("超长标题{i}"));
    }
    rt.store
        .create_desktop_memory("local", &title, "JMAT06 正文要点。", "2099-01-01", "fact")
        .unwrap();
    rt.conversation
        .handle_message(&conv.id, "你好，看看现在的背景情况？")
        .await
        .unwrap();
    let req = llm.judge_calls().last().unwrap().clone();
    let joined = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let line = joined
        .lines()
        .find(|l| l.contains("mem-") && l.contains("个人记忆"))
        .expect("应含记忆清单行");
    assert!(
        line.chars().count() < 200,
        "清单行应有界（标题截断）：{} 字",
        line.chars().count()
    );
    assert!(line.contains('…'), "超长标题应带省略号：{line}");
    assert!(line.contains("JMAT06"), "要点仍在：{line}");
}
