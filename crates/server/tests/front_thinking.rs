//! Runtime 集成用例 RT-FT-01 ~ RT-FT-11（定义见 docs/testing/CASES.md「前台提速」）：
//! 前台两次结构化判断（conversation 的 turn_decision、router 的 route_decision）在不思考时：
//! A 先写理由、B 规则判风险（只升不降）、D 可疑时开思考重判一次。
//!
//! 用假的 OpenAI 兼容服务记录每个请求体，并按脚本返回内容。
//! "不思考"指请求带 `chat_template_kwargs.enable_thinking = false`；"思考"指不带这个设置。

use std::sync::{Arc, Mutex};

use an_core::{Message, Risk, Role, TaskClass, TurnDecision};
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

// ───────────── FT.5：类别拆成两个是/否问题（RT-FT-12 ~ RT-FT-16） ─────────────

const CLASS_Q: &str = "route_class_questions = true";

/// 路由请求的系统提示词
fn system_prompt(req: &Value) -> String {
    req["messages"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// 两个是/否问题形式的 route_decision
fn route_q(needs_data_tools: bool, needs_document: bool) -> String {
    json!({"reason":"测试","needs_data_tools":needs_data_tools,"needs_document":needs_document,
           "execution_role":"general","required_tools":[],"risk_level":"low"})
    .to_string()
}

/// RT-FT-12：不写 route_class_questions 时，路由 schema 与提示词保持现状（基线守护）
#[tokio::test]
async fn rt_ft_12_class_questions_off_by_default() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route("general", "low"))).await;
    let rt = runtime(&llm.base, 0, "", "");
    rt.router
        .route("写一份周报", "", &[])
        .await
        .map_err(|e| e.message)
        .unwrap();

    let req = &llm.calls("route_decision")[0];
    let keys = schema_keys(req);
    assert!(keys.contains(&"task_class".to_string()), "{keys:?}");
    assert_eq!(
        schema_prop(req, "task_class")["enum"],
        json!(["L1", "L2", "L3", "L4"])
    );
    assert!(!keys.contains(&"needs_data_tools".to_string()), "{keys:?}");
    assert!(!keys.contains(&"needs_document".to_string()), "{keys:?}");
    let sys = system_prompt(req);
    assert!(!sys.contains("needs_data_tools"), "{sys}");
    assert!(!sys.contains("needs_document"), "{sys}");
}

/// RT-FT-13：打开后，schema 改为两个必填的布尔问题，排在 execution_role 之前；提示词写明口径
#[tokio::test]
async fn rt_ft_13_class_questions_schema_and_prompt() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route_q(true, false))).await;
    let rt = runtime(&llm.base, 0, "", CLASS_Q);
    rt.router
        .route("帮我算一下 3600 * 0.975", "", &[])
        .await
        .map_err(|e| e.message)
        .unwrap();

    let req = &llm.calls("route_decision")[0];
    let keys = schema_keys(req);
    assert!(!keys.contains(&"task_class".to_string()), "{keys:?}");
    for k in ["needs_data_tools", "needs_document"] {
        assert_eq!(schema_prop(req, k)["type"], json!("boolean"), "{k}");
        assert!(
            schema_required(req).contains(&k.to_string()),
            "{k} 应为必填"
        );
    }
    let pos = |k: &str| keys.iter().position(|x| x == k).unwrap();
    assert!(pos("needs_data_tools") < pos("execution_role"), "{keys:?}");
    assert!(pos("needs_document") < pos("execution_role"), "{keys:?}");

    let sys = system_prompt(req);
    assert!(sys.contains("needs_data_tools"), "{sys}");
    assert!(sys.contains("needs_document"), "{sys}");
    assert!(
        sys.contains("write_file"),
        "提示词应说明保存结果的 write_file 不算：{sys}"
    );
    assert!(
        !sys.contains("task_class"),
        "不应再要求模型选 task_class：{sys}"
    );
}

/// RT-FT-14：四种是/否组合分别算出 L1～L4
#[tokio::test]
async fn rt_ft_14_class_derived_from_answers() {
    let answers = [(false, false), (true, false), (false, true), (true, true)];
    let llm = MockLlm::start(Arc::new(move |_, idx, _| {
        let (t, d) = answers[idx];
        route_q(t, d)
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", CLASS_Q);
    let mut got = Vec::new();
    for _ in 0..4 {
        let out = rt
            .router
            .route("任务", "", &[])
            .await
            .map_err(|e| e.message)
            .unwrap();
        got.push(out.decision.task_class);
    }
    assert_eq!(
        got,
        vec![TaskClass::L1, TaskClass::L2, TaskClass::L3, TaskClass::L4]
    );
}

/// RT-FT-15：同时打开 A（先写理由）时，reason 仍是第一个字段，其后是两个问题
#[tokio::test]
async fn rt_ft_15_class_questions_with_reason_first() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route_q(false, true))).await;
    let rt = runtime(&llm.base, 0, THINK_OFF_REASON, CLASS_Q);
    rt.router
        .route("写一份周报", "", &[])
        .await
        .map_err(|e| e.message)
        .unwrap();

    let req = &llm.calls("route_decision")[0];
    let keys = schema_keys(req);
    assert_eq!(
        keys[..3],
        ["reason", "needs_data_tools", "needs_document"],
        "{keys:?}"
    );
    assert!(thinking_off(req));
}

/// RT-FT-16：开关与思考无关：不关思考时同样生效，且请求不带 enable_thinking
#[tokio::test]
async fn rt_ft_16_class_questions_independent_of_thinking() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route_q(true, true))).await;
    let rt = runtime(&llm.base, 0, "", CLASS_Q);
    let out = rt
        .router
        .route("读取 data.csv 写成报告", "", &[])
        .await
        .map_err(|e| e.message)
        .unwrap();

    let req = &llm.calls("route_decision")[0];
    assert!(req.get("chat_template_kwargs").is_none(), "{req}");
    assert!(schema_keys(req).contains(&"needs_data_tools".to_string()));
    assert_eq!(out.decision.task_class, TaskClass::L4);
}

// ───────────── FT.5b：类别由工具清单算出（RT-FT-17 ~ RT-FT-20） ─────────────

const FROM_TOOLS: &str = "route_class_from_tools = true";

/// 模型直接给类别的 route_decision（指定角色、类别和工具）
fn route_ct(role: &str, class: &str, tools: &[&str]) -> String {
    json!({"reason":"测试","task_class":class,"execution_role":role,"required_tools":tools,"risk_level":"low"})
        .to_string()
}

/// 取路由结果里的类别：成功时取决策；Policy 拒绝（如角色没有可用 agent）时取失败里带的决策
async fn routed_class(rt: &Runtime, goal: &str) -> TaskClass {
    match rt.router.route(goal, "", &[]).await {
        Ok(out) => out.decision.task_class,
        Err(f) => {
            f.decision
                .unwrap_or_else(|| panic!("路由没有给出决策：{}", f.message))
                .task_class
        }
    }
}

/// RT-FT-17：route_class_from_tools = false 时，类别保持模型给的值。
/// 2026-11-19 需求变更（DECISIONS 第 46 条，用户同意）：默认值改为打开，
/// 本用例由"不写配置"改为"显式关闭"，断言不变。
#[tokio::test]
async fn rt_ft_17_class_from_tools_off_keeps_model_class() {
    let llm = MockLlm::start(Arc::new(|_, _, _| {
        route_ct("general", "L1", &["read_file"])
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", "route_class_from_tools = false");
    assert_eq!(
        routed_class(&rt, "读一下 notes/todo.md").await,
        TaskClass::L1
    );
}

/// RT-FT-18：打开后，general 任务按工具清单重算类别（方案 B：要文档 = 有 write_file 或模型给 L3/L4）
#[tokio::test]
async fn rt_ft_18_class_from_tools_for_general() {
    let script: [(&str, &[&str]); 5] = [
        ("L1", &["read_file"]),
        ("L2", &[]),
        ("L1", &["write_file"]),
        ("L3", &[]),
        ("L1", &["read_file", "write_file"]),
    ];
    let llm = MockLlm::start(Arc::new(move |_, idx, _| {
        let (class, tools) = script[idx];
        route_ct("general", class, tools)
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", FROM_TOOLS);
    let mut got = Vec::new();
    for _ in 0..5 {
        got.push(routed_class(&rt, "任务").await);
    }
    assert_eq!(
        got,
        vec![
            TaskClass::L2,
            TaskClass::L1,
            TaskClass::L3,
            TaskClass::L3,
            TaskClass::L4
        ]
    );
}

/// RT-FT-19：打开后，非 general 角色的类别不变
#[tokio::test]
async fn rt_ft_19_class_from_tools_ignores_other_roles() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route_ct("coding", "L4", &[]))).await;
    let rt = runtime(&llm.base, 0, "", FROM_TOOLS);
    assert_eq!(routed_class(&rt, "修复登录接口").await, TaskClass::L4);
}

/// RT-FT-20：开关只影响结果的计算，发给模型的请求（提示词与 schema）完全不变
#[tokio::test]
async fn rt_ft_20_class_from_tools_does_not_change_request() {
    let mut reqs = Vec::new();
    for conv in ["", FROM_TOOLS] {
        let llm = MockLlm::start(Arc::new(|_, _, _| {
            route_ct("general", "L2", &["read_file"])
        }))
        .await;
        let rt = runtime(&llm.base, 0, "", conv);
        routed_class(&rt, "读一下 notes/todo.md").await;
        let req = llm.calls("route_decision")[0].clone();
        reqs.push((req["messages"].clone(), req["response_format"].clone()));
    }
    assert_eq!(reqs[0], reqs[1]);
}

// ───────────── FT.6：前台不扩写任务描述（RT-FT-21 ~ RT-FT-22） ─────────────

const CONCISE: &str = "concise_task_goal = true";
const CONCISE_RULE: &str = "不要添加用户没有提出的要求";

/// 跑一次前台判断，返回发给模型的 turn_decision 请求
async fn turn_request(conv_extra: &str) -> Value {
    let llm = MockLlm::start(Arc::new(|_, _, _| turn_ok("写一份周报"))).await;
    let rt = runtime(&llm.base, 0, "", conv_extra);
    rt.conversation
        .decide_turn(&user("写一份周报"), &[])
        .await
        .map_err(|e| e.to_string())
        .unwrap();
    llm.calls("turn_decision")[0].clone()
}

/// RT-FT-21：不写 concise_task_goal 时，task_goal 规则保持原样（基线守护）
#[tokio::test]
async fn rt_ft_21_concise_goal_off_keeps_prompt() {
    let sys = system_prompt(&turn_request("").await);
    assert!(sys.contains("写成完整、可独立执行的任务描述"), "{sys}");
    assert!(!sys.contains(CONCISE_RULE), "{sys}");
}

/// RT-FT-22：打开后，提示词要求不扩写，但仍补全指代、仍原样放入贴出的内容；schema 不变
#[tokio::test]
async fn rt_ft_22_concise_goal_on_changes_prompt_only() {
    let off = turn_request("").await;
    let on = turn_request(CONCISE).await;
    let sys = system_prompt(&on);
    assert!(sys.contains(CONCISE_RULE), "{sys}");
    assert!(sys.contains("指代"), "应要求补全指代：{sys}");
    assert!(
        sys.contains("原样放进 task_goal"),
        "贴出的内容仍须原样放入：{sys}"
    );
    assert_eq!(
        on["response_format"], off["response_format"],
        "schema 不应改变"
    );
}

// ───────────── FT.5b 默认打开（RT-FT-23 ~ RT-FT-24） ─────────────

/// RT-FT-23：不写 route_class_from_tools 时默认打开，general 任务按工具清单重算类别
#[tokio::test]
async fn rt_ft_23_class_from_tools_on_by_default() {
    let llm = MockLlm::start(Arc::new(|_, _, _| {
        route_ct("general", "L1", &["read_file"])
    }))
    .await;
    let rt = runtime(&llm.base, 0, "", "");
    assert_eq!(
        routed_class(&rt, "读一下 notes/todo.md").await,
        TaskClass::L2
    );
}

/// RT-FT-24：两问与工具清单规则同时打开时，以两问为准
#[tokio::test]
async fn rt_ft_24_class_questions_take_precedence() {
    let llm = MockLlm::start(Arc::new(|_, _, _| route_q(true, false))).await;
    let rt = runtime(
        &llm.base,
        0,
        "",
        "route_class_questions = true\nroute_class_from_tools = true",
    );
    assert_eq!(
        routed_class(&rt, "帮我算一下 3600 * 0.975").await,
        TaskClass::L2
    );
}
