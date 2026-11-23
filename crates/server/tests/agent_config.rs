//! 用例 RT-AGCFG-01 ~ RT-AGCFG-14（定义见 docs/testing/CASES.md「托管配置、热注册与撤销」）：
//! 通过提议 + 一次性确认凭证修改托管文件，不重启就生效；可撤销；config.toml 永远优先；
//! 没有可用 coding agent 时由前台模型用大白话回复。
//!
//! 接口通过 HTTP 驱动；注册表、编排器直接调用 Runtime。
//! 需要"已安装预设"的用例会把 PATH、HOME 指向临时目录（进程级全局变量），
//! 所以本文件所有用例用同一把锁串行执行。本文件是独立测试二进制。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::{PolicyVerdict, Risk, Role, RouteDecision, TaskClass};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post as route_post;
use axum::Json;
use serde_json::{json, Value};

/// 用例持锁期间会 await，所以用 tokio 的锁
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const MOCK: &str = env!("CARGO_BIN_EXE_an-mock-acp");

/// 用例独享的临时目录：bin/ 放假命令，home/ 当 HOME，data/ 放数据库和托管文件
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(an_core::new_id(&format!("agcfg-{name}")));
        for d in ["bin", "home", "data", "files", "code"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        Self { root }
    }

    fn install(&self, command: &str) -> PathBuf {
        let p = self.root.join("bin").join(command);
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    fn managed_file(&self) -> PathBuf {
        self.root.join("data").join("agents.managed.toml")
    }

    fn config_text(&self, llm_base: &str, extra: &str) -> String {
        format!(
            r#"
[server]
db_path = "{data}/agentnexus.db"
[llm]
base_url = "{llm_base}"
model = "m"
max_retries = 0
timeout_secs = 5
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
workdir = "{code}"
{extra}
"#,
            data = self.root.join("data").display(),
            files = self.root.join("files").display(),
            code = self.root.join("code").display(),
        )
    }

    /// 在 PATH = bin/、HOME = home/ 的环境下构造 Runtime 并启动 HTTP 服务
    async fn serve(&self, llm_base: &str, extra: &str) -> (String, Arc<Runtime>) {
        let (config, _) = AppConfig::parse(&self.config_text(llm_base, extra)).unwrap();
        let old_path = std::env::var_os("PATH");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("PATH", self.root.join("bin"));
        std::env::set_var("HOME", self.root.join("home"));
        let rt = Runtime::new(config, Store::open(Path::new(":memory:")).unwrap());
        restore("PATH", old_path);
        restore("HOME", old_home);
        let rt = Arc::new(rt.unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let for_serve = rt.clone();
        tokio::spawn(async move {
            axum::serve(listener, an_server::api(for_serve))
                .await
                .unwrap()
        });
        (format!("http://{addr}"), rt)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn restore(key: &str, old: Option<std::ffi::OsString>) {
    match old {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

/// 不会被调用的模型地址（只做配置操作的用例）
const NO_LLM: &str = "http://127.0.0.1:9/v1";

async fn post(base: &str, path: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

/// 提交提议并立即确认；返回确认结果
async fn propose_and_apply(base: &str, body: Value) -> (u16, Value) {
    let (status, p) = post(base, "/api/agents/proposals", body.clone()).await;
    assert_eq!(status, 200, "提议应被接受：{body} → {p}");
    let id = p["id"].as_str().expect("提议应有 id").to_string();
    let token = p["token"].as_str().expect("提议应带确认凭证").to_string();
    post(
        base,
        &format!("/api/agents/proposals/{id}/apply"),
        json!({ "token": token }),
    )
    .await
}

fn has_card(rt: &Runtime, id: &str) -> bool {
    rt.registry.cards().iter().any(|c| c.id == id)
}

/// 自定义 ACP agent（指向模拟 agent）
fn custom(id: &str, agent_type: &str) -> Value {
    json!({"kind": "add_custom", "agent": {"id": id, "command": MOCK, "role": "coding", "agent_type": agent_type}})
}

/// config.toml 里写一个 ACP agent
fn toml_agent(id: &str) -> String {
    format!("[[acp.agents]]\nid = \"{id}\"\ncommand = \"{MOCK}\"\nrole = \"coding\"\n")
}

/// RT-AGCFG-01：接入已安装的预设，不重启即生效
#[tokio::test]
async fn rt_agcfg_01_add_preset_hot() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("01");
    sb.install("claude-agent-acp");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    assert!(!has_card(&rt, "claude"));
    let (status, out) =
        propose_and_apply(&base, json!({"kind": "add_preset", "id": "claude"})).await;
    assert_eq!(status, 200, "{out}");
    assert!(has_card(&rt, "claude"), "确认后应立即注册");
    let catalog: Value = reqwest::get(format!("{base}/api/agents/catalog"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let item = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "claude")
        .unwrap();
    assert_eq!(item["status"], "registered");
    assert!(
        rt.registry.links.state("claude").last_probe.is_some(),
        "新接入的 agent 应立即做一次握手检测"
    );
    let text = std::fs::read_to_string(sb.managed_file()).expect("应写出托管文件");
    assert!(text.contains("claude"), "{text}");
}

/// RT-AGCFG-02：设为默认后，新分配交给它
#[tokio::test]
async fn rt_agcfg_02_set_default() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("02");
    let extra = format!(
        "[acp]\nauto_detect = false\n{}{}",
        toml_agent("m1"),
        toml_agent("m2")
    );
    let (base, rt) = sb.serve(NO_LLM, &extra).await;
    let (status, out) = propose_and_apply(
        &base,
        json!({"kind": "set_default", "role": "coding", "ids": ["m2"]}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let lease = rt
        .registry
        .acquire(Role::Coding, |_| {})
        .await
        .ok()
        .unwrap();
    assert_eq!(lease.agent.card().id, "m2");
}

/// RT-AGCFG-03：停用后不再分配；执行中的名额照常可用、可归还
#[tokio::test]
async fn rt_agcfg_03_disable_keeps_running_lease() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("03");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    assert_eq!(propose_and_apply(&base, custom("a1", "mock")).await.0, 200);
    assert_eq!(propose_and_apply(&base, custom("a2", "mock")).await.0, 200);
    let running = rt
        .registry
        .acquire(Role::Coding, |_| {})
        .await
        .ok()
        .unwrap();
    assert_eq!(running.agent.card().id, "a1");
    let (status, out) = propose_and_apply(&base, json!({"kind": "disable", "id": "a1"})).await;
    assert_eq!(status, 200, "{out}");
    assert!(!has_card(&rt, "a1"));
    // 执行中的名额仍持有 agent 引用
    assert_eq!(running.agent.card().id, "a1");
    let next = rt
        .registry
        .acquire(Role::Coding, |_| {})
        .await
        .ok()
        .unwrap();
    assert_eq!(next.agent.card().id, "a2");
    drop(running);
    drop(next);
}

/// RT-AGCFG-04：停用唯一的 coding agent 时，排队的分配报 E_ROUTE_NO_AGENT，并有大白话说明
#[tokio::test]
async fn rt_agcfg_04_disable_last_fails_queued() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("04");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    assert_eq!(
        propose_and_apply(&base, custom("only", "mock")).await.0,
        200
    );
    let busy = rt
        .registry
        .acquire(Role::Coding, |_| {})
        .await
        .ok()
        .unwrap();
    let reg = rt.registry.clone();
    let queued = tokio::spawn(async move { reg.acquire(Role::Coding, |_| {}).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (status, out) = propose_and_apply(&base, json!({"kind": "disable", "id": "only"})).await;
    assert_eq!(status, 200, "{out}");
    let err = tokio::time::timeout(Duration::from_secs(10), queued)
        .await
        .expect("排队的分配应很快结束")
        .unwrap()
        .err()
        .expect("应报错");
    assert_eq!(err.code, an_core::codes::ROUTE_NO_AGENT);
    assert!(
        err.message.contains("停用") || err.message.contains("没有安装"),
        "说明应让非专业用户看懂：{}",
        err.message
    );
    drop(busy);
}

/// RT-AGCFG-05：撤销可以连续进行，没有可撤销的版本时被拒绝
#[tokio::test]
async fn rt_agcfg_05_revert_steps_back() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("05");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    assert_eq!(propose_and_apply(&base, custom("r1", "mock")).await.0, 200);
    assert_eq!(propose_and_apply(&base, custom("r2", "mock")).await.0, 200);
    assert!(has_card(&rt, "r1") && has_card(&rt, "r2"));

    assert_eq!(
        propose_and_apply(&base, json!({"kind": "revert"})).await.0,
        200
    );
    assert!(has_card(&rt, "r1") && !has_card(&rt, "r2"));
    assert_eq!(
        propose_and_apply(&base, json!({"kind": "revert"})).await.0,
        200
    );
    assert!(!has_card(&rt, "r1") && !has_card(&rt, "r2"));

    let (status, out) = post(&base, "/api/agents/proposals", json!({"kind": "revert"})).await;
    assert_eq!(status, 409, "没有可撤销的版本时应拒绝：{out}");
}

/// RT-AGCFG-06：重启后托管内容仍生效；同 id 时托管配置优先于自动发现的预设
#[tokio::test]
async fn rt_agcfg_06_survives_restart() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("06");
    sb.install("claude-agent-acp");
    {
        let (base, _rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
        assert_eq!(
            propose_and_apply(&base, custom("keep", "mock")).await.0,
            200
        );
        let mine = json!({"kind": "add_custom", "agent": {"id": "claude", "name": "托管的 Claude", "command": MOCK, "role": "coding"}});
        assert_eq!(propose_and_apply(&base, mine).await.0, 200);
        let set = json!({"kind": "set_default", "role": "coding", "ids": ["keep"]});
        assert_eq!(propose_and_apply(&base, set).await.0, 200);
    }
    // 重新启动（这次开着自动发现：预设 claude 也会被发现）
    let (_base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = true").await;
    assert!(has_card(&rt, "keep"));
    assert_eq!(rt.registry.group_ids(Role::Coding).unwrap(), vec!["keep"]);
    let claude: Vec<_> = rt
        .registry
        .cards()
        .into_iter()
        .filter(|c| c.id == "claude")
        .collect();
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].name, "托管的 Claude", "同 id 时应以托管配置为准");
}

/// RT-AGCFG-07：config.toml 手写的内容不能在对话里改
#[tokio::test]
async fn rt_agcfg_07_config_toml_wins() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("07");
    let extra = format!(
        "[acp]\nauto_detect = false\n{}[agents.assign]\ncoding = [\"hand\"]\n",
        toml_agent("hand")
    );
    let (base, rt) = sb.serve(NO_LLM, &extra).await;
    for body in [
        json!({"kind": "disable", "id": "hand"}),
        json!({"kind": "set_default", "role": "coding", "ids": ["hand"]}),
        custom("hand", "mock"),
    ] {
        let (status, out) = post(&base, "/api/agents/proposals", body.clone()).await;
        assert_eq!(status, 409, "{body} 应被拒绝：{out}");
        assert!(
            out["error"]
                .as_str()
                .is_some_and(|e| e.contains("config.toml")),
            "应说明要改 config.toml：{out}"
        );
    }
    assert!(has_card(&rt, "hand"));
}

/// RT-AGCFG-08：凭证缺失、错误、提议过期都不能写入
#[tokio::test]
async fn rt_agcfg_08_token_required() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("08");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    let (status, p) = post(&base, "/api/agents/proposals", custom("t1", "mock")).await;
    assert_eq!(status, 200, "{p}");
    let id = p["id"].as_str().unwrap().to_string();
    let apply = format!("/api/agents/proposals/{id}/apply");

    assert_eq!(post(&base, &apply, json!({})).await.0, 422, "缺少凭证");
    assert_eq!(post(&base, &apply, json!({"token": "wrong"})).await.0, 403);
    assert!(!has_card(&rt, "t1"));
    assert!(!sb.managed_file().exists(), "不应写出托管文件");

    rt.agent_config.set_proposal_ttl(Duration::from_millis(50));
    let (_, p2) = post(&base, "/api/agents/proposals", custom("t2", "mock")).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (status, _) = post(
        &base,
        &format!("/api/agents/proposals/{}/apply", p2["id"].as_str().unwrap()),
        json!({"token": p2["token"]}),
    )
    .await;
    assert_eq!(status, 410, "过期提议应拒绝");
    assert!(!has_card(&rt, "t2"));
}

/// RT-AGCFG-09：托管文件损坏时启动正常、报错、不覆盖坏文件
#[tokio::test]
async fn rt_agcfg_09_corrupted_file() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("09");
    let bad = "这不是 TOML [[[";
    std::fs::write(sb.managed_file(), bad).unwrap();
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    let err = rt.agent_config.load_error().expect("应报告托管文件错误");
    assert!(err.contains("E_AGENT_MANAGED_INVALID"), "{err}");
    let (status, out) = post(&base, "/api/agents/proposals", custom("x", "mock")).await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(std::fs::read_to_string(sb.managed_file()).unwrap(), bad);
}

/// RT-AGCFG-10：环境变量只能写引用，不能写明文值
#[tokio::test]
async fn rt_agcfg_10_env_reference_only() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("10");
    let (base, _rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    let plain = json!({"kind": "add_custom", "agent": {"id": "e1", "command": MOCK, "role": "coding", "env": {"API_KEY": "sk-secret-value"}}});
    let (status, out) = post(&base, "/api/agents/proposals", plain).await;
    assert_eq!(status, 422, "明文值应被拒绝：{out}");

    let reference = json!({"kind": "add_custom", "agent": {"id": "e2", "command": MOCK, "role": "coding", "env": {"API_KEY": "${MY_API_KEY}"}}});
    let (status, out) = propose_and_apply(&base, reference).await;
    assert_eq!(status, 200, "{out}");
    let text = std::fs::read_to_string(sb.managed_file()).unwrap();
    assert!(text.contains("${MY_API_KEY}") && !text.contains("sk-secret-value"));
}

/// RT-AGCFG-11：执行组里放不同类型的 agent 被拒绝，执行组不变
#[tokio::test]
async fn rt_agcfg_11_group_type_mismatch() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("11");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    assert_eq!(
        propose_and_apply(&base, custom("ta", "type-a")).await.0,
        200
    );
    assert_eq!(
        propose_and_apply(&base, custom("tb", "type-b")).await.0,
        200
    );
    let before = rt.registry.group_ids(Role::Coding).unwrap();
    let (status, out) = post(
        &base,
        "/api/agents/proposals",
        json!({"kind": "set_default", "role": "coding", "ids": ["ta", "tb"]}),
    )
    .await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(rt.registry.group_ids(Role::Coding).unwrap(), before);
}

/// RT-AGCFG-12：两个修改同时确认，都生效，文件完整
#[tokio::test]
async fn rt_agcfg_12_concurrent_applies() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("12");
    let (base, rt) = sb.serve(NO_LLM, "[acp]\nauto_detect = false").await;
    let (_, p1) = post(&base, "/api/agents/proposals", custom("c1", "mock")).await;
    let (_, p2) = post(&base, "/api/agents/proposals", custom("c2", "mock")).await;
    let apply = |p: Value| {
        let base = base.clone();
        async move {
            post(
                &base,
                &format!("/api/agents/proposals/{}/apply", p["id"].as_str().unwrap()),
                json!({"token": p["token"]}),
            )
            .await
            .0
        }
    };
    let (s1, s2) = tokio::join!(apply(p1), apply(p2));
    assert_eq!((s1, s2), (200, 200));
    assert!(has_card(&rt, "c1") && has_card(&rt, "c2"));
    let text = std::fs::read_to_string(sb.managed_file()).unwrap();
    let parsed: toml::Value = toml::from_str(&text).expect("托管文件应是合法 TOML");
    let ids: Vec<&str> = parsed["acp"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert!(ids.contains(&"c1") && ids.contains(&"c2"), "{text}");
}

// ───────────── RT-AGCFG-13 / 14：没有可用 coding agent 时由前台模型回复 ─────────────

/// 假模型：结构化请求按 schema 返回；普通对话请求记录下来并返回固定文字（或 500）
async fn mock_llm(reply_ok: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let plain: Arc<Mutex<Vec<Value>>> = Arc::default();
    let log = plain.clone();
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        route_post(move |Json(req): Json<Value>| {
            let log = log.clone();
            async move {
                let schema = req["response_format"]["json_schema"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let content = match schema.as_str() {
                    "turn_decision" => json!({"action":"delegate","question":"","task_goal":"修复 src/main.rs 的编译错误","task_id":"","follow_up_kind":"","change":""}).to_string(),
                    "route_decision" => json!({"task_class":"L2","execution_role":"coding","required_tools":[],"risk_level":"medium","reason":"改代码"}).to_string(),
                    _ => {
                        log.lock().unwrap().push(req.clone());
                        if !reply_ok {
                            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "down").into_response();
                        }
                        "现在没有能写代码的助手，所以这件事暂时做不了。".to_string()
                    }
                };
                let chunk = json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]});
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
    (format!("http://{addr}/v1"), plain)
}

/// 把一次请求的全部消息拼成文字（检查提示词里有没有要求的内容）
fn request_text(req: &Value) -> String {
    req["messages"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|x| x["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// RT-AGCFG-13：派发时没有可用 coding agent，由前台模型用大白话回复
#[tokio::test]
async fn rt_agcfg_13_no_agent_reply_by_front_model() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("13");
    let (llm, plain) = mock_llm(true).await;
    let (_base, rt) = sb.serve(&llm, "[acp]\nauto_detect = false").await;
    let conv = rt.store.create_conversation("测试").unwrap();
    let out = rt
        .conversation
        .handle_message(&conv.id, "修复 src/main.rs 的编译错误")
        .await
        .unwrap();
    assert!(out.task.is_none());
    // 回复由前台模型生成；错误码只允许出现在末尾的括号里（与 e2e node_flow 的"如实报告错误码"要求一致）
    let content = out.reply.content.as_str();
    assert!(
        content.starts_with("现在没有能写代码的助手，所以这件事暂时做不了。"),
        "回复应由前台模型生成：{content}"
    );
    let code_at = content.find("E_ROUTE_NO_AGENT").expect("末尾应附错误码");
    assert!(
        content[code_at..].trim_end().ends_with('）'),
        "错误码应放在最后的括号里：{content}"
    );
    let reqs = plain.lock().unwrap().clone();
    let text = reqs.iter().map(request_text).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("非专业"),
        "应要求用非专业用户能看懂的话：{text}"
    );
    assert!(text.contains("coding") || text.contains("写代码"), "{text}");

    // 模型不可用时的兜底文字：不以错误码开头
    let sb2 = Sandbox::new("13b");
    let (llm2, _) = mock_llm(false).await;
    let (_b2, rt2) = sb2.serve(&llm2, "[acp]\nauto_detect = false").await;
    let conv2 = rt2.store.create_conversation("测试").unwrap();
    let out2 = rt2
        .conversation
        .handle_message(&conv2.id, "修复 src/main.rs 的编译错误")
        .await
        .unwrap();
    let reply = out2.reply.content.trim_start();
    assert!(
        !reply.starts_with('[') && !reply.starts_with("这个任务暂时无法派发"),
        "兜底文字应先用大白话说明：{reply}"
    );
}

/// RT-AGCFG-14：执行阶段因没有可用 coding agent 失败，交付消息由前台模型用大白话撰写
#[tokio::test]
async fn rt_agcfg_14_failed_delivery_plain_language() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("14");
    let (llm, plain) = mock_llm(true).await;
    let (_base, rt) = sb.serve(&llm, "[acp]\nauto_detect = false").await;
    let conv = rt.store.create_conversation("测试").unwrap();
    let route = RouteDecision {
        task_class: TaskClass::L2,
        execution_role: Role::Coding,
        required_tools: vec![],
        risk_level: Risk::Medium,
        reason: "测试".into(),

        template_id: String::new(),
        deliverable: String::new(),
        slots_ok: String::new(),
    };
    let policy = PolicyVerdict {
        auto_execute: true,
        needs_review: false,
        notes: vec![],
        candidates: vec![],
    };
    let task = rt
        .orchestrator
        .submit(&conv.id, "修复编译错误", "", route, policy)
        .unwrap();
    // 等交付消息
    let mut delivered = None;
    for _ in 0..100 {
        let msgs = rt.store.list_messages(&conv.id, None).unwrap();
        if let Some(m) = msgs
            .into_iter()
            .find(|m| m.role == "assistant" && m.task_id.as_deref() == Some(task.id.as_str()))
        {
            delivered = Some(m);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(delivered.is_some(), "应有交付消息");
    let reqs = plain.lock().unwrap().clone();
    let text = reqs.iter().map(request_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("非专业"), "应要求用大白话：{text}");
    assert!(
        text.contains("停用") || text.contains("没有安装"),
        "材料里应有原因：{text}"
    );
    assert!(
        text.contains("恢复") || text.contains("接入"),
        "材料里应有恢复办法：{text}"
    );
}
