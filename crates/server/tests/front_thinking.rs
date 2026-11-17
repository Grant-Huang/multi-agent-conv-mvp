//! Runtime 集成用例 RT-FT-01 ~ RT-FT-11（定义见 docs/testing/CASES.md「前台提速」）：
//! 前台两次结构化判断（conversation 的 turn_decision、router 的 route_decision）在不思考时：
//! A 先写理由、B 规则判风险（只升不降）、D 可疑时开思考重判一次。
//!
//! 用假的 OpenAI 兼容服务记录每个请求体，并按脚本返回内容。
//! "不思考"指请求带 `chat_template_kwargs.enable_thinking = false`；"思考"指不带这个设置。

use std::sync::{Arc, Mutex};

use an_core::{Message, Risk, Role, TurnDecision};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 脚本：(schema 名, 该 schema 的第几次请求（从 0 开始）, 这次是否不思考) → 模型输出
type Script = Arc<dyn Fn(&str, usize, bool) -> String + Send + Sync>;

/// 假模型服务：记录请求体，按脚本返回流式结果
struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start(script: Script) -> Self {
        let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = requests.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let log = log.clone();
                let script = script.clone();
                async move {
                    let schema = schema_name(&req).to_string();
                    let off = thinking_off(&req);
                    let idx = {
                        let mut l = log.lock().unwrap();
                        let idx = l.iter().filter(|r| schema_name(r) == schema).count();
                        l.push(req.clone());
                        idx
                    };
                    let content = script(&schema, idx, off);
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

    /// 某个 schema 的全部请求（按发送顺序）
    fn calls(&self, schema: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| schema_name(r) == schema)
            .cloned()
            .collect()
    }
}

fn schema_name(req: &Value) -> &str {
    req["response_format"]["json_schema"]["name"]
        .as_str()
        .unwrap_or("")
}

fn thinking_off(req: &Value) -> bool {
    req["chat_template_kwargs"]["enable_thinking"] == json!(false)
}

/// 请求里 JSON Schema 的属性名（保持顺序）
fn schema_keys(req: &Value) -> Vec<String> {
    req["response_format"]["json_schema"]["schema"]["properties"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

fn schema_prop<'a>(req: &'a Value, key: &str) -> &'a Value {
    &req["response_format"]["json_schema"]["schema"]["properties"][key]
}

fn schema_required(req: &Value) -> Vec<String> {
    req["response_format"]["json_schema"]["schema"]["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// 构造 Runtime：前台两个角色都走名为 local 的后端（指向假服务）。
/// `backend_extra` 写进 `[backends.local]`，`conv_extra` 写进 `[conversation]`。
fn runtime(base: &str, max_retries: u32, backend_extra: &str, conv_extra: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("front-thinking"));
    let text = format!(
        r#"
[llm]
base_url = "{base}"
model = "scripted"
max_retries = {max_retries}
timeout_secs = 5
[backends.local]
kind = "openai"
base_url = "{base}"
{backend_extra}
[routing]
conversation = "local"
router = "local"
general = "local"
reviewer = "local"
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
[acp]
auto_detect = false
[conversation]
{conv_extra}
"#,
        files = dir.display()
    );
    let (config, _unknown) = AppConfig::parse(&text).unwrap();
    Runtime::new(
        config,
        Store::open(std::path::Path::new(":memory:")).unwrap(),
    )
    .unwrap()
}

fn user(text: &str) -> Vec<Message> {
    vec![Message {
        id: "m1".into(),
        conversation_id: "c1".into(),
        role: "user".into(),
        content: text.into(),
        task_id: None,
        created_at: an_core::now(),
    }]
}

/// 合格的 turn_decision（delegate）
fn turn_ok(goal: &str) -> String {
    json!({"reason":"需要后台执行","action":"delegate","question":"","task_goal":goal,"task_id":"","follow_up_kind":"","change":""}).to_string()
}

/// route_decision
fn route(role: &str, risk: &str) -> String {
    json!({"reason":"测试","task_class":"L2","execution_role":role,"required_tools":[],"risk_level":risk}).to_string()
}

const THINK_OFF: &str = "front_structured_thinking = false";
const THINK_OFF_REASON: &str = "front_structured_thinking = false\nfront_reason_first = true";
const THINK_OFF_ESCALATE: &str =
    "front_structured_thinking = false\nfront_thinking_escalation = true";

/// RT-FT-01：不写新配置时，请求与现在完全相同
#[tokio::test]
async fn rt_ft_01_defaults_unchanged() {
    let llm = MockLlm::start(Arc::new(|schema, _, _| {
        if schema == "turn_decision" {
            turn_ok("写一份周报")
        } else {
            route("general", "low")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", "");
    rt.conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .unwrap();
    assert!(rt.router.route("写一份周报", "", &[]).await.is_ok());

    let turn = &llm.calls("turn_decision")[0];
    let route_req = &llm.calls("route_decision")[0];
    assert!(turn.get("chat_template_kwargs").is_none(), "{turn}");
    assert!(
        route_req.get("chat_template_kwargs").is_none(),
        "{route_req}"
    );
    assert!(!schema_keys(turn).contains(&"reason".to_string()));
    assert_eq!(
        schema_keys(route_req).last().map(String::as_str),
        Some("reason")
    );
}

/// RT-FT-02：不思考 + 先写理由 → 两个 schema 的第一个字段都是 reason，限 40 字且必填
#[tokio::test]
async fn rt_ft_02_reason_first_when_thinking_off() {
    let llm = MockLlm::start(Arc::new(|schema, _, _| {
        if schema == "turn_decision" {
            turn_ok("写一份周报")
        } else {
            route("general", "low")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 0, THINK_OFF_REASON, "");
    rt.conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .unwrap();
    assert!(rt.router.route("写一份周报", "", &[]).await.is_ok());

    for schema in ["turn_decision", "route_decision"] {
        let req = &llm.calls(schema)[0];
        assert_eq!(
            schema_keys(req).first().map(String::as_str),
            Some("reason"),
            "{schema} 的第一个字段应为 reason：{:?}",
            schema_keys(req)
        );
        assert_eq!(schema_prop(req, "reason")["maxLength"], 40, "{schema}");
        assert!(
            schema_required(req).contains(&"reason".to_string()),
            "{schema}"
        );
    }
}

/// RT-FT-03：只开 front_reason_first、不关思考 → schema 与基线相同
#[tokio::test]
async fn rt_ft_03_reason_first_ignored_when_thinking_on() {
    let llm = MockLlm::start(Arc::new(|schema, _, _| {
        if schema == "turn_decision" {
            turn_ok("写一份周报")
        } else {
            route("general", "low")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 0, "front_reason_first = true", "");
    rt.conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .unwrap();
    assert!(rt.router.route("写一份周报", "", &[]).await.is_ok());

    assert!(!schema_keys(&llm.calls("turn_decision")[0]).contains(&"reason".to_string()));
    assert_eq!(
        schema_keys(&llm.calls("route_decision")[0])
            .last()
            .map(String::as_str),
        Some("reason")
    );
}

/// RT-FT-04：规则判风险只升不降
#[tokio::test]
async fn rt_ft_04_risk_rules_only_raise() {
    let llm = MockLlm::start(Arc::new(|_, idx, _| match idx {
        0 | 1 => route("general", "low"),
        _ => route("general", "high"),
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", "risk_rules = true");
    let cases = [
        ("删除生产环境数据库里的旧表", Risk::High),
        ("修改 src/main.rs 里的函数", Risk::Medium),
        ("总结这篇文章", Risk::High),
    ];
    for (goal, want) in cases {
        let out = rt.router.route(goal, "", &[]).await.ok().unwrap();
        assert_eq!(out.decision.risk_level, want, "任务：{goal}");
    }
}

/// RT-FT-05：不开 risk_rules 时，风险保持模型给的值
#[tokio::test]
async fn rt_ft_05_risk_rules_off_keeps_model_value() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route("general", "low"))).await;
    let rt = runtime(&llm.base, 0, "", "");
    let out = rt
        .router
        .route("删除生产环境数据库里的旧表", "", &[])
        .await
        .ok()
        .unwrap();
    assert_eq!(out.decision.risk_level, Risk::Low);
}

/// RT-FT-06：前台判断格式不合格 → 开思考重判
#[tokio::test]
async fn rt_ft_06_format_error_escalates() {
    let llm = MockLlm::start(Arc::new(|_, idx, _| {
        if idx == 0 {
            "这不是 JSON".into()
        } else {
            turn_ok("写一份周报")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 2, THINK_OFF_ESCALATE, "");
    let out = rt
        .conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .unwrap();
    assert!(matches!(out.value, TurnDecision::Delegate { .. }));
    let calls = llm.calls("turn_decision");
    assert_eq!(calls.len(), 2, "应只请求两次");
    assert!(thinking_off(&calls[0]), "第 1 次应不思考");
    assert!(!thinking_off(&calls[1]), "第 2 次应开思考重判");
}

/// RT-FT-07：路由给出 high → 开思考重判，以重判结果为准
#[tokio::test]
async fn rt_ft_07_high_risk_escalates() {
    let llm = MockLlm::start(Arc::new(|_, _, off| {
        if off {
            route("general", "high")
        } else {
            route("general", "medium")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 0, THINK_OFF_ESCALATE, "");
    let out = rt.router.route("整理会议纪要", "", &[]).await.ok().unwrap();
    assert_eq!(out.decision.risk_level, Risk::Medium);
    let calls = llm.calls("route_decision");
    assert_eq!(calls.len(), 2);
    assert!(thinking_off(&calls[0]) && !thinking_off(&calls[1]));
}

/// RT-FT-08：路由选了没有可用 agent 的角色 → 开思考重判
#[tokio::test]
async fn rt_ft_08_no_agent_role_escalates() {
    let llm = MockLlm::start(Arc::new(|_, _, off| {
        if off {
            route("coding", "low")
        } else {
            route("general", "low")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 0, THINK_OFF_ESCALATE, "");
    let out = rt
        .router
        .route("整理会议纪要", "", &[])
        .await
        .ok()
        .expect("重判后应路由成功");
    assert_eq!(out.decision.execution_role, Role::General);
    let calls = llm.calls("route_decision");
    assert_eq!(calls.len(), 2);
    assert!(!thinking_off(&calls[1]));
}

/// RT-FT-09：delegate 但任务目标为空 → 开思考重判
#[tokio::test]
async fn rt_ft_09_empty_goal_escalates() {
    let llm = MockLlm::start(Arc::new(|_, _, off| {
        if off {
            json!({"reason":"","action":"delegate","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""}).to_string()
        } else {
            turn_ok("写一份周报")
        }
    }))
    .await;
    let rt = runtime(&llm.base, 2, THINK_OFF_ESCALATE, "");
    let out = rt
        .conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .unwrap();
    assert!(matches!(&out.value, TurnDecision::Delegate { task_goal } if !task_goal.is_empty()));
    let calls = llm.calls("turn_decision");
    assert_eq!(calls.len(), 2);
    assert!(thinking_off(&calls[0]) && !thinking_off(&calls[1]));
}

/// RT-FT-10：最多升级一次；升级后按原有重试规则，不会退回不思考
#[tokio::test]
async fn rt_ft_10_escalates_at_most_once() {
    let llm = MockLlm::start(Arc::new(|_, _, _| "这不是 JSON".into())).await;
    let rt = runtime(&llm.base, 1, THINK_OFF_ESCALATE, "");
    let out = rt.conversation.decide_turn(&user("写一份周报"), &[]).await;
    assert!(out.is_err(), "始终不合格应返回错误");
    let calls = llm.calls("turn_decision");
    let off: Vec<bool> = calls.iter().map(thinking_off).collect();
    assert_eq!(
        off,
        vec![true, false, false],
        "应为 1 次不思考 + (1 + max_retries) 次思考"
    );
}

/// RT-FT-11：不开升级，或没关思考时，重试的思考设置不变（与现在一致）
#[tokio::test]
async fn rt_ft_11_no_escalation_without_both_switches() {
    for (extra, want_off) in [
        (THINK_OFF, true),
        ("front_thinking_escalation = true", false),
    ] {
        let llm = MockLlm::start(Arc::new(|_, idx, _| {
            if idx == 0 {
                "这不是 JSON".into()
            } else {
                turn_ok("写一份周报")
            }
        }))
        .await;
        let rt = runtime(&llm.base, 2, extra, "");
        rt.conversation
            .decide_turn(&user("写一份周报"), &[])
            .await
            .unwrap();
        let off: Vec<bool> = llm
            .calls("turn_decision")
            .iter()
            .map(thinking_off)
            .collect();
        assert_eq!(off, vec![want_off, want_off], "配置：{extra}");
    }
}
