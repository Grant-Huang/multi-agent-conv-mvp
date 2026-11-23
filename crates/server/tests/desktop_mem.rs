//! RT-ADMEM：记忆列表夹具 HTTP。用例见 docs/testing/CASES.md。

use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::Value;

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
}

async fn boot() -> Api {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-mem"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    Api {
        base: serve(an_server::api(rt)).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_admem_01_list_returns_fixture_items() {
    let api = boot().await;
    let (s, v) = api.get("/api/desktop/v1/memories").await;
    assert_eq!(s, 200, "{v}");
    let items = v["items"].as_array().expect("items array");
    assert!(items.len() >= 2, "{v}");
    let ids: Vec<&str> = items.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(ids.contains(&"mem-1"), "{v}");
    assert!(ids.contains(&"mem-2"), "{v}");
    let mem1 = items.iter().find(|i| i["id"] == "mem-1").unwrap();
    assert!(mem1["title"].as_str().unwrap().contains("质量报告"));
    assert!(!mem1["source"].as_str().unwrap().is_empty());
    assert!(!mem1["body"].as_str().unwrap().is_empty());
    assert!(!mem1["expires"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_admem_02_get_detail_matches_list() {
    let api = boot().await;
    let (s_list, list) = api.get("/api/desktop/v1/memories").await;
    assert_eq!(s_list, 200, "{list}");
    let listed = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "mem-1")
        .unwrap()
        .clone();
    let (s, v) = api.get("/api/desktop/v1/memories/mem-1").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["id"], listed["id"]);
    assert_eq!(v["title"], listed["title"]);
    assert_eq!(v["body"], listed["body"]);
    assert_eq!(v["source"], listed["source"]);
    assert_eq!(v["expires"], listed["expires"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_admem_03_unknown_id_not_found() {
    let api = boot().await;
    let (s, v) = api.get("/api/desktop/v1/memories/no-such").await;
    assert_eq!(s, 404, "{v}");
    assert_eq!(v["code"], "not_found");
}
