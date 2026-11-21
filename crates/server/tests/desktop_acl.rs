//! RT-ADACL：证据 ACL 与撤权（C1）。用例见 docs/testing/CASES.md。

use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::{DesktopEvidence, Store};
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
    async fn get(&self, path: &str, principal: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .header("x-desktop-principal", principal)
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
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-acl"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(Runtime::new(config(), Store::open(&dir.join("an.db")).unwrap()).unwrap());
    rt.store.ensure_desktop_evidence_fixtures().unwrap();
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (api, rt)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adacl_01_alice_can_read_summary_and_source() {
    let (api, _) = boot().await;
    let (s, v) = api
        .get("/api/desktop/v1/evidence/ev-smt-qc-1?view=summary", "alice")
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["evidence"]["summary"].as_str().unwrap().contains("抽检"));
    assert!(v["evidence"]["source_body"]
        .as_str()
        .unwrap_or("")
        .is_empty());

    let (s2, v2) = api
        .get("/api/desktop/v1/evidence/ev-smt-qc-1?view=source", "alice")
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert!(v2["evidence"]["source_body"]
        .as_str()
        .unwrap()
        .contains("LOT=SMT-03-Q"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adacl_02_bob_forbidden_no_content_leak() {
    let (api, _) = boot().await;
    let (s, v) = api
        .get("/api/desktop/v1/evidence/ev-smt-qc-1?view=source", "bob")
        .await;
    assert_eq!(s, 403, "{v}");
    assert_eq!(v["code"], "forbidden");
    let raw = v.to_string();
    assert!(!raw.contains("LOT=SMT-03-Q"));
    assert!(!raw.contains("抽检"));
    assert!(!raw.contains("source_body"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adacl_03_revoke_blocks_all_views_and_search() {
    let (api, _) = boot().await;
    let (r, rv) = api
        .post(
            "/api/desktop/v1/evidence/ev-smt-qc-1/revoke",
            json!({ "principal_id": "alice" }),
        )
        .await;
    assert_eq!(r, 200, "{rv}");
    let (r2, _) = api
        .post(
            "/api/desktop/v1/evidence/ev-smt-qc-1/revoke",
            json!({ "principal_id": "alice" }),
        )
        .await;
    assert_eq!(r2, 200);

    for view in ["summary", "source", "download"] {
        let (s, v) = api
            .get(
                &format!("/api/desktop/v1/evidence/ev-smt-qc-1?view={view}"),
                "alice",
            )
            .await;
        assert_eq!(s, 403, "{view} {v}");
        assert!(!v.to_string().contains("LOT=SMT-03-Q"));
    }
    let (ls, list) = api
        .get("/api/desktop/v1/tasks/TASK-2026-018/evidence", "alice")
        .await;
    assert_eq!(ls, 200, "{list}");
    assert!(list["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["id"] != "ev-smt-qc-1"));

    let (ss, search) = api
        .get("/api/desktop/v1/evidence/search?q=首件", "alice")
        .await;
    assert_eq!(ss, 200, "{search}");
    assert!(search["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["id"] != "ev-smt-qc-1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adacl_04_revoke_alice_does_not_affect_carol() {
    let (api, _) = boot().await;
    let (r, _) = api
        .post(
            "/api/desktop/v1/evidence/ev-smt-qc-1/revoke",
            json!({ "principal_id": "alice" }),
        )
        .await;
    assert_eq!(r, 200);
    let (s, v) = api
        .get("/api/desktop/v1/evidence/ev-smt-qc-1?view=source", "carol")
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(v["evidence"]["source_body"]
        .as_str()
        .unwrap()
        .contains("LOT=SMT-03-Q"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adacl_05_expired_or_revoked_source_returns_410() {
    let (api, rt) = boot().await;
    let (s, v) = api
        .get(
            "/api/desktop/v1/evidence/ev-smt-revoked-src?view=source",
            "alice",
        )
        .await;
    assert_eq!(s, 410, "{v}");
    assert_eq!(v["code"], "source_unavailable");
    assert!(!v.to_string().contains("SECRET-SHOULD-NOT-LEAK"));

    let (s2, v2) = api
        .get("/api/desktop/v1/evidence/ev-smt-exp?view=summary", "alice")
        .await;
    assert_eq!(s2, 410, "{v2}");
    assert_eq!(v2["code"], "source_unavailable");

    // 自定义 expired 仍不回正文
    rt.store
        .upsert_desktop_evidence_with_grant(
            &DesktopEvidence {
                id: "ev-custom-exp".into(),
                task_id: "TASK-2026-018".into(),
                title: "自定义过期".into(),
                object_label: "x".into(),
                collected_at: "2026-01-01T00:00:00Z".into(),
                source_type: "t".into(),
                version: "v0".into(),
                status: "expired".into(),
                summary: "不应出现在 410 body".into(),
                source_body: "BODY-LEAK".into(),
                error_message: "过期".into(),
            },
            "alice",
        )
        .unwrap();
    let (s3, v3) = api
        .get(
            "/api/desktop/v1/evidence/ev-custom-exp?view=download",
            "alice",
        )
        .await;
    assert_eq!(s3, 410, "{v3}");
    assert!(!v3.to_string().contains("BODY-LEAK"));
}
