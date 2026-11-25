//! RT-ADLAUD（T-99）：关联修正审计上云 HTTP。用例见 docs/testing/CASES.md。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn config() -> AppConfig {
    toml::from_str(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n",
    )
    .unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn boot() -> (Api, Arc<Runtime>) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-link-aud"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(Runtime::new(config(), Store::open(&dir.join("an.db")).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (api, rt)
}

async fn boot_at(db: &std::path::Path) -> Api {
    let rt = Arc::new(Runtime::new(config(), Store::open(db).unwrap()).unwrap());
    Api {
        base: serve(an_server::api(rt)).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    }
}

/// RT-ADLAUD-01：追加/列表回环（服务端 id；created_at 升序；字段一致）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlaud_01_append_and_list_roundtrip() {
    let (api, _rt) = boot().await;
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-AUD-A/link-audits",
            json!({ "source": "workspace", "action": "link" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(
        v["audit"]["id"].as_str().unwrap_or_default().len() > 0,
        "{v}"
    );
    assert_eq!(v["audit"]["task_id"], "TASK-AUD-A", "{v}");
    assert_eq!(v["audit"]["action"], "link", "{v}");
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-AUD-A/link-audits",
            json!({ "source": "workspace", "action": "revoke" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["audit"]["action"], "revoke", "{v}");

    let (s, g) = api
        .get("/api/desktop/v1/tasks/TASK-AUD-A/link-audits")
        .await;
    assert_eq!(s, 200, "{g}");
    let items = g["items"].as_array().expect("items 数组");
    assert_eq!(items.len(), 2, "{g}");
    assert_eq!(items[0]["action"], "link", "{g}");
    assert_eq!(items[1]["action"], "revoke", "{g}");
    assert!(items.iter().all(|i| i["task_id"] == "TASK-AUD-A"), "{g}");
}

/// RT-ADLAUD-02：按任务过滤——各任务只含自己的审计。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlaud_02_filter_by_task() {
    let (api, _rt) = boot().await;
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-B-AUD/link-audits",
            json!({ "source": "workspace", "action": "link" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-B-AUD/link-audits",
            json!({ "source": "workspace", "action": "revoke" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-C-AUD/link-audits",
            json!({ "source": "workspace", "action": "revoke" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");

    let (s, g) = api
        .get("/api/desktop/v1/tasks/TASK-B-AUD/link-audits")
        .await;
    assert_eq!(s, 200, "{g}");
    let items = g["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "TASK-B-AUD 应恰有 2 条: {g}");
    assert!(items.iter().all(|i| i["task_id"] == "TASK-B-AUD"), "{g}");
}

/// RT-ADLAUD-03：action 非法值拒绝。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlaud_03_invalid_action_rejected() {
    let (api, _rt) = boot().await;
    let (s, v) = api
        .post(
            "/api/desktop/v1/tasks/TASK-AUD-A/link-audits",
            json!({ "source": "workspace", "action": "其他值" }),
        )
        .await;
    assert_eq!(s, 400, "{v}");
    assert_eq!(v["code"], "invalid_request", "{v}");
}

/// RT-ADLAUD-04：同库重启持久 + 跨库隔离（H4 同族探针）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlaud_04_restart_persists_and_cross_db_isolated() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-link-aud-p5"));
    std::fs::create_dir_all(&dir).unwrap();
    let db1 = dir.join("a.db");
    let db2 = dir.join("b.db");
    {
        let api = boot_at(&db1).await;
        let (s, v) = api
            .post(
                "/api/desktop/v1/tasks/TASK-P/link-audits",
                json!({ "source": "workspace", "action": "link" }),
            )
            .await;
        assert_eq!(s, 200, "{v}");
    }
    // 同库重开：审计保留
    let api_r = boot_at(&db1).await;
    let (s, g) = api_r.get("/api/desktop/v1/tasks/TASK-P/link-audits").await;
    assert_eq!(s, 200, "{g}");
    assert_eq!(
        g["items"].as_array().unwrap().len(),
        1,
        "重启后审计丢失: {g}"
    );
    // 全新库：空（用户事件不播种、跨库不泄漏）
    let api_f = boot_at(&db2).await;
    let (s, g) = api_f.get("/api/desktop/v1/tasks/TASK-P/link-audits").await;
    assert_eq!(s, 200, "{g}");
    assert_eq!(
        g["items"].as_array().unwrap().len(),
        0,
        "全新库不应有其他库的审计: {g}"
    );
}
