//! RT-ADXR：助理消息已读跨端夹具 HTTP。用例见 docs/testing/CASES.md。

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::{Mutex, MutexGuard};

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

fn read_test_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

async fn boot() -> (Api, MutexGuard<'static, ()>) {
    let guard = read_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-read"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt)).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    let (s, v) = api
        .post(
            "/api/desktop/v1/assistant/messages/fixture/reset",
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "reset: {v}");
    (api, guard)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_01_list_returns_unread_fixture() {
    let (api, _gate) = boot().await;
    let (s, v) = api.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s, 200, "{v}");
    let items = v["items"].as_array().expect("items array");
    assert!(items.len() >= 2, "{v}");
    let ids: Vec<&str> = items.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(ids.contains(&"asst-msg-1"), "{v}");
    assert!(ids.contains(&"asst-msg-2"), "{v}");
    assert!(items.iter().all(|i| i["read"] == false), "{v}");
    assert!(v["unread_count"].as_u64().unwrap() >= 2, "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_02_mark_read_updates_list() {
    let (api, _gate) = boot().await;
    let (s, v) = api
        .post(
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": ["asst-msg-1"] }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["newly_marked"], 1);
    let reads = v["read_ids"].as_array().unwrap();
    assert!(reads.iter().any(|x| x == "asst-msg-1"), "{v}");

    let (s2, list) = api.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s2, 200, "{list}");
    let msg1 = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-1")
        .unwrap();
    assert_eq!(msg1["read"], true);
    let unread = list["unread_count"].as_u64().unwrap();
    assert_eq!(unread, (list["items"].as_array().unwrap().len() as u64) - 1);

    let (s3, reads) = api.get("/api/desktop/v1/assistant/messages/read").await;
    assert_eq!(s3, 200, "{reads}");
    assert!(
        reads["read_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "asst-msg-1"),
        "{reads}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_03_mark_read_is_idempotent() {
    let (api, _gate) = boot().await;
    let (s1, v1) = api
        .post(
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": ["asst-msg-1"] }),
        )
        .await;
    assert_eq!(s1, 200, "{v1}");
    assert_eq!(v1["newly_marked"], 1);
    let (s2, v2) = api
        .post(
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": ["asst-msg-1"] }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["newly_marked"], 0);
    assert_eq!(v1["read_ids"], v2["read_ids"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_04_empty_message_ids_rejected() {
    let (api, _gate) = boot().await;
    let (s, v) = api
        .post(
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": [] }),
        )
        .await;
    assert_eq!(s, 400, "{v}");
    assert_eq!(v["code"], "invalid_request");
}
