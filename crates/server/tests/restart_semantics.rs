//! RT-RESTART-01～06：重启语义（WP7 / G08 / GS6 重启分支）。用例见 docs/testing/CASES.md。
//!
//! RT-RESTART-01/02 为 WP7 新行为：中断任务重启后置 FAILED 且有原因事件 + 失败交付。
//! RT-RESTART-03～05 为既有跨重启人工出口的基线锁定：待验收 / 返工额度 / 待批准。
//! RT-RESTART-06 锁定终态任务不受重启影响。
//!
//! 重启手法沿用仓库惯例（desktop_replay::rt_adreplay_04）：flush → drop 旧 Runtime →
//! 同一 `db_path` 重新 `Runtime::new` + 挂新 API。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use an_core::{EventKind, PolicyVerdict, Role, RouteDecision, TaskClass, TaskRecord, TaskStatus};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// 假 LLM：委派 coding；审查首次 needs_human，之后按 `pass_after_rework`（同 review_verdict.rs）。
async fn start_mock_llm(pass_after_rework: bool, reviews: Arc<AtomicUsize>) -> String {
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
                            if n == 1 {
                                json!({"verdict":"needs_human","issues":["需人工确认边界"],"comment":"请人工验收"}).to_string()
                            } else if pass_after_rework {
                                json!({"verdict":"pass","issues":[],"comment":"返工后符合要求"}).to_string()
                            } else {
                                json!({"verdict":"needs_human","issues":["仍需人工"],"comment":"再次待验收"}).to_string()
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

fn min_config_toml() -> String {
    "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
     [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n\
     [agents.reviewer]\nenabled = false\n"
        .into()
}

fn min_config() -> AppConfig {
    toml::from_str(&min_config_toml()).unwrap()
}

fn flow_config_toml(llm_url: &str, ws: &str, code: &str, rounds: u32, reviewer: bool) -> String {
    format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"
max_retries = 1

[orchestrator]
max_review_rounds = {rounds}

[agents.general]
workspace_dir = "{ws}"

[agents.coding]
command = "sh"
args = ["-c", "echo 改代码; echo hello >> main.txt; echo '已修改 main.txt' > \"$0\"", "{{last_message_file}}", "{{prompt}}"]
workdir = "{code}"
timeout_secs = 60

[agents.reviewer]
enabled = {reviewer}
"#,
        llm_url = llm_url,
        ws = ws,
        code = code,
        rounds = rounds,
        reviewer = reviewer,
    )
}

struct Env {
    base: String,
    http: reqwest::Client,
    rt: Arc<Runtime>,
    conv_id: String,
    db: PathBuf,
    config_toml: String,
}

async fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

/// 模拟进程重启：flush → drop 旧实例 → 同库 `Runtime::new` → 挂新 API。
async fn restart(env: Env) -> Env {
    env.rt.store.flush_events();
    let config_toml = env.config_toml.clone();
    let db = env.db.clone();
    let conv_id = env.conv_id.clone();
    let http = env.http.clone();
    drop(env);
    let rt = Arc::new(
        Runtime::new(
            toml::from_str::<AppConfig>(&config_toml).unwrap(),
            Store::open(&db).unwrap(),
        )
        .unwrap(),
    );
    let base = serve(an_server::api(rt.clone())).await;
    Env {
        base,
        http,
        rt,
        conv_id,
        db,
        config_toml,
    }
}

async fn boot_min() -> Env {
    let dir = std::env::temp_dir().join(an_core::new_id("an-restart"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let config_toml = min_config_toml();
    let rt = Arc::new(Runtime::new(min_config(), Store::open(&db).unwrap()).unwrap());
    let conv = rt.store.create_conversation("重启语义测试").unwrap();
    let base = serve(an_server::api(rt.clone())).await;
    Env {
        base,
        http: http_client().await,
        rt,
        conv_id: conv.id,
        db,
        config_toml,
    }
}

async fn boot_flow(rounds: u32, pass_after_rework: bool, reviewer: bool) -> Env {
    let reviews = Arc::new(AtomicUsize::new(0));
    let llm_url = start_mock_llm(pass_after_rework, reviews).await;
    let tmp = std::env::temp_dir().join(an_core::new_id("an-restart-flow"));
    let ws = tmp.join("files");
    let code = tmp.join("code");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&code).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&code)
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
    let config_toml = flow_config_toml(
        &llm_url,
        &ws.display().to_string(),
        &code.display().to_string(),
        rounds,
        reviewer,
    );
    let db = tmp.join("an.db");
    let rt = Arc::new(
        Runtime::new(
            toml::from_str::<AppConfig>(&config_toml).unwrap(),
            Store::open(&db).unwrap(),
        )
        .unwrap(),
    );
    let conv = rt.store.create_conversation("重启语义流程测试").unwrap();
    let base = serve(an_server::api(rt.clone())).await;
    Env {
        base,
        http: http_client().await,
        rt,
        conv_id: conv.id,
        db,
        config_toml,
    }
}

fn seed_task(rt: &Runtime, conv: &str, id: &str, status: TaskStatus, role: Role) {
    let t = TaskRecord {
        id: id.into(),
        conversation_id: conv.into(),
        goal: "重启语义验证任务".into(),
        role,
        status,
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: role,
            required_tools: vec![],
            risk_level: an_core::Risk::Low,
            reason: "test".into(),
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
        result: None,
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: an_core::now(),
        updated_at: an_core::now(),
    };
    rt.store.insert_task(&t, "").unwrap();
}

impl Env {
    async fn post_raw(&self, path: &str, body: Value) -> (reqwest::StatusCode, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(json!({}));
        (status, v)
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let (status, v) = self.post_raw(path, body).await;
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

    /// 跑到审查 needs_human（待验收：有 result）
    async fn await_acceptance(&self) -> String {
        let out = self
            .post(
                &format!("/api/conversations/{}/messages", self.conv_id),
                json!({"text": "帮我改代码，加一个文件"}),
            )
            .await;
        assert_eq!(out["task"]["role"], "coding", "{out}");
        let tid = out["task"]["id"].as_str().unwrap().to_string();
        let status = self.wait_status(&tid, &["NEEDS_HUMAN"], 20_000).await;
        assert_eq!(status, "NEEDS_HUMAN");
        let d = self.get(&format!("/api/tasks/{tid}")).await;
        assert!(!d["task"]["result"].is_null(), "待验收须有 result：{d}");
        tid
    }

    /// 轮询任务状态直到进入 `want`；提前落到其他终态则失败。
    async fn wait_status(&self, tid: &str, want: &[&str], timeout_ms: u64) -> String {
        let start = std::time::Instant::now();
        loop {
            let d = self.get(&format!("/api/tasks/{tid}")).await;
            let s = d["task"]["status"].as_str().unwrap_or("").to_string();
            if want.contains(&s.as_str()) {
                return s;
            }
            if matches!(s.as_str(), "COMPLETED" | "FAILED" | "CANCELLED") {
                panic!("任务 {tid} 提前结束为 {s}，期望 {want:?}：{d}");
            }
            if start.elapsed() > Duration::from_millis(timeout_ms) {
                panic!(
                    "任务 {tid} 未在期限内变为 {want:?}：{}",
                    self.get(&format!("/api/tasks/{tid}")).await
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// RT-RESTART-01：四类中断任务重启后均 FAILED，且每任务有含「重启中断」原因的事件（WP7 新行为）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_01_interrupted_tasks_marked_failed_with_reason_event() {
    let e = boot_min().await;
    let cid = e.conv_id.clone();
    let cases = [
        ("t-created", TaskStatus::Created),
        ("t-queued", TaskStatus::Queued),
        ("t-running", TaskStatus::Running),
        ("t-verifying", TaskStatus::Verifying),
    ];
    for (id, status) in &cases {
        seed_task(&e.rt, &cid, id, *status, Role::General);
    }
    let e = restart(e).await;
    for (id, _) in &cases {
        let t = e.rt.store.get_task(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Failed, "任务 {id} 重启后应为 FAILED");
        let events = e.rt.store.list_events(id).unwrap();
        let hit = events.iter().any(|ev| {
            ev.kind == EventKind::StatusChanged
                && ev.status == Some(TaskStatus::Failed)
                && ev.message.contains("重启中断")
        });
        assert!(
            hit,
            "任务 {id} 缺少含「重启中断」原因的 StatusChanged 事件：{events:?}"
        );
    }
}

/// RT-RESTART-02：中断任务的会话收到绑定 task_id 的失败交付（WP7 新行为）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_02_interrupted_tasks_get_failure_delivery() {
    let e = boot_min().await;
    let cid = e.conv_id.clone();
    seed_task(&e.rt, &cid, "t-run", TaskStatus::Running, Role::General);
    let e = restart(e).await;
    let msgs = e.rt.store.list_messages(&cid, None).unwrap();
    let hit = msgs.iter().any(|m| {
        m.role == "assistant"
            && m.task_id.as_deref() == Some("t-run")
            && m.content.contains("任务因服务重启中断，可重新委派")
    });
    assert!(hit, "会话 {cid} 缺少 t-run 的失败交付消息：{msgs:?}");
}

/// RT-RESTART-03：待验收跨重启后 accept → COMPLETED，事件含 note，交付照常（基线锁定）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_03_verdict_accept_after_restart_completes() {
    let e = boot_flow(1, true, true).await;
    let tid = e.await_acceptance().await;
    let e = restart(e).await;
    let note = "重启后验收通过";
    let (status, body) = e
        .post_raw(
            &format!("/api/tasks/{tid}/review-verdict"),
            json!({"verdict": "accept", "note": note}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    let final_status = e.wait_status(&tid, &["COMPLETED"], 20_000).await;
    assert_eq!(final_status, "COMPLETED");
    let events = e.rt.store.list_events(&tid).unwrap();
    assert!(
        events.iter().any(|ev| ev.message.contains(note)),
        "事件须含人工 note「{note}」：{events:?}"
    );
    let msgs = e.rt.store.list_messages(&e.conv_id, None).unwrap();
    assert!(
        msgs.iter()
            .any(|m| m.role == "assistant" && m.task_id.as_deref() == Some(tid.as_str())),
        "验收通过后应有绑定任务的交付消息：{msgs:?}"
    );
}

/// RT-RESTART-04：待验收跨重启后 reject → 返工（同会话）→ 再待验收 → 再 reject → FAILED（基线锁定）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_04_verdict_reject_after_restart_reworks_then_fails() {
    let e = boot_flow(1, false, true).await;
    let tid = e.await_acceptance().await;
    let e = restart(e).await;
    let (status, body) = e
        .post_raw(
            &format!("/api/tasks/{tid}/review-verdict"),
            json!({"verdict": "reject", "note": "边界不对，返工"}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    // 返工后再次进入待验收（mock 第二轮审查仍 needs_human）
    e.wait_status(&tid, &["NEEDS_HUMAN"], 30_000).await;
    // 返工额度（max_review_rounds=1）已用完：二次 reject → FAILED
    let (status, body) = e
        .post_raw(
            &format!("/api/tasks/{tid}/review-verdict"),
            json!({"verdict": "reject", "note": "仍不通过"}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    let final_status = e.wait_status(&tid, &["FAILED"], 20_000).await;
    assert_eq!(final_status, "FAILED");
    let t = e.rt.store.get_task(&tid).unwrap().unwrap();
    assert_eq!(t.conversation_id, e.conv_id, "返工不得串到其他会话");
}

/// RT-RESTART-05：待批准（NEEDS_HUMAN 无 result）跨重启后 approve → 继续执行至终态（基线锁定）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_05_awaiting_approval_survives_and_approves_after_restart() {
    let e = boot_flow(1, true, false).await;
    let cid = e.conv_id.clone();
    // 预置用户消息：批准后的 coding 执行需要 last_message_file 上下文
    e.rt.store
        .add_message(&cid, "user", "帮我改代码，加一个文件", None)
        .unwrap();
    seed_task(
        &e.rt,
        &cid,
        "t-approve",
        TaskStatus::NeedsHuman,
        Role::Coding,
    );
    let e = restart(e).await;
    let (status, body) = e.post_raw("/api/tasks/t-approve/approve", json!({})).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    let final_status = e.wait_status("t-approve", &["COMPLETED"], 30_000).await;
    assert_eq!(final_status, "COMPLETED");
}

/// RT-RESTART-06：终态任务重启后状态与 updated_at 不变，无新增重启事件（基线锁定）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_restart_06_terminal_tasks_untouched_by_restart() {
    let e = boot_min().await;
    let cid = e.conv_id.clone();
    let cases = [
        ("t-done", TaskStatus::Completed),
        ("t-failed", TaskStatus::Failed),
        ("t-cancelled", TaskStatus::Cancelled),
    ];
    for (id, status) in &cases {
        seed_task(&e.rt, &cid, id, *status, Role::General);
    }
    let before: Vec<(String, String)> = cases
        .iter()
        .map(|(id, _)| {
            let t = e.rt.store.get_task(id).unwrap().unwrap();
            (t.updated_at.clone(), t.status.as_str().to_string())
        })
        .collect();
    let e = restart(e).await;
    for ((id, _), (updated_at, status)) in cases.iter().zip(&before) {
        let t = e.rt.store.get_task(id).unwrap().unwrap();
        assert_eq!(
            t.status.as_str(),
            status.as_str(),
            "终态任务 {id} 状态不得改变"
        );
        assert_eq!(
            t.updated_at,
            updated_at.as_str(),
            "终态任务 {id} updated_at 不得改变"
        );
        let events = e.rt.store.list_events(id).unwrap();
        assert!(
            !events.iter().any(|ev| ev.message.contains("重启中断")),
            "终态任务 {id} 不应有重启事件：{events:?}"
        );
    }
}
