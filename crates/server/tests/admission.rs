//! RT-ADM-14：准入 HTTP 接口端到端（用例见 docs/testing/CASES.md）。

use std::sync::Arc;
use std::time::Duration;

use an_core::{PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskStatus};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn config(admission: bool) -> AppConfig {
    let mut text = String::from(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n",
    );
    if admission {
        text.push_str("[admission]\nenabled = true\n");
    }
    toml::from_str(&text).unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }
}

fn claim(id: &str) -> Value {
    json!({
        "agent_id": id, "name": "外部质量 agent", "description": "质量分析",
        "capabilities": {"read_context": true},
        "requested_level": "trusted", "max_subtask_depth": null, "working_language": "zh"
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_adm_14_admission_http_end_to_end() {
    // 桩外部 agent
    let ext = serve(Router::new().route(
        "/run",
        post(|| async { Json(json!({"status": "succeeded", "summary": "ext 完成"})) }),
    ))
    .await;

    let dir = std::env::temp_dir().join(an_core::new_id("an-adm-http"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(true), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };

    // S1：提交 Card
    let (s, v) = api.post("/api/admission/agents", claim("ext")).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["questions"], json!([]));
    // 顺序错误 → 409；未知 agent → 404
    let (s, _) = api
        .post(
            "/api/admission/agents/ext/score",
            json!({"completeness": 1.0, "conversation": 1.0, "test": 1.0}),
        )
        .await;
    assert_eq!(s, 409);
    let (s, _) = api
        .post(
            "/api/admission/agents/nobody/clarify",
            json!({"answers": []}),
        )
        .await;
    assert_eq!(s, 404);
    // S2–S5
    let (s, _) = api
        .post("/api/admission/agents/ext/clarify", json!({"answers": []}))
        .await;
    assert_eq!(s, 200);
    let (s, v) = api
        .post(
            "/api/admission/agents/ext/score",
            json!({"completeness": 1.0, "conversation": 1.0, "test": 1.0}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["total"], 100.0);
    assert_eq!(v["recommended"], "trusted");
    let (s, _) = api
        .post(
            "/api/admission/agents/ext/contract",
            json!({"role": "executor", "working_language": "zh", "context_scope": [], "allowed_tools": []}),
        )
        .await;
    assert_eq!(s, 200);
    let binding =
        json!({"endpoint": ext, "role": "general", "agent_type": "ext", "max_concurrent": 1});
    // Trusted 未审批不能激活
    let (s, _) = api
        .post("/api/admission/agents/ext/activate", binding.clone())
        .await;
    assert_eq!(s, 409);
    let (s, _) = api
        .post(
            "/api/admission/agents/ext/approve",
            json!({"approver": "supervisor"}),
        )
        .await;
    assert_eq!(s, 200);
    let (s, v) = api
        .post("/api/admission/agents/ext/activate", binding)
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["epoch"], 1);
    // 非法分数 → 400
    api.post("/api/admission/agents", claim("ext2")).await;
    api.post("/api/admission/agents/ext2/clarify", json!({"answers": []}))
        .await;
    let (s, _) = api
        .post(
            "/api/admission/agents/ext2/score",
            json!({"completeness": 1.5, "conversation": 1.0, "test": 1.0}),
        )
        .await;
    assert_eq!(s, 400);

    // 激活后出现在 /api/agents，并能完成任务
    let (_, agents) = api.get("/api/agents").await;
    let card = agents
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "ext")
        .cloned()
        .expect("外部 agent 应已注册");
    assert_eq!(card["kind"], "a2a");
    let conv = rt.store.create_conversation("t").unwrap().id;
    let task = rt
        .orchestrator
        .submit(
            &conv,
            "分析 CPK",
            "",
            RouteDecision {
                task_class: TaskClass::L1,
                execution_role: Role::General,
                required_tools: vec![],
                risk_level: Risk::Low,
                reason: String::new(),
            },
            PolicyVerdict {
                auto_execute: true,
                needs_review: false,
                notes: vec![],
                candidates: vec![],
            },
        )
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let done = loop {
        let t = rt.store.get_task(&task.id).unwrap().unwrap();
        if matches!(t.status, TaskStatus::Completed | TaskStatus::Failed) {
            break t;
        }
        assert!(std::time::Instant::now() < deadline, "任务未结束");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(done.status, TaskStatus::Completed, "{:?}", done.result);
    assert!(done.result.unwrap().summary.contains("ext 完成"));
    let (s, v) = api.get("/api/admission/agents/ext").await;
    assert_eq!(s, 200);
    assert_eq!(v["record"]["probation"]["tasks_completed"], 1);
    assert!(!v["events"].as_array().unwrap().is_empty());
    let (s, v) = api.get("/api/admission/agents").await;
    assert_eq!(s, 200);
    assert!(v.as_array().unwrap().iter().any(|r| r["agent_id"] == "ext"));

    // 重启（同一数据库）：已激活的外部 agent 自动重新注册
    let rt2 = Runtime::new(config(true), Store::open(&db).unwrap()).unwrap();
    assert!(rt2
        .registry
        .cards()
        .iter()
        .any(|c| c.id == "ext" && c.kind == "a2a"));
    drop(rt2);

    // 撤权
    let (s, v) = api
        .post(
            "/api/admission/agents/ext/revoke",
            json!({"reason": "测试"}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["epoch"], 2);
    assert_eq!(v["aborted"], json!([]));
    let (s, _) = api
        .post(
            "/api/admission/agents/ext/revoke",
            json!({"reason": "再次"}),
        )
        .await;
    assert_eq!(s, 409);

    // 未开启准入：接口返回 404
    let off = Arc::new(
        Runtime::new(
            config(false),
            Store::open(std::path::Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api_off = Api {
        base: serve(an_server::api(off)).await,
        http: reqwest::Client::new(),
    };
    assert_eq!(api_off.get("/api/admission/agents").await.0, 404);
    assert_eq!(
        api_off.post("/api/admission/agents", claim("x")).await.0,
        404
    );
    let _ = std::fs::remove_dir_all(&dir);
}
