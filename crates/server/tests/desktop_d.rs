//! RT-ADVOICE / RT-ADHAND：D 语音与跨端。用例见 docs/testing/CASES.md。

use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

const TASK: &str = "TASK-2026-018";

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
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn boot() -> Api {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-d"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(Runtime::new(config(), Store::open(&dir.join("an.db")).unwrap()).unwrap());
    Api {
        base: serve(an_server::api(rt)).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_advoice_01_deny_then_as_text() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/reset"),
        json!({}),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/permission"),
            json!({ "permission": "denied" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["session"]["permission"], "denied");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/as-text"),
            json!({ "text": "改用文字继续" }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["session"]["final_text"], "改用文字继续");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_advoice_02_partial_not_linked_until_finalize() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/reset"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/start"),
        json!({}),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/partial"),
            json!({ "text": "正在说…SMT" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["session"]["linked_task_id"].is_null());
    assert_eq!(v["session"]["final_text"], "");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/finalize"),
            json!({ "text": "调查 SMT-03 振动异常", "link_task": true }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["session"]["linked_task_id"], TASK);
    assert!(v2["session"]["final_text"]
        .as_str()
        .unwrap()
        .contains("SMT-03"));
    assert_eq!(v2["session"]["partial"], "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_advoice_03_barge_stops_playback() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/reset"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/start"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/finalize"),
        json!({ "text": "调查 SMT-03 振动异常", "link_task": true }),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/play"),
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["session"]["playing"], true);
    assert_eq!(v["session"]["phase"], "speaking");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/barge"),
            json!({}),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["session"]["playing"], false);
    assert_eq!(v2["session"]["phase"], "listening");
    assert!(v2["session"]["final_text"]
        .as_str()
        .unwrap()
        .contains("SMT-03"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_advoice_04_offline_keeps_final_via_as_text() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/reset"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/start"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/voice/finalize"),
        json!({ "text": "调查 SMT-03 振动异常", "link_task": true }),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/offline"),
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["session"]["phase"], "offline");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/voice/as-text"),
            json!({ "text": "调查 SMT-03 振动异常" }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["session"]["phase"], "offline");
    assert!(v2["session"]["final_text"]
        .as_str()
        .unwrap()
        .contains("SMT-03"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adhand_01_same_task_run_anchor() {
    let api = boot().await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/handoff/reset"),
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["handoff"]["task_id"], TASK);
    assert_eq!(v["handoff"]["run_id"], "run-smt-03-1");
    assert_eq!(v["handoff"]["read_message_id"], "msg-anchor-1");
    assert_eq!(v["handoff"]["voice_shared"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adhand_02_conflict_resolve_phone() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/handoff/reset"),
        json!({}),
    )
    .await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/handoff/draft"),
        json!({ "device": "phone", "text": "手机侧草稿 A" }),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/handoff/draft"),
            json!({ "device": "desktop", "text": "桌面侧草稿 B" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["handoff"]["conflict"], true);
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/handoff/resolve"),
            json!({ "chosen": "phone" }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["handoff"]["conflict"], false);
    assert_eq!(v2["handoff"]["chosen"], "phone");
    assert_eq!(v2["handoff"]["phone_text"], "手机侧草稿 A");
    assert_eq!(v2["handoff"]["desktop_text"], "手机侧草稿 A");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adhand_03_voice_shared_toggle() {
    let api = boot().await;
    api.post(
        &format!("/api/desktop/v1/tasks/{TASK}/handoff/reset"),
        json!({}),
    )
    .await;
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/handoff/share-voice"),
            json!({ "share": true }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["handoff"]["voice_shared"], true);
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/handoff/share-voice"),
            json!({ "share": false }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["handoff"]["voice_shared"], false);
}
