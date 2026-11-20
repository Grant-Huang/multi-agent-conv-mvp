//! 业务内省 S6 HTTP：RT-INT-10、RT-INT-21（HTTP 侧）、推送/未读/标注。

use std::sync::Arc;

use an_runtime::config::AppConfig;
use an_runtime::introspect::{
    lock_operator_override, set_operator_override, ActionSummary, ApprovalSummary, CaseSummary,
    MemorySource, RunOnceOptions, RunOnceRequest, MAIN_CONVERSATION_TITLE,
};
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn config(min_samples: u32, push_threshold: f64) -> AppConfig {
    let text = format!(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n\
         [introspection]\nenabled = true\nmin_samples = {min_samples}\npush_threshold = {push_threshold}\ndaily_cap = 10\n"
    );
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
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
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

async fn boot(min_samples: u32, push_threshold: f64) -> (Api, Arc<Runtime>) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-int-http"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(
        Runtime::new(
            config(min_samples, push_threshold),
            Store::open(&dir.join("t.db")).unwrap(),
        )
        .unwrap(),
    );
    let base = serve(an_server::api(rt.clone())).await;
    (
        Api {
            base,
            http: reqwest::Client::new(),
        },
        rt,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_int_21_http_non_operator_forbidden_no_leak() {
    let _lock = lock_operator_override().await;
    set_operator_override(Some(false));
    let (api, _rt) = boot(1, 4.0).await;
    let (s, v) = api.get("/api/introspection/findings").await;
    assert_eq!(s, 403);
    assert!(v.get("findings").is_none(), "{v}");
    assert!(v.get("unread").is_none(), "{v}");
    assert!(v.get("title").is_none(), "{v}");
    assert_eq!(v["error"], "需要运营负责人权限");
    let (s, v) = api.get("/api/introspection/unread").await;
    assert_eq!(s, 403);
    assert!(v.get("unread").is_none());
    set_operator_override(None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_int_10_reject_keeps_finding_and_gate_config() {
    let _lock = lock_operator_override().await;
    set_operator_override(None);
    let (api, rt) = boot(1, 4.0).await;
    let cfg_before = rt.config.introspection.clone();

    let (s, v) = api
        .post(
            "/api/introspection/run",
            json!({ "fixture": "adoption_push" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["status"], "done");

    let (s, list) = api.get("/api/introspection/findings").await;
    assert_eq!(s, 200);
    let findings = list["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    let id = findings[0]["id"].as_str().unwrap().to_string();

    let (s, v) = api
        .post(
            &format!("/api/introspection/findings/{id}/verdict"),
            json!({ "verdict": "rejected", "note": "样本太少" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["finding"]["verdict"], "rejected");

    // 默认列表不再出现
    let (_, list2) = api.get("/api/introspection/findings").await;
    assert!(list2["findings"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["id"] != id));

    // 仍可按 id 取到；再跑分析不删、不改规则
    let (s, detail) = api.get(&format!("/api/introspection/findings/{id}")).await;
    assert_eq!(s, 200);
    assert_eq!(detail["finding"]["verdict"], "rejected");

    let (s, v) = api
        .post(
            "/api/introspection/run",
            json!({ "fixture": "adoption_push" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let again = rt.store.introspection_get_finding(&id).unwrap().unwrap();
    assert_eq!(again.verdict.as_deref(), Some("rejected"));
    assert_eq!(rt.config.introspection, cfg_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn introspection_http_label_unread_push_and_read() {
    let _lock = lock_operator_override().await;
    set_operator_override(None);
    // 阈值 5：单案例 Supported 得分 4，入库但不推送
    let (api, rt) = boot(1, 5.0).await;

    let (s, v) = api
        .post(
            "/api/introspection/run",
            json!({ "fixture": "adoption_hold" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let (s, list) = api.get("/api/introspection/findings").await;
    assert_eq!(s, 200, "{list}");
    let id = list["findings"][0]["id"].as_str().unwrap().to_string();
    assert!(
        list["findings"][0]["gate_reason"]
            .as_str()
            .unwrap()
            .contains("不推送"),
        "{list}"
    );
    assert_eq!(list["findings"][0]["pushed"], false);

    let (s, v) = api
        .post(
            &format!("/api/introspection/findings/{id}/label"),
            json!({ "kind": "should_push", "note": "很有用" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["label"]["kind"], "should_push");
    // 标注不立即推送
    assert_eq!(rt.store.introspection_unread_count().unwrap(), 0);

    // 推送夹具（阈值 4 + 三案例）
    let (api2, rt2) = boot(1, 4.0).await;
    let (s, v) = api2
        .post(
            "/api/introspection/run",
            json!({ "fixture": "adoption_push" }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let (s, u) = api2.get("/api/introspection/unread").await;
    assert_eq!(s, 200);
    assert!(u["unread"].as_u64().unwrap() >= 1);
    assert_eq!(u["title"], MAIN_CONVERSATION_TITLE);
    let main = u["conversation_id"].as_str().unwrap().to_string();
    let views = rt2.store.list_message_views(&main, None).unwrap();
    assert!(!views.is_empty());
    assert!(views[0].message.task_id.is_none());

    let (s, v) = api2.post("/api/introspection/read", json!({})).await;
    assert_eq!(s, 200, "{v}");
    let (_, u2) = api2.get("/api/introspection/unread").await;
    assert_eq!(u2["unread"], 0);
}

/// 直接再跑 MemorySource 证明否决后 insert 幂等（RT-INT-10 配套）。
#[tokio::test]
async fn rt_int_10_rerun_insert_idempotent_after_reject() {
    use an_runtime::introspect::run_once;
    use std::path::Path;
    use std::time::Duration;

    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = an_runtime::config::IntrospectionSection {
        enabled: true,
        min_samples: 1,
        daily_cap: 10,
        ..Default::default()
    };
    let src = MemorySource {
        source_id: "t".into(),
        cases: vec![
            CaseSummary {
                case_id: "a".into(),
                template: "t".into(),
                scenario: "s1".into(),
                approvals: vec![ApprovalSummary {
                    id: "p".into(),
                    scope: "production".into(),
                    approver: "s".into(),
                    expires_at: 10_000,
                    plan_revision: 1,
                }],
                actions: vec![ActionSummary {
                    key: "a:a".into(),
                    state: "ACCEPTED".into(),
                    verification: Some("ok".into()),
                    accepted_at: 1,
                    complete_at: 2,
                }],
                event_times: vec![(1, 1)],
            },
            CaseSummary {
                case_id: "b".into(),
                template: "t".into(),
                scenario: "s1".into(),
                approvals: vec![ApprovalSummary {
                    id: "p".into(),
                    scope: "production".into(),
                    approver: "s".into(),
                    expires_at: 10_000,
                    plan_revision: 1,
                }],
                actions: vec![ActionSummary {
                    key: "b:a".into(),
                    state: "ACCEPTED".into(),
                    verification: Some("ok".into()),
                    accepted_at: 1,
                    complete_at: 2,
                }],
                event_times: vec![(1, 1)],
            },
            CaseSummary {
                case_id: "c".into(),
                template: "t".into(),
                scenario: "s1".into(),
                approvals: vec![ApprovalSummary {
                    id: "p".into(),
                    scope: "production".into(),
                    approver: "s".into(),
                    expires_at: 10_000,
                    plan_revision: 1,
                }],
                actions: vec![ActionSummary {
                    key: "c:a".into(),
                    state: "ACCEPTED".into(),
                    verification: Some("ok".into()),
                    accepted_at: 1,
                    complete_at: 2,
                }],
                event_times: vec![(1, 1)],
            },
        ],
    };
    let opts = RunOnceOptions {
        timeout: Duration::from_secs(30),
        lease_until: "9999-12-31T00:00:00.000Z".into(),
        cancel: None,
        after_finding_delay: None,
        cancel_after_findings: None,
    };
    run_once(RunOnceRequest {
        store: &store,
        source: &src,
        cfg: &cfg,
        trigger: "manual",
        watermark_json: "{}",
        input_hash: "h",
        analysis_now: 1000,
        opts: opts.clone(),
        bus: None,
        explainer: None,
    })
    .await
    .unwrap();
    let f = store.introspection_list_findings(None, 10).unwrap();
    assert_eq!(f.len(), 1);
    let id = f[0].id.clone();
    store
        .introspection_set_verdict(&id, "rejected", "no")
        .unwrap();
    run_once(RunOnceRequest {
        store: &store,
        source: &src,
        cfg: &cfg,
        trigger: "manual",
        watermark_json: "{}",
        input_hash: "h",
        analysis_now: 1000,
        opts,
        bus: None,
        explainer: None,
    })
    .await
    .unwrap();
    assert_eq!(
        store.introspection_list_findings(None, 10).unwrap().len(),
        1
    );
    assert_eq!(
        store
            .introspection_get_finding(&id)
            .unwrap()
            .unwrap()
            .verdict
            .as_deref(),
        Some("rejected")
    );
}
