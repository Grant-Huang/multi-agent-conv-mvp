//! RT-ADDRAFTS：工作区草稿上云（T-84）。用例见 docs/testing/CASES.md。

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
    async fn put(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .put(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn delete(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .delete(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn api_for(rt: Arc<Runtime>) -> Api {
    Api {
        base: serve(an_server::api(rt)).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    }
}

async fn boot() -> (Api, Arc<Runtime>) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-drafts"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(Runtime::new(config(), Store::open(&dir.join("an.db")).unwrap()).unwrap());
    (api_for(rt.clone()).await, rt)
}

async fn boot_at(db: &std::path::Path) -> Api {
    let rt = Arc::new(Runtime::new(config(), Store::open(db).unwrap()).unwrap());
    api_for(rt).await
}

/// RT-ADDRAFTS-01：保存/读取回环，版本与 scope/continue_task_id 一致。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_addrafts_01_save_load_roundtrip() {
    let (api, _rt) = boot().await;
    let (s, v) = api
        .put(
            "/api/desktop/v1/drafts/task/TASK-X",
            json!({
                "text": "第一版草稿",
                "scope": "continue",
                "continue_task_id": "TASK-X",
                "version": 1
            }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["draft"]["version"], 1, "{v}");
    let (s, v) = api.get("/api/desktop/v1/drafts/task/TASK-X").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["draft"]["text"], "第一版草稿", "{v}");
    assert_eq!(v["draft"]["scope"], "continue", "{v}");
    assert_eq!(v["draft"]["continue_task_id"], "TASK-X", "{v}");
    assert_eq!(v["draft"]["version"], 1, "{v}");
    // 二次编辑（version 由客户端递增，expected_version 做丢失更新保护）
    let (s, v) = api
        .put(
            "/api/desktop/v1/drafts/task/TASK-X",
            json!({
                "text": "第二版草稿",
                "scope": "continue",
                "continue_task_id": "TASK-X",
                "version": 2,
                "expected_version": 1
            }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["draft"]["version"], 2, "{v}");
}

/// RT-ADDRAFTS-02：expected_version 过期 → 409 + 当前草稿，不覆盖。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_addrafts_02_version_conflict() {
    let (api, _rt) = boot().await;
    api.put(
        "/api/desktop/v1/drafts/task/TASK-C",
        json!({ "text": "已有内容", "version": 2 }),
    )
    .await;
    let (s, v) = api
        .put(
            "/api/desktop/v1/drafts/task/TASK-C",
            json!({
                "text": "过期设备的覆盖",
                "version": 3,
                "expected_version": 1
            }),
        )
        .await;
    assert_eq!(s, 409, "{v}");
    assert_eq!(v["code"], "version_conflict", "{v}");
    assert_eq!(v["draft"]["text"], "已有内容", "{v}");
    assert_eq!(v["draft"]["version"], 2, "{v}");
    let (s, g) = api.get("/api/desktop/v1/drafts/task/TASK-C").await;
    assert_eq!(s, 200, "{g}");
    assert_eq!(g["draft"]["text"], "已有内容", "{g}");
}

/// RT-ADDRAFTS-03：发送后按 sent_version 清空正文；stale 版本幂等不变化。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_addrafts_03_clear_after_send() {
    let (api, _rt) = boot().await;
    api.put(
        "/api/desktop/v1/drafts/conv/conv-A",
        json!({
            "text": "发送前的草稿",
            "scope": "independent",
            "version": 2
        }),
    )
    .await;
    let (s, v) = api
        .delete("/api/desktop/v1/drafts/conv/conv-A?sent_version=2")
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["draft"]["text"], "", "{v}");
    assert_eq!(v["draft"]["version"], 3, "{v}");
    assert_eq!(v["draft"]["scope"], "independent", "{v}");
    // stale sent_version：不再变化（幂等）
    let (s2, v2) = api
        .delete("/api/desktop/v1/drafts/conv/conv-A?sent_version=2")
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["draft"]["text"], "", "{v2}");
    assert_eq!(v2["draft"]["version"], 3, "{v2}");
}

/// RT-ADDRAFTS-04：同库重启持久（scope/continue_task_id 保留）；kind 非法 400。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_addrafts_04_restart_persists_and_kind_validation() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-drafts-p4"));
    std::fs::create_dir_all(&dir).unwrap();
    let db: PathBuf = dir.join("an.db");
    {
        let api = boot_at(&db).await;
        let (s, v) = api
            .put(
                "/api/desktop/v1/drafts/task/TASK-P",
                json!({
                    "text": "重启仍在的草稿",
                    "scope": "continue",
                    "continue_task_id": "TASK-P",
                    "version": 4
                }),
            )
            .await;
        assert_eq!(s, 200, "{v}");
    }
    // 同库重开（新进程语义）：草稿与 scope/continue_task_id 仍在
    let api2 = boot_at(&db).await;
    let (s, v) = api2.get("/api/desktop/v1/drafts/task/TASK-P").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["draft"]["text"], "重启仍在的草稿", "{v}");
    assert_eq!(v["draft"]["scope"], "continue", "{v}");
    assert_eq!(v["draft"]["continue_task_id"], "TASK-P", "{v}");
    assert_eq!(v["draft"]["version"], 4, "{v}");
    // kind 非法
    let (s, v) = api2
        .put(
            "/api/desktop/v1/drafts/other/X",
            json!({ "text": "x", "version": 1 }),
        )
        .await;
    assert_eq!(s, 400, "{v}");
    assert_eq!(v["code"], "invalid_request", "{v}");
}
