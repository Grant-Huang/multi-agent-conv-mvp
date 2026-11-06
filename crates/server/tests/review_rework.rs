//! 端到端：质检与返工（DESIGN 7.6）。
//!
//! 覆盖：审查不通过 → 带问题清单返工 → 再审通过 → 完成；
//! 返工用完仍不通过 → 失败（不无限循环）；`max_review_rounds = 0` 时首次不通过即失败。
//! 假 LLM 按调用次数返回审查结论，coding 用 sh 脚本冒充 codex。

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

/// 假 LLM：总是委派 coding；审查结论由 `pass_from` 决定
/// （第 N 次审查起才通过；`usize::MAX` 表示永远不通过）。
async fn start_mock_llm(pass_from: usize, reviews: Arc<AtomicUsize>) -> String {
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let reviews = reviews.clone();
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("");
                    let msgs = req["messages"].as_array().unwrap();
                    let last_user = msgs
                        .iter()
                        .rev()
                        .filter(|m| m["role"] == "user")
                        .filter_map(|m| m["content"].as_str())
                        .next()
                        .unwrap_or("");
                    let content = match schema {
                        "turn_decision" => json!({"action":"delegate","question":"","task_goal":last_user,"task_id":"","follow_up_kind":"","change":""}).to_string(),
                        "route_decision" => json!({"task_class":"L2","execution_role":"coding","required_tools":[],"risk_level":"medium","reason":"测试路由"}).to_string(),
                        "review_verdict" => {
                            let n = reviews.fetch_add(1, Ordering::SeqCst) + 1;
                            if n >= pass_from {
                                json!({"verdict":"pass","issues":[],"comment":"符合要求"}).to_string()
                            } else {
                                json!({"verdict":"fail","issues":["缺少测试"],"comment":"需要补测试"}).to_string()
                            }
                        }
                        _ => "【交付】已处理。".into(),
                    };
                    if req["stream"] == true {
                        let body = format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices": [{"delta": {"content": content}}]})
                        );
                        return ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], body)
                            .into_response();
                    }
                    Json(json!({"choices": [{"message": {"role": "assistant", "content": content}}]}))
                        .into_response()
                }
            }),
        )
        .route(
            "/v1/models",
            get(|| async { Json(json!({"data": [{"id": "mock-model"}]})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/v1")
}

struct Env {
    base: String,
    http: reqwest::Client,
    reviews: Arc<AtomicUsize>,
}

async fn setup(pass_from: usize, max_rounds: Option<u32>) -> Env {
    let reviews = Arc::new(AtomicUsize::new(0));
    let llm_url = start_mock_llm(pass_from, reviews.clone()).await;
    let tmp = std::env::temp_dir().join(an_core::new_id("an-rework"));
    let code_dir = tmp.join("code");
    std::fs::create_dir_all(&code_dir).unwrap();
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
    let orchestrator = match max_rounds {
        Some(n) => format!("[orchestrator]\nmax_review_rounds = {n}\n"),
        None => String::new(),
    };
    let config_toml = format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"
max_retries = 1

{orchestrator}
[agents.general]
workspace_dir = "{ws}"

[agents.coding]
command = "sh"
args = ["-c", "echo 改代码; echo hello >> main.txt; echo '已修改 main.txt' > \"$0\"", "{{last_message_file}}", "{{prompt}}"]
workdir = "{code}"
timeout_secs = 60
"#,
        ws = tmp.join("files").display(),
        code = code_dir.display(),
    );
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
        reviews,
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
    async fn run_task(&self) -> Value {
        let conv = self.post("/api/conversations", json!({})).await;
        let cid = conv["id"].as_str().unwrap();
        let out = self
            .post(
                &format!("/api/conversations/{cid}/messages"),
                json!({"text": "帮我改代码，加一个文件"}),
            )
            .await;
        assert_eq!(out["task"]["role"], "coding", "{out}");
        let tid = out["task"]["id"].as_str().unwrap().to_string();
        for _ in 0..100 {
            let d = self.get(&format!("/api/tasks/{tid}")).await;
            if ["COMPLETED", "FAILED", "NEEDS_HUMAN"]
                .contains(&d["task"]["status"].as_str().unwrap_or(""))
            {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "任务 {tid} 未结束：{}",
            self.get(&format!("/api/tasks/{tid}")).await
        );
    }
}

fn statuses(d: &Value) -> Vec<String> {
    d["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["status"].as_str().map(String::from))
        .collect()
}

/// 第一次审查不通过 → 返工一轮（回到 QUEUED 再执行）→ 第二次审查通过 → 完成
#[tokio::test]
async fn review_fail_then_rework_then_pass() {
    let env = setup(2, None).await;
    let d = env.run_task().await;
    assert_eq!(d["task"]["status"], "COMPLETED", "{d}");
    assert_eq!(d["task"]["review"]["verdict"], "pass");
    assert_eq!(env.reviews.load(Ordering::SeqCst), 2, "应审查两次");
    assert_eq!(
        statuses(&d),
        [
            "CREATED",
            "QUEUED",
            "RUNNING",
            "VERIFYING",
            "QUEUED",
            "RUNNING",
            "VERIFYING",
            "COMPLETED"
        ]
    );
}

/// 返工用完（默认 1 轮）仍不通过 → 失败，不无限循环
#[tokio::test]
async fn rework_exhausted_marks_failed() {
    let env = setup(usize::MAX, None).await;
    let d = env.run_task().await;
    assert_eq!(d["task"]["status"], "FAILED", "{d}");
    assert_eq!(d["task"]["review"]["verdict"], "fail");
    assert_eq!(
        env.reviews.load(Ordering::SeqCst),
        2,
        "1 轮返工 = 共审查 2 次"
    );
}

/// `max_review_rounds = 0`：首次不通过直接失败，不返工
#[tokio::test]
async fn zero_rework_rounds_fails_immediately() {
    let env = setup(usize::MAX, Some(0)).await;
    let d = env.run_task().await;
    assert_eq!(d["task"]["status"], "FAILED", "{d}");
    assert_eq!(env.reviews.load(Ordering::SeqCst), 1);
    assert_eq!(
        statuses(&d),
        ["CREATED", "QUEUED", "RUNNING", "VERIFYING", "FAILED"]
    );
}
