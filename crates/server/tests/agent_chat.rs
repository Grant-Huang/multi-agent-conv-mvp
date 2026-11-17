//! 用例 RT-AGCHAT-01 ~ 07、09（定义见 docs/testing/CASES.md「对话里管理助手」）：
//! 前台判断新增 manage_agents；向导由代码生成卡片；卡片存进消息；送给模型的历史不含卡片数据。
//!
//! 通过 HTTP 驱动（发消息、读消息、确认提议），模型换成按脚本回答并记录请求的假服务。
//! 需要"已安装 claude-agent-acp"的用例会把 PATH、HOME 指向临时目录（进程级全局变量），
//! 所以本文件所有用例用同一把锁串行执行。本文件是独立测试二进制。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post as route_post;
use axum::Json;
use serde_json::{json, Value};

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const MOCK: &str = env!("CARGO_BIN_EXE_an-mock-acp");

/// 前台判断脚本：(用户最新消息, 本 schema 第几次请求) → turn_decision 的 JSON
type TurnScript = Arc<dyn Fn(&str, usize) -> Value + Send + Sync>;

struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start(turn: TurnScript) -> Self {
        let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = requests.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            route_post(move |Json(req): Json<Value>| {
                let log = log.clone();
                let turn = turn.clone();
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let idx = {
                        let mut l = log.lock().unwrap();
                        let idx = l
                            .iter()
                            .filter(|r| {
                                r["response_format"]["json_schema"]["name"] == schema.as_str()
                            })
                            .count();
                        l.push(req.clone());
                        idx
                    };
                    let last = req["messages"]
                        .as_array()
                        .and_then(|m| m.iter().rev().find(|x| x["role"] == "user"))
                        .and_then(|x| x["content"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let content = match schema.as_str() {
                        "turn_decision" => turn(&last, idx).to_string(),
                        _ => "好的。".to_string(),
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

    fn turn_requests(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"] == "turn_decision")
            .cloned()
            .collect()
    }

    fn all_text(&self) -> String {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// manage_agents 判断
fn manage(op: &str, id: &str, role: &str) -> Value {
    json!({"action":"manage_agents","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":"",
           "agent_op":op,"agent_id":id,"agent_role":role})
}

/// 用例独享的临时目录。PATH / HOME 在整条用例期间都指向这里（检测"是否已安装"发生在处理请求时），
/// 用例结束（Drop）时恢复；用例之间由 ENV_LOCK 串行。
struct Sandbox {
    root: PathBuf,
    old_path: Option<std::ffi::OsString>,
    old_home: Option<std::ffi::OsString>,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(an_core::new_id(&format!("agchat-{name}")));
        for d in ["bin", "home", "data", "files", "code"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let p = root.join("bin").join("claude-agent-acp");
        std::fs::write(&p, format!("#!/bin/sh\nexec \"{MOCK}\" \"$@\"\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old_path = std::env::var_os("PATH");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("PATH", root.join("bin"));
        std::env::set_var("HOME", root.join("home"));
        Self {
            root,
            old_path,
            old_home,
        }
    }

    async fn serve(&self, llm: &str, management: bool) -> (String, Arc<Runtime>) {
        let text = format!(
            r#"
[server]
db_path = "{data}/agentnexus.db"
[llm]
base_url = "{llm}"
model = "m"
max_retries = 1
timeout_secs = 5
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
workdir = "{code}"
[acp]
auto_detect = false
[[acp.agents]]
id = "hand"
command = "{MOCK}"
role = "reviewer"
[conversation]
agent_management = {management}
"#,
            data = self.root.join("data").display(),
            files = self.root.join("files").display(),
            code = self.root.join("code").display(),
        );
        let (config, _) = AppConfig::parse(&text).unwrap();
        let rt =
            Arc::new(Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap());
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
        restore("PATH", self.old_path.take());
        restore("HOME", self.old_home.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn restore(key: &str, old: Option<std::ffi::OsString>) {
    match old {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

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

async fn get(base: &str, path: &str) -> Value {
    reqwest::get(format!("{base}{path}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap_or(Value::Null)
}

/// 新建会话并发一条消息，返回 (会话 id, 发送后的全部消息)
async fn say(base: &str, conv: Option<&str>, text: &str) -> (String, Vec<Value>) {
    let cid = match conv {
        Some(c) => c.to_string(),
        None => post(base, "/api/conversations", json!({})).await.1["id"]
            .as_str()
            .unwrap()
            .to_string(),
    };
    let (status, out) = post(
        base,
        &format!("/api/conversations/{cid}/messages"),
        json!({"text": text}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let msgs = get(base, &format!("/api/conversations/{cid}/messages")).await;
    (cid, msgs.as_array().cloned().unwrap_or_default())
}

fn last_assistant(msgs: &[Value]) -> Value {
    msgs.iter()
        .rev()
        .find(|m| m["role"] == "assistant")
        .cloned()
        .unwrap_or(Value::Null)
}

/// RT-AGCHAT-01：开关关闭时，前台判断的 schema 与现在相同
#[tokio::test]
async fn rt_agchat_01_off_by_default_unchanged() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("01");
    let llm = MockLlm::start(Arc::new(|_, _| {
        json!({"action":"respond","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""})
    }))
    .await;
    let (base, _rt) = sb.serve(&llm.base, false).await;
    say(&base, None, "你好").await;
    let req = &llm.turn_requests()[0];
    let schema = &req["response_format"]["json_schema"]["schema"];
    let actions = schema["properties"]["action"]["enum"].to_string();
    assert!(!actions.contains("manage_agents"), "{actions}");
    assert!(schema["properties"].get("agent_op").is_none());
}

/// RT-AGCHAT-02：说"接入 claude"→ 确认卡；正文是大白话，不含凭证
#[tokio::test]
async fn rt_agchat_02_connect_makes_confirm_card() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("02");
    let llm = MockLlm::start(Arc::new(|_, _| manage("connect", "claude", ""))).await;
    let (base, _rt) = sb.serve(&llm.base, true).await;
    let (_, msgs) = say(&base, None, "把 Claude Code 接进来").await;
    let reply = last_assistant(&msgs);
    let card = &reply["card"];
    assert_eq!(card["type"], "confirm", "应回复确认卡：{reply}");
    assert!(card["proposal_id"].as_str().is_some_and(|s| !s.is_empty()));
    let token = card["token"].as_str().unwrap_or("");
    assert!(!token.is_empty(), "确认卡应带凭证");
    let content = reply["content"].as_str().unwrap();
    assert!(content.contains("确认"), "正文应提示用户确认：{content}");
    assert!(!content.contains(token), "正文不能含凭证");
}

/// RT-AGCHAT-03：没说是哪个 → 选择卡（只列写代码、未接入的）；点选后追加确认卡
#[tokio::test]
async fn rt_agchat_03_choice_card_then_confirm() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("03");
    let llm = MockLlm::start(Arc::new(|_, _| manage("connect", "", "coding"))).await;
    let (base, _rt) = sb.serve(&llm.base, true).await;
    let (cid, msgs) = say(&base, None, "帮我接个写代码的助手").await;
    let card = last_assistant(&msgs)["card"].clone();
    assert_eq!(card["type"], "choice", "{card}");
    let options = card["options"].as_array().cloned().unwrap_or_default();
    assert!(options.iter().any(|o| o["id"] == "claude"), "{card}");
    assert!(
        !options.iter().any(|o| o["id"] == "hermes"),
        "不应列出非写代码的助手：{card}"
    );
    assert!(options.iter().all(|o| o["role"] == "coding"), "{card}");

    let (status, out) = post(
        &base,
        &format!("/api/conversations/{cid}/agent-proposals"),
        json!({"kind": "add_preset", "id": "claude"}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let msgs = get(&base, &format!("/api/conversations/{cid}/messages")).await;
    assert_eq!(
        last_assistant(msgs.as_array().unwrap())["card"]["type"],
        "confirm"
    );
}

/// RT-AGCHAT-04：要停用 config.toml 里的 agent → 大白话拒绝，没有卡片
#[tokio::test]
async fn rt_agchat_04_refusal_is_plain_text() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("04");
    let llm = MockLlm::start(Arc::new(|_, _| manage("disable", "hand", ""))).await;
    let (base, rt) = sb.serve(&llm.base, true).await;
    let (_, msgs) = say(&base, None, "把 hand 停掉").await;
    let reply = last_assistant(&msgs);
    assert!(reply["card"].is_null(), "{reply}");
    assert!(
        reply["content"].as_str().unwrap().contains("config.toml"),
        "{reply}"
    );
    assert!(rt.registry.cards().iter().any(|c| c.id == "hand"));
}

/// RT-AGCHAT-05：送给模型的历史不含凭证和卡片数据
#[tokio::test]
async fn rt_agchat_05_history_excludes_card_data() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("05");
    let llm = MockLlm::start(Arc::new(|text, _| {
        if text.contains("接进来") {
            manage("connect", "claude", "")
        } else {
            json!({"action":"respond","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""})
        }
    }))
    .await;
    let (base, _rt) = sb.serve(&llm.base, true).await;
    let (cid, msgs) = say(&base, None, "把 Claude Code 接进来").await;
    let reply = last_assistant(&msgs);
    let token = reply["card"]["token"].as_str().unwrap().to_string();
    let summary = reply["content"].as_str().unwrap().to_string();
    say(&base, Some(&cid), "好的谢谢").await;
    let sent = llm.all_text();
    assert!(!sent.contains(&token), "模型请求里不能出现凭证");
    assert!(!sent.contains("proposal_id"), "模型请求里不能出现卡片数据");
    let last_turn = llm.turn_requests().last().unwrap().to_string();
    assert!(
        last_turn.contains(summary.lines().next().unwrap()),
        "历史里应有卡片消息的大白话正文"
    );
}

/// RT-AGCHAT-06：确认后追加结果卡，原确认卡变为 applied
#[tokio::test]
async fn rt_agchat_06_apply_appends_result_card() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("06");
    let llm = MockLlm::start(Arc::new(|_, _| manage("connect", "claude", ""))).await;
    let (base, rt) = sb.serve(&llm.base, true).await;
    let (cid, msgs) = say(&base, None, "把 Claude Code 接进来").await;
    let confirm = last_assistant(&msgs);
    let card = &confirm["card"];
    let (status, out) = post(
        &base,
        &format!(
            "/api/agents/proposals/{}/apply",
            card["proposal_id"].as_str().unwrap()
        ),
        json!({"token": card["token"]}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert!(rt.registry.cards().iter().any(|c| c.id == "claude"));
    let msgs = get(&base, &format!("/api/conversations/{cid}/messages")).await;
    let msgs = msgs.as_array().unwrap();
    let result = last_assistant(msgs);
    assert_eq!(result["card"]["type"], "result", "{result}");
    let original = msgs.iter().find(|m| m["id"] == confirm["id"]).unwrap();
    assert_eq!(original["card"]["state"], "applied", "{original}");
}

/// RT-AGCHAT-07：过期后确认 → 410，确认卡变为 expired
#[tokio::test]
async fn rt_agchat_07_expired_card() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("07");
    let llm = MockLlm::start(Arc::new(|_, _| manage("connect", "claude", ""))).await;
    let (base, rt) = sb.serve(&llm.base, true).await;
    rt.agent_config.set_proposal_ttl(Duration::from_millis(50));
    let (cid, msgs) = say(&base, None, "把 Claude Code 接进来").await;
    let confirm = last_assistant(&msgs);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (status, _) = post(
        &base,
        &format!(
            "/api/agents/proposals/{}/apply",
            confirm["card"]["proposal_id"].as_str().unwrap()
        ),
        json!({"token": confirm["card"]["token"]}),
    )
    .await;
    assert_eq!(status, 410);
    let msgs = get(&base, &format!("/api/conversations/{cid}/messages")).await;
    let original = msgs
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == confirm["id"])
        .cloned()
        .unwrap();
    assert_eq!(original["card"]["state"], "expired", "{original}");
}

/// RT-AGCHAT-09：agent_id 不在候选里 → 按格式不合格重试
#[tokio::test]
async fn rt_agchat_09_unknown_agent_id_retries() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("09");
    let llm = MockLlm::start(Arc::new(|_, idx| {
        if idx == 0 {
            manage("connect", "no-such-agent", "")
        } else {
            manage("connect", "claude", "")
        }
    }))
    .await;
    let (base, _rt) = sb.serve(&llm.base, true).await;
    let (_, msgs) = say(&base, None, "把 Claude Code 接进来").await;
    assert_eq!(llm.turn_requests().len(), 2, "应重试一次");
    assert_eq!(last_assistant(&msgs)["card"]["type"], "confirm");
}
