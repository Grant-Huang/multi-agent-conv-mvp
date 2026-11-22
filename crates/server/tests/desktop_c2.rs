//! RT-ADAPPR / RT-ADATTN：C2 动作网关与 Attention。用例见 docs/testing/CASES.md。

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

async fn boot() -> Api {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-c2"));
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

async fn reset(api: &Api) -> Value {
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/tasks/{TASK}/approvals/reset"),
            json!({}),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    v["approval"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adappr_01_approve_matching_revision() {
    let api = boot().await;
    let appr = reset(&api).await;
    let id = appr["id"].as_str().unwrap();
    let rev = appr["plan_revision"].as_i64().unwrap();
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/decide"),
            json!({ "decision": "approve", "expected_revision": rev }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["approval"]["status"], "approved");
    assert!(!v["approval"]["decided_by"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adappr_02_bump_invalidates_old_execute() {
    let api = boot().await;
    let appr = reset(&api).await;
    let id = appr["id"].as_str().unwrap();
    let rev = appr["plan_revision"].as_i64().unwrap();
    let (s, _) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/decide"),
            json!({ "decision": "approve", "expected_revision": rev }),
        )
        .await;
    assert_eq!(s, 200);
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/bump"),
            json!({ "params": "改排产 + 刀具补齐" }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["approval"]["plan_revision"], rev + 1);
    assert_eq!(v2["approval"]["status"], "pending");
    let (s3, v3) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/execute"),
            json!({ "expected_revision": rev }),
        )
        .await;
    assert_eq!(s3, 409, "{v3}");
    assert_eq!(v3["code"], "revision_conflict");
    let (sg, g) = api
        .get(&format!("/api/desktop/v1/tasks/{TASK}/approvals"))
        .await;
    assert_eq!(sg, 200, "{g}");
    assert_eq!(g["approval"]["execute_count"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adappr_03_reject_blocks_execute() {
    let api = boot().await;
    let appr = reset(&api).await;
    let id = appr["id"].as_str().unwrap();
    let rev = appr["plan_revision"].as_i64().unwrap();
    let (s, _) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/decide"),
            json!({ "decision": "reject", "expected_revision": rev }),
        )
        .await;
    assert_eq!(s, 200);
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/execute"),
            json!({ "expected_revision": rev }),
        )
        .await;
    assert!(s2 == 403 || s2 == 409, "{v2}");
    assert_eq!(v2["approval"]["execute_count"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adappr_04_execute_once() {
    let api = boot().await;
    let appr = reset(&api).await;
    let id = appr["id"].as_str().unwrap();
    let rev = appr["plan_revision"].as_i64().unwrap();
    api.post(
        &format!("/api/desktop/v1/approvals/{id}/decide"),
        json!({ "decision": "approve", "expected_revision": rev }),
    )
    .await;
    let (s1, v1) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/execute"),
            json!({ "expected_revision": rev }),
        )
        .await;
    assert_eq!(s1, 200, "{v1}");
    assert_eq!(v1["approval"]["execute_count"], 1);
    assert_eq!(v1["approval"]["last_receipt"], "applied");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/execute"),
            json!({ "expected_revision": rev }),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["approval"]["execute_count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adappr_05_unknown_then_ack() {
    let api = boot().await;
    let appr = reset(&api).await;
    let id = appr["id"].as_str().unwrap();
    let rev = appr["plan_revision"].as_i64().unwrap();
    api.post(
        &format!("/api/desktop/v1/approvals/{id}/decide"),
        json!({ "decision": "approve", "expected_revision": rev }),
    )
    .await;
    let (s1, v1) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/execute"),
            json!({ "expected_revision": rev, "force_unknown": true }),
        )
        .await;
    assert_eq!(s1, 200, "{v1}");
    assert_eq!(v1["approval"]["last_receipt"], "unknown");
    let (s2, v2) = api
        .post(
            &format!("/api/desktop/v1/approvals/{id}/ack-unknown"),
            json!({}),
        )
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert_eq!(v2["approval"]["last_receipt"], "applied");
    assert_eq!(v2["approval"]["execute_count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adattn_01_mute_drops_ordinary_keeps_urgent() {
    let api = boot().await;
    let (s, _) = api
        .post(
            "/api/desktop/v1/attention/mute",
            json!({ "mute_ordinary": true }),
        )
        .await;
    assert_eq!(s, 200);
    let (so, vo) = api
        .post(
            "/api/desktop/v1/attention",
            json!({
                "kind": "ordinary",
                "title": "建议：复核 SMT-03 证据",
                "body": "可在稍后打开"
            }),
        )
        .await;
    assert_eq!(so, 200, "{vo}");
    assert_eq!(vo["delivered"], false);
    let (su, vu) = api
        .post(
            "/api/desktop/v1/attention",
            json!({
                "kind": "urgent",
                "title": "紧急：产线联锁",
                "body": "立即处理"
            }),
        )
        .await;
    assert_eq!(su, 200, "{vu}");
    assert_eq!(vu["delivered"], true);
    let (sg, g) = api.get("/api/desktop/v1/attention").await;
    assert_eq!(sg, 200, "{g}");
    let items = g["items"].as_array().unwrap();
    assert!(items.iter().all(|i| i["kind"] != "ordinary"));
    assert!(items.iter().any(|i| i["kind"] == "urgent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adattn_02_dedupe_ordinary_by_title() {
    let api = boot().await;
    api.post(
        "/api/desktop/v1/attention/mute",
        json!({ "mute_ordinary": false }),
    )
    .await;
    let title = "建议：复核 SMT-03 证据";
    for _ in 0..2 {
        let (s, v) = api
            .post(
                "/api/desktop/v1/attention",
                json!({ "kind": "ordinary", "title": title, "body": "x" }),
            )
            .await;
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["delivered"], true);
    }
    let (sg, g) = api.get("/api/desktop/v1/attention").await;
    assert_eq!(sg, 200, "{g}");
    let count = g["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "ordinary" && i["title"] == title)
        .count();
    assert_eq!(count, 1, "{g}");
}
