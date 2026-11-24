//! RT-UFND：内省发现按需进入会话（conversation-memory 5.3，REQ-conversation-memory-04）。
//! 用例定义见 docs/testing/CASES.md「内省发现按需进入会话」。
//! 口径（2026-11-23 用户确认）：确定性 bigram 相关性门（不占模型槽位）/ 待核实标注注入 /
//! 过闸门即可候选（人工否决优先）/ 2 条独立计；注入纯只读（发现不自动晋升）。

use std::path::Path;
use std::sync::{Arc, Mutex};

use an_runtime::config::AppConfig;
use an_runtime::introspect::model::IntrospectionFinding;
use an_runtime::session_materials::MATERIALS_MARKER;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 假模型：turn_decision → respond；文本 → 固定回答（SSE）；记录请求体
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

fn runtime(base: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("ufnd"));
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

/// 种一条发现（默认过闸门、无人工否决）
fn seed_finding(
    rt: &Runtime,
    id: &str,
    topic: &str,
    hypothesis: &str,
    verification: &str,
    case_ids: &[&str],
    verdict: Option<&str>,
) {
    let json = json!({
        "template": "方案采纳模式",
        "hypothesis": hypothesis,
        "next_step": "复核原始记录",
        "case_ids": case_ids,
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&IntrospectionFinding {
            id: id.into(),
            job_id: format!("job-{id}"),
            topic: topic.into(),
            json,
            verification: verification.into(),
            gate_score: Some(0.8),
            gate_reason: "测试直过".into(),
            pushed: false,
            verdict: verdict.map(Into::into),
            verdict_note: None,
            created_at: ts.clone(),
            updated_at: ts,
        })
        .unwrap();
}

fn materials_of(req: &Value) -> String {
    req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| {
            let c = m["content"].as_str()?;
            c.contains(MATERIALS_MARKER).then(|| c.to_string())
        })
        .unwrap_or_default()
}

async fn ask(rt: &Runtime, conv_id: &str, text: &str) {
    rt.conversation.handle_message(conv_id, text).await.unwrap();
}

/// RT-UFND-01：相关提问注入发现；无关提问不注入（相关性门）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_ufnd_01_relevance_gate_selects_related_question() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_finding(
        &rt,
        "intf-ufnd-01",
        "方案采纳模式",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
        "supported",
        &["case-a1"],
        None,
    );
    ask(&rt, &conv.id, "你好，3 号产线 CPK 和方案采纳的情况？").await;
    let related = materials_of(llm.plain_calls().last().unwrap());
    assert!(
        related.contains("intf-ufnd-01"),
        "相关提问应注入发现：{related}"
    );
    ask(&rt, &conv.id, "你好，公司食堂今天中午吃什么？").await;
    let unrelated = materials_of(llm.plain_calls().last().unwrap());
    assert!(
        !unrelated.contains("intf-ufnd-01"),
        "无关提问不得注入发现：{unrelated}"
    );
}

/// RT-UFND-02：inconclusive 注入带「待核实」；supported 无标注
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_ufnd_02_inconclusive_marked_pending_verification() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_finding(
        &rt,
        "intf-ufnd-02s",
        "方案采纳模式",
        "已验证结论：2 号产线方案采纳率下降。",
        "supported",
        &["case-s1"],
        None,
    );
    seed_finding(
        &rt,
        "intf-ufnd-02i",
        "方案采纳模式",
        "待核实结论：3 号产线方案采纳率异常升高，样本量不足。",
        "inconclusive",
        &["case-i1"],
        None,
    );
    ask(&rt, &conv.id, "你好，各产线的方案采纳率怎么样？").await;
    let c = materials_of(llm.plain_calls().last().unwrap());
    let s_line = c
        .lines()
        .find(|l| l.contains("intf-ufnd-02s"))
        .expect("supported 应注入：{c}");
    let i_line = c
        .lines()
        .find(|l| l.contains("intf-ufnd-02i"))
        .expect("inconclusive 应注入：{c}");
    assert!(
        i_line.contains("待核实"),
        "inconclusive 来源行必须带待核实标注：{i_line}"
    );
    assert!(
        !s_line.contains("待核实"),
        "supported 来源行不得带待核实标注：{s_line}"
    );
}

/// RT-UFND-03：人工否决（verdict=rejected）的匹配发现不注入
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_ufnd_03_rejected_finding_not_injected() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_finding(
        &rt,
        "intf-ufnd-03",
        "方案采纳模式",
        "被运营否决的结论：4 号产线方案误采纳。",
        "supported",
        &["case-r1"],
        Some("rejected"),
    );
    ask(&rt, &conv.id, "你好，4 号产线的方案采纳情况？").await;
    let c = materials_of(llm.plain_calls().last().unwrap());
    assert!(!c.contains("intf-ufnd-03"), "人工否决的发现不得注入：{c}");
}

/// RT-UFND-04：来源行含 finding id、topic 与证据 case id（可追溯）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_ufnd_04_source_line_carries_traceable_refs() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_finding(
        &rt,
        "intf-ufnd-04",
        "同因复发",
        "同一设备重复出现同因停机。",
        "supported",
        &["case-d1", "case-d2"],
        None,
    );
    ask(&rt, &conv.id, "你好，那台设备的同因复发情况？").await;
    let c = materials_of(llm.plain_calls().last().unwrap());
    let line = c
        .lines()
        .find(|l| l.contains("intf-ufnd-04"))
        .expect("应注入：{c}");
    assert!(line.contains("同因复发"), "来源行应含 topic：{line}");
    assert!(
        line.contains("case-d1") && line.contains("case-d2"),
        "来源行应含证据 case id：{line}"
    );
}

/// RT-UFND-05：注入纯只读——desktop_memories 行数不变、findings 无写入
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_ufnd_05_injection_never_promotes_or_writes() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base);
    let conv = rt.store.create_conversation("t").unwrap();
    seed_finding(
        &rt,
        "intf-ufnd-05",
        "方案采纳模式",
        "5 号产线方案采纳待核实。",
        "inconclusive",
        &["case-p1"],
        None,
    );
    let mem_before = rt.store.list_desktop_memories("local").unwrap().len();
    let finding_before = rt.store.introspection_get_finding("intf-ufnd-05").unwrap();
    ask(&rt, &conv.id, "你好，5 号产线的方案采纳情况？").await;
    let c = materials_of(llm.plain_calls().last().unwrap());
    assert!(c.contains("intf-ufnd-05"), "发现应注入：{c}");
    let mem_after = rt.store.list_desktop_memories("local").unwrap().len();
    assert_eq!(
        mem_before, mem_after,
        "注入不得自动晋升为个人记忆（发现 → desktop_memories 无写路径）"
    );
    let finding_after = rt.store.introspection_get_finding("intf-ufnd-05").unwrap();
    assert_eq!(
        finding_before.unwrap().updated_at,
        finding_after.unwrap().updated_at,
        "注入不得写 findings 表"
    );
}
