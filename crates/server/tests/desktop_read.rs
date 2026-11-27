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
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n[desktop]\ntest_fixtures = true\n",
    )
    .unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn get(&self, path: &str) -> (u16, Value) {
        self.get_as("", path).await
    }
    /// 带 `x-desktop-principal` 夹具头请求（M4 用户隔离口径；空串 = 不带头）
    async fn get_as(&self, principal: &str, path: &str) -> (u16, Value) {
        let mut r = self.http.get(format!("{}{path}", self.base));
        if !principal.is_empty() {
            r = r.header("x-desktop-principal", principal);
        }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.post_as("", path, body).await
    }
    async fn post_as(&self, principal: &str, path: &str, body: Value) -> (u16, Value) {
        let mut r = self.http.post(format!("{}{path}", self.base)).json(&body);
        if !principal.is_empty() {
            r = r.header("x-desktop-principal", principal);
        }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

fn read_test_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

/// 在指定库文件上启动服务（T-96 持久化/隔离探针用）
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

async fn boot() -> (Api, MutexGuard<'static, ()>) {
    let guard = read_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-read"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let api = boot_at(&db).await;
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

/// RT-ADXR-05（T-96）：同库重启持久。已读标记后同一库重开服务，状态必须仍在。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_05_reopen_same_db_keeps_read_state() {
    let _gate = read_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-read-p5"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    {
        let api = boot_at(&db).await;
        let (s, v) = api
            .post(
                "/api/desktop/v1/assistant/messages/read",
                json!({ "message_ids": ["asst-msg-1"] }),
            )
            .await;
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["newly_marked"], 1);
    }
    // 同库重开（新进程语义）：已读状态必须保留
    let api2 = boot_at(&db).await;
    let (s, list) = api2.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s, 200, "{list}");
    let msg1 = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-1")
        .expect("asst-msg-1 应存在");
    assert_eq!(msg1["read"], true, "重启后已读状态丢失");
    let (s, reads) = api2.get("/api/desktop/v1/assistant/messages/read").await;
    assert_eq!(s, 200, "{reads}");
    assert!(
        reads["read_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "asst-msg-1"),
        "{reads}"
    );
}

/// RT-ADXR-06（T-96）：库间隔离。全新库上的服务不得看到其他库/进程标记的已读状态
/// （H4 同族探针：进程级状态不得跨库泄漏）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_06_fresh_db_does_not_inherit_process_state() {
    let _gate = read_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-read-p6"));
    std::fs::create_dir_all(&dir).unwrap();
    let db1 = dir.join("a.db");
    let db2 = dir.join("b.db");

    let api_a = boot_at(&db1).await;
    let (s, _) = api_a
        .post(
            "/api/desktop/v1/assistant/messages/fixture/reset",
            json!({}),
        )
        .await;
    assert_eq!(s, 200);
    let api_b = boot_at(&db2).await;
    let (s, seed_list) = api_b.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s, 200, "{seed_list}");
    let seed_unread = seed_list["unread_count"].as_u64().unwrap();

    // 库一标记已读
    let (s, v) = api_a
        .post(
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": ["asst-msg-1"] }),
        )
        .await;
    assert_eq!(s, 200, "{v}");

    // 库二（全新库）必须仍是种子态：asst-msg-1 未读、未读数不变
    let (s, list_b) = api_b.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s, 200, "{list_b}");
    let msg1_b = list_b["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-1")
        .expect("asst-msg-1 应存在");
    assert_eq!(
        msg1_b["read"], false,
        "进程级已读状态泄漏到了另一个全新库: {list_b}"
    );
    assert_eq!(
        list_b["unread_count"].as_u64().unwrap(),
        seed_unread,
        "新库未读数应保持种子值"
    );
}

/// RT-ADXR-07（M4，Issue #361）：已读状态按用户隔离。
/// alice 标记的已读，bob 查询不得看到（跨用户状态泄漏修复的回归用例）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adxr_07_read_state_is_isolated_per_user() {
    let (api, _gate) = boot().await;
    // alice 标记 asst-msg-2 已读
    let (s, v) = api
        .post_as(
            "alice",
            "/api/desktop/v1/assistant/messages/read",
            json!({ "message_ids": ["asst-msg-2"] }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["newly_marked"], 1, "{v}");

    // alice 自己看到已读
    let (s, list_a) = api
        .get_as("alice", "/api/desktop/v1/assistant/messages")
        .await;
    assert_eq!(s, 200, "{list_a}");
    let msg2_a = list_a["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-2")
        .expect("asst-msg-2 应存在");
    assert_eq!(msg2_a["read"], true, "alice 应看到自己的已读态: {list_a}");

    // bob 看到未读：alice 的已读态不得泄漏给 bob
    let (s, list_b) = api
        .get_as("bob", "/api/desktop/v1/assistant/messages")
        .await;
    assert_eq!(s, 200, "{list_b}");
    let msg2_b = list_b["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-2")
        .expect("asst-msg-2 应存在");
    assert_eq!(
        msg2_b["read"], false,
        "alice 的已读态泄漏给了 bob（M4 跨用户状态泄漏）: {list_b}"
    );
    let (s, reads_b) = api
        .get_as("bob", "/api/desktop/v1/assistant/messages/read")
        .await;
    assert_eq!(s, 200, "{reads_b}");
    assert!(
        !reads_b["read_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "asst-msg-2"),
        "bob 的 read_ids 不应含 alice 标记的条目: {reads_b}"
    );

    // 无头请求回退 local 用户：与 alice/bob 互不可见（现网单用户行为不变的护栏）
    let (s, list_local) = api.get("/api/desktop/v1/assistant/messages").await;
    assert_eq!(s, 200, "{list_local}");
    let msg2_local = list_local["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "asst-msg-2")
        .expect("asst-msg-2 应存在");
    assert_eq!(
        msg2_local["read"], false,
        "无头回退 local 不应看到 alice 的已读态"
    );
}
