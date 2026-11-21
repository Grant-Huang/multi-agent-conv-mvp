//! RT-ADACCEPT：可靠受理 HTTP（B1）。用例见 docs/testing/CASES.md。

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

async fn boot() -> (Api, Arc<Runtime>, String) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-accept"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let conv = rt.store.create_conversation("SMT-03 受理测").unwrap();
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (api, rt, conv.id)
}

fn body(op: &str, text: &str) -> Value {
    json!({
        "client_operation_id": op,
        "text": text,
        "scope": "continue",
        "task_id": "TASK-2026-018"
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adaccept_01_post_returns_202_with_ids() {
    let (api, rt, conv_id) = boot().await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/conversations/{conv_id}/requests"),
            body("op-1", "深入调查 SMT-03"),
        )
        .await;
    assert_eq!(s, 202, "{v}");
    assert!(v["request_id"].as_str().unwrap().starts_with("dreq"));
    assert!(v["message_id"].as_str().unwrap().starts_with("msg"));
    assert_eq!(v["status"], "accepted");
    assert_eq!(v["dispatch_count"], 1);
    assert_eq!(v["replayed"], false);
    let msgs = rt.store.list_messages(&conv_id, None).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "深入调查 SMT-03");
    assert_eq!(msgs[0].id, v["message_id"].as_str().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adaccept_02_same_op_id_replays_without_second_dispatch() {
    let (api, rt, conv_id) = boot().await;
    let path = format!("/api/desktop/v1/conversations/{conv_id}/requests");
    let (s1, v1) = api.post(&path, body("op-replay", "同一正文")).await;
    assert_eq!(s1, 202, "{v1}");
    let (s2, v2) = api.post(&path, body("op-replay", "同一正文")).await;
    assert_eq!(s2, 202, "{v2}");
    assert_eq!(v2["request_id"], v1["request_id"]);
    assert_eq!(v2["message_id"], v1["message_id"]);
    assert_eq!(v2["dispatch_count"], 1);
    assert_eq!(v2["replayed"], true);
    let msgs = rt.store.list_messages(&conv_id, None).unwrap();
    assert_eq!(msgs.iter().filter(|m| m.role == "user").count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adaccept_03_same_op_id_changed_text_conflicts() {
    let (api, rt, conv_id) = boot().await;
    let path = format!("/api/desktop/v1/conversations/{conv_id}/requests");
    let (s1, v1) = api.post(&path, body("op-conflict", "原文")).await;
    assert_eq!(s1, 202, "{v1}");
    let (s2, v2) = api.post(&path, body("op-conflict", "改正文")).await;
    assert_eq!(s2, 409, "{v2}");
    assert_eq!(v2["code"], "operation_conflict");
    assert_eq!(v2["request_id"], v1["request_id"]);
    assert_eq!(v2["message_id"], v1["message_id"]);
    let msgs = rt.store.list_messages(&conv_id, None).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "原文");
    let rec = rt
        .store
        .get_desktop_request(v1["request_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(rec.dispatch_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adaccept_04_get_request_after_dropped_receipt() {
    let (api, _rt, conv_id) = boot().await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/conversations/{conv_id}/requests"),
            body("op-readback", "丢回执后回读"),
        )
        .await;
    assert_eq!(s, 202, "{v}");
    let rid = v["request_id"].as_str().unwrap();
    let (gs, gv) = api.get(&format!("/api/desktop/v1/requests/{rid}")).await;
    assert_eq!(gs, 200, "{gv}");
    assert_eq!(gv["request_id"], rid);
    assert_eq!(gv["message_id"], v["message_id"]);
    assert_eq!(gv["status"], "accepted");
    assert_eq!(gv["dispatch_count"], 1);
    assert_eq!(gv["text"], "丢回执后回读");
}
