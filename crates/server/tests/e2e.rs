//! 端到端测试：用一个"假的 OpenAI 兼容服务"代替 llama.cpp，跑通完整流程。
//!
//! 覆盖：直接回答、格式错误重试、委派 General（工具调用 + 写文件 + 审查）、
//! Coding Agent 子进程（用 sh 脚本冒充 codex）+ git diff 证据、高风险审批、
//! 中途取消（follow_up）、SSE 事件流、路由日志。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

// ───────────────────────── 假 LLM 服务 ─────────────────────────

/// 根据请求里的 schema 名称和消息内容，返回预设的回复
fn mock_reply(req: &Value, format_calls: &AtomicUsize) -> String {
    let schema = req["response_format"]["json_schema"]["name"]
        .as_str()
        .unwrap_or("");
    let msgs = req["messages"].as_array().unwrap();
    let all_text: String = msgs
        .iter()
        .map(|m| m["content"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    // 最后一条"真正的"用户消息（跳过重试时追加的格式错误反馈）
    let last_user = msgs
        .iter()
        .rev()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .find(|c| !c.starts_with("上面的输出不合格"))
        .unwrap_or("");
    let system = msgs[0]["content"].as_str().unwrap_or("");

    match schema {
        "turn_decision" => {
            // 第一步只输出动作，不含回答内容
            let empty = json!({"action":"","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""});
            let mut d = empty;
            if last_user.contains("格式测试") {
                // 第一次故意输出非法内容，测试重试
                if format_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return "我觉得应该直接回答".into();
                }
                d["action"] = "respond".into();
            } else if last_user.contains("取消") {
                // 从 system prompt 的"进行中的任务"里找 task_id
                let id = system
                    .split("task_id=")
                    .nth(1)
                    .and_then(|s| s.split('，').next())
                    .unwrap_or("");
                d["action"] = "follow_up".into();
                d["task_id"] = id.into();
                d["follow_up_kind"] = "cancel".into();
            } else if last_user.contains("问题一") || last_user.contains("你好") {
                // 两种问候/问题都直接回答（原来是两个内容相同的分支，clippy::if_same_then_else 报错）
                d["action"] = "respond".into();
            } else if last_user.contains("验收") {
                // 没看到纠正时故意派错；提示词里有「正确做法：respond」才改对
                if system.contains("正确做法：respond") {
                    d["action"] = "respond".into();
                } else {
                    d["action"] = "delegate".into();
                    d["task_goal"] = last_user.into();
                }
            } else {
                d["action"] = "delegate".into();
                d["task_goal"] = last_user.into();
            }
            d.to_string()
        }
        "route_decision" => {
            // 只看"任务目标："这一行，忽略对话背景
            let goal = last_user.lines().next().unwrap_or("");
            let (role, class, tools, risk) = if goal.contains("代码") {
                ("coding", "L2", json!([]), "medium")
            } else if goal.contains("删除") {
                ("general", "L2", json!(["list_files"]), "high")
            } else {
                ("general", "L4", json!(["write_file"]), "low")
            };
            json!({"task_class": class, "execution_role": role, "required_tools": tools, "risk_level": risk, "reason": "测试路由"}).to_string()
        }
        "general_step" => {
            if all_text.contains("工具 write_file 的返回结果")
                || all_text.contains("工具 list_files 的返回结果")
            {
                // 重启场景测试：执行者是否收到了重启前的对话背景（context）
                let answer = if all_text.contains("用户：删除旧数据") {
                    "CONTEXT_OK"
                } else {
                    "报告已写入 reports/r.md"
                };
                json!({"thought":"完成","action":"final","tool":"","args":{},"answer":answer})
                    .to_string()
            } else if all_text.contains("删除") {
                json!({"thought":"先看看","action":"tool_call","tool":"list_files","args":{"path":""},"answer":""}).to_string()
            } else {
                json!({"thought":"写报告","action":"tool_call","tool":"write_file","args":{"path":"reports/r.md","content":"# 报告\n内容"},"answer":""}).to_string()
            }
        }
        "review_verdict" => json!({"verdict":"pass","issues":[],"comment":"符合要求"}).to_string(),
        // 没有 response_format：直接回答（第二步）或 Composer 生成交付消息
        _ if system.contains("直接回答用户的最新消息") => {
            if last_user.contains("问题一") && system.contains("按编号逐条回答") {
                "1. MES 是制造执行系统。\n2. TCP 面向连接，UDP 不面向连接。".into()
            } else if last_user.contains("格式测试") {
                "格式修正后的回答".into()
            } else {
                "你好！有什么可以帮你？".into()
            }
        }
        _ => "【交付】任务已完成，详见产物。".into(),
    }
}

async fn start_mock_llm() -> String {
    let format_calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let fc = format_calls.clone();
                async move {
                    let content = mock_reply(&req, &fc);
                    if req["stream"] == true {
                        // 流式：先发一段思考内容（应被过滤），再把回答拆成几块发送
                        let mut pieces = vec!["<think>".to_string(), "先想一想".to_string(), "</think>\n".to_string()];
                        let chars: Vec<char> = content.chars().collect();
                        pieces.extend(chars.chunks(3).map(|c| c.iter().collect::<String>()));
                        let mut body = String::new();
                        for p in pieces {
                            body += &format!("data: {}\n\n", json!({"choices": [{"delta": {"content": p}}]}));
                        }
                        body += "data: [DONE]\n\n";
                        return ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], body).into_response();
                    }
                    Json(json!({"choices": [{"message": {"role": "assistant", "content": content}}]})).into_response()
                }
            }),
        )
        .route("/v1/models", get(|| async { Json(json!({"data": [{"id": "mock-model"}]})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/v1")
}

// ───────────────────────── 测试辅助 ─────────────────────────

struct Env {
    base: String,
    http: reqwest::Client,
    code_dir: std::path::PathBuf,
}

/// 测试场景
#[derive(PartialEq)]
enum Mode {
    /// 用 sh 脚本冒充 codex
    Codex,
    /// codex 不可用；coding 执行组明确指定为模拟 ACP agent
    Acp,
    /// coding 执行组指定为一个启动即崩溃的 ACP agent：应如实报错，不能换给别的 agent
    AcpBroken,
}

async fn setup(mode: Mode) -> Env {
    let llm_url = start_mock_llm().await;
    let tmp = std::env::temp_dir().join(an_core::new_id("an-e2e"));
    let code_dir = tmp.join("code");
    std::fs::create_dir_all(&code_dir).unwrap();
    // 代码目录初始化为 git 仓库，用于验证 diff 证据
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&code_dir)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);

    // 用 sh 脚本冒充 codex：$0=最后回复文件，$1=任务描述；描述里含"慢"时睡 30 秒（用于测试取消）
    let config_toml = format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"
max_retries = 2

[agents.general]
workspace_dir = "{ws}"

[agents.coding]
command = "sh"
args = ["-c", "case \"$1\" in *慢*) sleep 30;; esac; echo 正在修改代码; echo hello > main.txt; echo '已添加 main.txt' > \"$0\"", "{{last_message_file}}", "{{prompt}}"]
workdir = "{code}"
timeout_secs = 60
"#,
        ws = tmp.join("files").display(),
        code = code_dir.display(),
    );
    let config_toml = if mode != Mode::Codex {
        // codex 命令改成不存在的；broken 握手前就退出
        let assign = if mode == Mode::Acp { "mock" } else { "broken" };
        config_toml.replace("command = \"sh\"", "command = \"no-such-codex-cmd\"")
            + &format!(
                r#"
[agents.assign]
coding = "{assign}"

[acp]
auto_detect = false

[[acp.agents]]
id = "broken"
command = "sh"
args = ["-c", "exit 3"]
role = "coding"

[[acp.agents]]
id = "mock"
name = "Mock ACP"
command = "{mock}"
role = "coding"
"#,
                mock = env!("CARGO_BIN_EXE_an-mock-acp")
            )
    } else {
        config_toml
    };
    let config: AppConfig = toml::from_str(&config_toml).unwrap();
    let rt = Arc::new(
        Runtime::new(
            config,
            Store::open(std::path::Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt)).await.unwrap() });
    Env {
        base: format!("http://{addr}"),
        http: reqwest::Client::new(),
        code_dir,
    }
}

impl Env {
    async fn post(&self, path: &str, body: Value) -> Value {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let v: Value = r.json().await.unwrap();
        assert!(status.is_success(), "POST {path} 失败：{status} {v}");
        v
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    /// 轮询直到任务状态满足条件（测试里用轮询即可）
    async fn wait_status(&self, task_id: &str, want: &[&str]) -> Value {
        for _ in 0..100 {
            let d = self.get(&format!("/api/tasks/{task_id}")).await;
            if want.contains(&d["task"]["status"].as_str().unwrap_or("")) {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "任务 {task_id} 未达到状态 {want:?}：{}",
            self.get(&format!("/api/tasks/{task_id}")).await
        );
    }
}

// ───────────────────────── 测试 ─────────────────────────

#[tokio::test]
async fn full_flow() {
    let env = setup(Mode::Codex).await;

    // 健康检查
    let h = env.get("/api/health").await;
    assert_eq!(h["llm"]["ok"], true);
    assert_eq!(h["backends"][0]["ok"], true);
    assert!(h["backends"][0]["calls"]["total"].as_u64().is_some());
    // 内置 general / reviewer + codex；测试环境不会自动发现预设之外的 agent
    assert!(h["agents"].as_array().unwrap().len() >= 3);

    let conv = env.post("/api/conversations", json!({})).await;
    let cid = conv["id"].as_str().unwrap().to_string();

    // SSE：先订阅
    let mut sse = env
        .http
        .get(format!("{}/api/events?conversation_id={cid}", env.base))
        .send()
        .await
        .unwrap();

    // 1. 直接回答：判断和回答分两步；回答流式推送到界面，思考段落被过滤
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "你好"}),
        )
        .await;
    assert_eq!(out["decision"]["action"], "respond");
    assert_eq!(out["reply"]["content"], "你好！有什么可以帮你？");
    let reply_id = out["reply"]["id"].as_str().unwrap().to_string();
    let mut streamed = String::new();
    while !streamed.contains("有什么可以帮你") {
        let chunk = tokio::time::timeout(Duration::from_secs(2), sse.chunk())
            .await
            .expect("等待流式片段超时")
            .unwrap()
            .unwrap();
        for line in String::from_utf8_lossy(&chunk).lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let ev: Value = serde_json::from_str(data.trim()).unwrap();
                if ev["kind"] == "assistant_delta" {
                    assert_eq!(
                        ev["stream_id"],
                        reply_id.as_str(),
                        "片段的 stream_id 应等于最终消息 id"
                    );
                    streamed += ev["message"].as_str().unwrap();
                }
            }
        }
    }
    assert_eq!(streamed, "你好！有什么可以帮你？", "思考段落不应推送到界面");

    // 2. 格式错误 → 重试成功
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "格式测试"}),
        )
        .await;
    assert_eq!(out["reply"]["content"], "格式修正后的回答");
    let logs = env.get("/api/route-logs").await;
    assert_eq!(logs[0]["attempts"], 2, "应该重试过一次：{}", logs[0]);

    // 3. 委派 General：工具调用写文件 → Reviewer 审查 → 完成 → 交付消息
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "帮我写一份周报"}),
        )
        .await;
    assert_eq!(out["decision"]["action"], "delegate");
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    assert_eq!(d["task"]["result"]["artifacts"][0]["name"], "reports/r.md");
    assert_eq!(d["task"]["review"]["verdict"], "pass"); // L4 任务需要审查
    let statuses: Vec<_> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["status"].as_str())
        .collect();
    assert_eq!(
        statuses,
        ["CREATED", "QUEUED", "RUNNING", "VERIFYING", "COMPLETED"]
    );
    tokio::time::sleep(Duration::from_millis(200)).await; // 等 Composer 写入交付消息
    let msgs = env.get(&format!("/api/conversations/{cid}/messages")).await;
    let last = msgs.as_array().unwrap().last().unwrap();
    assert_eq!(last["content"], "【交付】任务已完成，详见产物。");
    assert_eq!(last["task_id"], tid.as_str());

    // SSE 应该收到事件
    let chunk = tokio::time::timeout(Duration::from_secs(2), sse.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&chunk).contains("task_event"));

    // 4. Coding Agent：子进程 + git diff 证据 + 审查
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "帮我改代码，加一个文件"}),
        )
        .await;
    assert_eq!(out["task"]["role"], "coding");
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    assert_eq!(d["task"]["result"]["summary"], "已添加 main.txt");
    assert!(d["task"]["result"]["artifacts"][0]["content"]
        .as_str()
        .unwrap()
        .contains("main.txt"));
    assert!(env.code_dir.join("main.txt").exists());

    // 5. 高风险 → 等待审批 → 批准后执行
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "删除旧数据"}),
        )
        .await;
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    assert_eq!(out["task"]["status"], "NEEDS_HUMAN");
    env.post(&format!("/api/tasks/{tid}/approve"), json!({}))
        .await;
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");

    // 6. 中途取消：慢任务执行中，用户说"取消"
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "改代码，这个会很慢"}),
        )
        .await;
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    env.wait_status(&tid, &["RUNNING"]).await;
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "算了，取消吧"}),
        )
        .await;
    assert_eq!(out["decision"]["action"], "follow_up", "{out}");
    let d = env.wait_status(&tid, &["CANCELLED"]).await;
    assert_eq!(d["task"]["status"], "CANCELLED");

    // 7. 路由日志：每轮对话一条
    let logs = env.get("/api/route-logs").await;
    assert_eq!(logs.as_array().unwrap().len(), 7);
}

/// ACP 接入：模拟 ACP agent → 权限请求 → 写文件 → git diff 证据 → 审查 → 完成；以及执行中取消。
#[tokio::test]
async fn acp_flow() {
    let env = setup(Mode::Acp).await;

    // 卡片：codex 不可用，broken 和 mock 都已注册
    let agents = env.get("/api/agents").await;
    let card = |id: &str| {
        agents
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .cloned()
            .unwrap()
    };
    assert_eq!(card("codex")["enabled"], false);
    assert_eq!(card("mock")["kind"], "acp");

    let conv = env.post("/api/conversations", json!({})).await;
    let cid = conv["id"].as_str().unwrap().to_string();

    // 1. 完整流程
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "帮我改代码，加一个文件"}),
        )
        .await;
    assert_eq!(out["task"]["role"], "coding", "{out}");
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    let result = &d["task"]["result"];
    assert_eq!(result["summary"], "已创建 hello.txt（来自模拟 ACP agent）");
    let ev = |kind: &str| {
        result["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .map(|e| e["content"].as_str().unwrap().to_string())
    };
    assert_eq!(ev("executed_by").unwrap(), "Mock ACP（acp，本机）");
    assert!(ev("permissions").unwrap().contains("允许"));
    assert_eq!(ev("acp_stop_reason").unwrap(), "正常结束");
    assert!(ev("git_diff_stat").unwrap().contains("hello.txt"));
    assert!(env.code_dir.join("hello.txt").exists());
    assert_eq!(d["task"]["review"]["verdict"], "pass");
    let msgs: Vec<String> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap().to_string())
        .collect();
    assert!(msgs.iter().any(|m| m.contains("权限请求")), "{msgs:?}");
    assert!(
        !msgs.iter().any(|m| m.contains("broken")),
        "不应使用未指定的 agent：{msgs:?}"
    );

    // 2. 执行中取消：子进程应被杀掉，状态为 CANCELLED
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "改代码，这个会很慢"}),
        )
        .await;
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    env.wait_status(&tid, &["RUNNING"]).await;
    tokio::time::sleep(Duration::from_millis(300)).await; // 等 agent 进程启动
    env.post(&format!("/api/tasks/{tid}/cancel"), json!({}))
        .await;
    let d = env.wait_status(&tid, &["CANCELLED"]).await;
    assert_eq!(d["task"]["status"], "CANCELLED");
}

/// 执行组指定的 agent 启动失败：如实报错（带错误码），不会悄悄换给别的 agent
#[tokio::test]
async fn acp_error_is_reported() {
    let env = setup(Mode::AcpBroken).await;
    let conv = env.post("/api/conversations", json!({})).await;
    let cid = conv["id"].as_str().unwrap().to_string();
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "帮我改代码"}),
        )
        .await;
    let tid = out["task"]["id"].as_str().unwrap().to_string();
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "FAILED", "{d}");
    let errors = d["task"]["result"]["errors"].to_string();
    assert!(
        errors.contains("E_ACP_HANDSHAKE"),
        "应带握手失败的错误码：{errors}"
    );
    assert!(
        !env.code_dir.join("hello.txt").exists(),
        "mock 不在执行组里，不能被使用"
    );
}

// ───────────────────────── 节点 ─────────────────────────

/// 在本进程中启动一个节点（两个同类型的模拟 agent，各自并发 1）
async fn start_node(
    workdir: &std::path::Path,
    state_dir: &std::path::Path,
) -> (Arc<an_node::Node>, String) {
    let mock = env!("CARGO_BIN_EXE_an-mock-acp");
    let cfg: an_node::NodeConfig = toml::from_str(&format!(
        r#"
name = "test-node"
workdir = "{w}"
state_dir = "{s}"
advertise = false

[acp]
auto_detect = false

[[acp.agents]]
id = "mock"
command = "{mock}"
role = "coding"

[[acp.agents]]
id = "mock2"
command = "{mock}"
role = "coding"
"#,
        w = workdir.display(),
        s = state_dir.display()
    ))
    .unwrap();
    let node = an_node::Node::new(&cfg, an_observe::diagnostics_only(), Box::new(|_| {})).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let router = node.router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (node, addr)
}

/// 节点：认证、配对、通过节点执行、执行组分担并发与排队、取消后名额归还、解除配对
#[tokio::test]
async fn node_flow() {
    let tmp = std::env::temp_dir().join(an_core::new_id("an-node"));
    let node_ws = tmp.join("ws");
    std::fs::create_dir_all(&node_ws).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&node_ws)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);
    let (node, node_addr) = start_node(&node_ws, &tmp.join("state")).await;

    // AgentNexus：coding 执行组 = 节点上的两个同类 agent；本机 codex 不可用
    let llm_url = start_mock_llm().await;
    let config: AppConfig = toml::from_str(&format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"

[agents.general]
workspace_dir = "{ws}"

[agents.coding]
command = "no-such-codex-cmd"

[agents.assign]
coding = ["mock@test-node", "mock2@test-node"]

[acp]
auto_detect = false

[nodes]
discovery = false
"#,
        ws = tmp.join("files").display()
    ))
    .unwrap();
    let rt = Arc::new(
        Runtime::new(
            config,
            Store::open(std::path::Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt)).await.unwrap() });
    let env = Env {
        base,
        http: reqwest::Client::new(),
        code_dir: node_ws.clone(),
    };

    // 1. 未配对的请求被拒绝
    let r = env
        .http
        .get(format!("{node_addr}/node/agents"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], "E_NODE_AUTH");

    // 2. 配对前 coding 没有可用 agent
    let diag = env.get("/api/diagnostics").await;
    assert_eq!(diag["groups"]["coding"]["enabled"], false, "{diag}");

    // 3. 错误的配对码被拒绝
    let r = env
        .http
        .post(format!("{}/api/nodes/pair", env.base))
        .json(&json!({"address": node_addr, "code": "000000x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    assert!(r.text().await.unwrap().contains("E_NODE_PAIR"));

    // 4. 正确配对
    let paired = env
        .post(
            "/api/nodes/pair",
            json!({"address": node_addr, "code": node.pairing_code()}),
        )
        .await;
    assert_eq!(paired["name"], "test-node");
    assert_eq!(paired["online"], true);
    let diag = env.get("/api/diagnostics").await;
    assert_eq!(
        diag["groups"]["coding"]["members"],
        json!(["mock@test-node", "mock2@test-node"])
    );
    assert_eq!(diag["groups"]["coding"]["enabled"], true);

    let conv = env.post("/api/conversations", json!({})).await;
    let cid = conv["id"].as_str().unwrap().to_string();
    let send = |text: &'static str| {
        let env = &env;
        let cid = cid.clone();
        async move {
            env.post(
                &format!("/api/conversations/{cid}/messages"),
                json!({"text": text}),
            )
            .await["task"]["id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };
    let evidence = |d: &Value, kind: &str| {
        d["task"]["result"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .map(|e| e["content"].as_str().unwrap().to_string())
            .unwrap_or_default()
    };

    // 5. 通过节点执行：改动发生在节点的工作目录，git diff 在节点上收集
    let tid = send("帮我改代码").await;
    let d = env.wait_status(&tid, &["COMPLETED", "FAILED"]).await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    assert!(evidence(&d, "executed_by").contains("test-node"), "{d}");
    assert!(evidence(&d, "git_diff_stat").contains("hello.txt"));
    assert!(node_ws.join("hello.txt").exists());

    // 6. 并发：3 个任务、2 个 agent（各并发 1）→ 两个同时执行，第三个排队，最终全部完成
    let ids = [
        send("改代码 稍慢 1").await,
        send("改代码 稍慢 2").await,
        send("改代码 稍慢 3").await,
    ];
    let mut used = std::collections::BTreeSet::new();
    let mut queued = false;
    for id in &ids {
        let d = env.wait_status(id, &["COMPLETED", "FAILED"]).await;
        assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
        used.insert(evidence(&d, "executed_by"));
        queued |= d["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["message"].as_str().unwrap().contains("排队等待"));
    }
    assert_eq!(used.len(), 2, "两个备份都应被用上：{used:?}");
    assert!(queued, "第三个任务应排队等待");

    // 7. 取消经由节点执行的任务；之后两个 agent 的名额都应归还
    let tid = send("改代码，这个会很慢").await;
    env.wait_status(&tid, &["RUNNING"]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    env.post(&format!("/api/tasks/{tid}/cancel"), json!({}))
        .await;
    env.wait_status(&tid, &["CANCELLED"]).await;
    tokio::time::sleep(Duration::from_millis(300)).await; // 等节点检测到断开
    let ids = [send("改代码 稍慢 a").await, send("改代码 稍慢 b").await];
    for id in &ids {
        let d = env.wait_status(id, &["COMPLETED", "FAILED"]).await;
        assert_eq!(d["task"]["status"], "COMPLETED", "取消后名额应归还：{d}");
    }

    // 8. 解除配对后，coding 没有可用 agent，路由时如实报告
    let node_id = paired["node_id"].as_str().unwrap();
    let r = env
        .http
        .delete(format!("{}/api/nodes/{node_id}", env.base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let out = env
        .post(
            &format!("/api/conversations/{cid}/messages"),
            json!({"text": "帮我改代码"}),
        )
        .await;
    assert!(
        out["reply"]["content"]
            .as_str()
            .unwrap()
            .contains("E_ROUTE_NO_AGENT"),
        "{out}"
    );
}

/// 局域网发现：节点广播 → AgentNexus 发现。需要支持组播的网络环境，默认不运行：
/// `cargo test -p an-server --test e2e mdns_discovery -- --ignored`
#[tokio::test]
#[ignore]
async fn mdns_discovery() {
    let tmp = std::env::temp_dir().join(an_core::new_id("an-mdns"));
    let (node, addr) = start_node(&tmp.join("ws"), &tmp.join("state")).await;
    let port: u16 = addr.rsplit(':').next().unwrap().parse().unwrap();
    let _daemon = node.advertise(port).expect("广播失败");

    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let registry = an_runtime::registry::AgentRegistry::new(Default::default());
    let mgr = an_runtime::nodes::NodeManager::new(store, registry, "test".into());
    mgr.start_discovery();
    for _ in 0..50 {
        if let Some(found) = mgr
            .discovered()
            .into_iter()
            .find(|n| n.node_id == node.info.node_id)
        {
            assert_eq!(found.name, "test-node");
            assert!(found.address.ends_with(&format!(":{port}")), "{found:?}");
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("10 秒内没有发现节点");
}

/// 用同一个 SQLite 文件先后启动两个 Runtime（模拟服务重启）
async fn serve_runtime(db: &std::path::Path, config_toml: &str) -> (String, Arc<Runtime>) {
    let config: AppConfig = toml::from_str(config_toml).unwrap();
    let rt = Arc::new(Runtime::new(config, Store::open(db).unwrap()).unwrap());
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

/// 执行上下文持久化：高风险任务等审批时服务重启，批准后执行者仍能拿到重启前的对话背景。
/// （对应 docs/DESIGN.md 已知限制第 2 条的修复：context 存 tasks 表，不再依赖内存）
#[tokio::test]
async fn context_survives_restart() {
    let llm_url = start_mock_llm().await;
    let tmp = std::env::temp_dir().join(an_core::new_id("an-ctx"));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = tmp.join("state.db");
    let config_toml = format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"

[agents.general]
workspace_dir = "{ws}"
"#,
        ws = tmp.join("files").display()
    );
    let http = reqwest::Client::new();

    // 第一次启动：高风险任务（"删除旧数据"）→ NEEDS_HUMAN，未执行
    let (base_a, _) = serve_runtime(&db, &config_toml).await;
    let conv: Value = http
        .post(format!("{base_a}/api/conversations"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cid = conv["id"].as_str().unwrap().to_string();
    let out: Value = http
        .post(format!("{base_a}/api/conversations/{cid}/messages"))
        .json(&json!({"text": "删除旧数据"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(out["task"]["status"], "NEEDS_HUMAN", "{out}");
    let tid = out["task"]["id"].as_str().unwrap().to_string();

    // 第二次启动：内存全部丢失，从数据库恢复。批准后任务执行时应带着原始对话背景
    let (base_b, rt_b) = serve_runtime(&db, &config_toml).await;
    let r = http
        .post(format!("{base_b}/api/tasks/{tid}/approve"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let mut d = Value::Null;
    for _ in 0..100 {
        d = http
            .get(format!("{base_b}/api/tasks/{tid}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if ["COMPLETED", "FAILED"].contains(&d["task"]["status"].as_str().unwrap_or("")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    // mock 收到含"用户：删除旧数据"的上下文时回答 CONTEXT_OK，否则 CONTEXT 缺失
    assert_eq!(d["task"]["result"]["summary"], "CONTEXT_OK", "{d}");
    assert!(rt_b
        .store
        .get_task_context(&tid)
        .unwrap()
        .unwrap_or_default()
        .contains("删除旧数据"));
}

#[tokio::test]
async fn feedback_api_saves_and_rejects() {
    let config: AppConfig =
        toml::from_str("[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n").unwrap();
    let rt = Arc::new(
        Runtime::new(
            config,
            Store::open(std::path::Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let rt_keep = rt.clone();
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt)).await.unwrap() });
    let http = reqwest::Client::new();
    let base = format!("http://{addr}");

    let bad = http
        .post(format!("{base}/api/feedback"))
        .json(&json!({"message_id":"m1","original_action":"delegate","correct_action":"guess"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    let body: Value = bad.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert!(body["message"].as_str().unwrap_or("").contains("动作"));

    let ok = http
        .post(format!("{base}/api/feedback"))
        .json(&json!({
            "message_id": "m1",
            "original_action": "delegate",
            "original_role": "coding",
            "correct_action": "respond",
            "note": "只是在问概念"
        }))
        .send()
        .await
        .unwrap();
    assert!(ok.status().is_success());
    let body: Value = ok.json().await.unwrap();
    assert_eq!(body["status"], "success");
    let saved = rt_keep.store.list_feedback().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].message_id, "m1");
    assert_eq!(saved[0].correct_action, "respond");
    assert!(saved[0].correct_role.is_none());
}

#[tokio::test]
async fn ten_corrections_fix_similar_messages() {
    use an_core::{Message, TurnDecision};
    use an_runtime::store::RoutingFeedback;

    let pairs = [
        ("什么是 MES 系统", "验收：什么是 MES 系统"),
        ("解释 Rust 的所有权机制", "验收：解释 Rust 的所有权机制"),
        ("TCP 和 UDP 的区别", "验收：TCP 和 UDP 的区别"),
        (
            "Python 的 list 和 tuple 区别",
            "验收：Python 的 list 和 tuple 区别",
        ),
        ("什么是代码审查的最佳实践", "验收：什么是代码审查的最佳实践"),
        ("给我降低抛料率的建议", "验收：给我降低抛料率的建议"),
        ("写一个快速排序给我看看", "验收：写一个快速排序给我看看"),
        (
            "用 Rust 写个读取 CSV 的示例",
            "验收：用 Rust 写个读取 CSV 的示例",
        ),
        ("git 怎么回滚上一次提交", "验收：git 怎么回滚上一次提交"),
        (
            "calculator 工具支持哪些运算",
            "验收：calculator 工具支持哪些运算",
        ),
    ];
    let llm_url = start_mock_llm().await;
    let config: AppConfig = toml::from_str(&format!(
        "[llm]\nbase_url = \"{llm_url}\"\nmodel = \"mock\"\n"
    ))
    .unwrap();
    let rt = Arc::new(
        Runtime::new(
            config,
            Store::open(std::path::Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let conv = rt.store.create_conversation("验收").unwrap();

    let ask = |text: &str| {
        let history = vec![Message {
            id: "q".into(),
            conversation_id: conv.id.clone(),
            role: "user".into(),
            content: text.into(),
            task_id: None,
            created_at: an_core::now(),
        }];
        let rt = rt.clone();
        let text = text.to_string();
        async move {
            let d = rt
                .conversation
                .decide_turn(&history, &[])
                .await
                .unwrap_or_else(|e| panic!("{text}: {e}"));
            matches!(d.value, TurnDecision::Respond { .. })
        }
    };

    let mut before = 0;
    for (_, similar) in pairs {
        if ask(similar).await {
            before += 1;
        }
    }
    for (i, (orig, _)) in pairs.iter().enumerate() {
        let msg = rt.store.add_message(&conv.id, "user", orig, None).unwrap();
        rt.store
            .insert_feedback(&RoutingFeedback {
                id: format!("fb-{i}"),
                message_id: msg.id,
                original_action: "delegate".into(),
                original_role: Some("general".into()),
                correct_action: "respond".into(),
                correct_role: None,
                note: "这是概念题".into(),
                created_at: an_core::now(),
            })
            .unwrap();
    }
    let mut after = 0;
    for (_, similar) in pairs {
        if ask(similar).await {
            after += 1;
        }
    }
    assert_eq!(before, 0, "纠正前应全部误派");
    assert_eq!(after, pairs.len(), "纠正后相似消息应全部直接回答");
}

#[tokio::test]
async fn multi_question_reply_is_numbered() {
    let llm_url = start_mock_llm().await;
    let config: AppConfig = toml::from_str(&format!(
        "[llm]\nbase_url = \"{llm_url}\"\nmodel = \"mock\"\n"
    ))
    .unwrap();
    let rt = Runtime::new(
        config,
        Store::open(std::path::Path::new(":memory:")).unwrap(),
    )
    .unwrap();
    let conv = rt.store.create_conversation("多问题").unwrap();
    let out = rt
        .conversation
        .handle_message(&conv.id, "问题一：什么是 MES？问题二：TCP 和 UDP 的区别？")
        .await
        .unwrap();
    let text = out.reply.content;
    assert!(text.contains("1."), "{text}");
    assert!(text.contains("2."), "{text}");
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .count(),
        2
    );
}

/// 回归测试：模拟刷新页面 = HTTP 请求在回合中途断开。断开后回合仍应完成，助手消息落库。
async fn start_slow_mock_llm(delay: std::time::Duration) -> String {
    let format_calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let fc = format_calls.clone();
                async move {
                    tokio::time::sleep(delay).await;
                    let content = mock_reply(&req, &fc);
                    if req["stream"] == true {
                        let mut body = String::new();
                        for p in ["<think>", "想", "</think>\n"].iter().map(|s| s.to_string()).chain(
                            content.chars().collect::<Vec<char>>().chunks(3).map(|c| c.iter().collect::<String>()),
                        ) {
                            body += &format!("data: {}\n\n", json!({"choices": [{"delta": {"content": p}}]}));
                        }
                        body += "data: [DONE]\n\n";
                        return ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], body).into_response();
                    }
                    Json(json!({"choices": [{"message": {"role": "assistant", "content": content}}]})).into_response()
                }
            }),
        )
        .route("/v1/models", get(|| async { Json(json!({"data": [{"id": "mock-model"}]})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/v1")
}

#[tokio::test]
async fn turn_survives_client_disconnect() {
    let llm = start_slow_mock_llm(std::time::Duration::from_millis(1500)).await;
    let config: AppConfig = toml::from_str(&format!("[llm]\nbase_url = \"{llm}\"\nmodel = \"mock\"\n")).unwrap();
    let rt = Arc::new(Runtime::new(config, Store::open(std::path::Path::new(":memory:")).unwrap()).unwrap());
    let conv = rt.store.create_conversation("刷新测试").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let rt_serve = rt.clone();
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt_serve)).await.unwrap() });
    // 300ms 后客户端放弃请求，等价于浏览器刷新：连接被断开
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_millis(300)).build().unwrap();
    let res = http
        .post(format!("http://{addr}/api/conversations/{}/messages", conv.id))
        .json(&json!({"text": "什么是 MES 系统"}))
        .send()
        .await;
    assert!(res.is_err(), "客户端应该超时（模拟刷新）");
    tokio::time::sleep(std::time::Duration::from_millis(8000)).await;
    let msgs = rt.store.list_messages(&conv.id, None).unwrap();
    assert!(
        msgs.iter().any(|m| m.role == "assistant"),
        "断开后助手消息应仍被保存，实际消息：{:?}",
        msgs.iter().map(|m| (m.role.clone(), m.content.chars().take(20).collect::<String>())).collect::<Vec<_>>()
    );
}

/// 对照：同样的慢速 LLM，客户端不断开，应正常返回并保存助手消息。
#[tokio::test]
async fn turn_completes_without_disconnect() {
    let llm = start_slow_mock_llm(std::time::Duration::from_millis(1500)).await;
    let config: AppConfig = toml::from_str(&format!("[llm]\nbase_url = \"{llm}\"\nmodel = \"mock\"\n")).unwrap();
    let rt = Arc::new(Runtime::new(config, Store::open(std::path::Path::new(":memory:")).unwrap()).unwrap());
    let conv = rt.store.create_conversation("对照测试").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let rt_serve = rt.clone();
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt_serve)).await.unwrap() });
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60)).build().unwrap();
    let res = http
        .post(format!("http://{addr}/api/conversations/{}/messages", conv.id))
        .json(&json!({"text": "什么是 MES 系统"}))
        .send()
        .await
        .expect("不断开时请求应正常返回");
    assert!(res.status().is_success(), "状态码 {}", res.status());
    let msgs = rt.store.list_messages(&conv.id, None).unwrap();
    assert!(msgs.iter().any(|m| m.role == "assistant"), "对照组也应有助手消息");
}
