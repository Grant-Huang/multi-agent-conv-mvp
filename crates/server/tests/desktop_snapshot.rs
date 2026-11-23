//! RT-ADSNAP：首页快照（T-82）。用例见 docs/testing/CASES.md。

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
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-snap"));
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

/// RT-ADSNAP-01：fixture reset 后的快照形状契约（三区卡片、模拟标注、计数与游标）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adsnap_01_fixture_shape_contract() {
    let (api, _rt) = boot().await;
    let (s, v) = api
        .post("/api/desktop/v1/snapshot/fixture/reset", json!({}))
        .await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = api.get("/api/desktop/v1/snapshot").await;
    assert_eq!(s, 200, "{v}");
    let cards = v["cards"].as_array().expect("cards 数组");
    let smt = cards
        .iter()
        .find(|c| c["id"] == "card-smt-03")
        .cloned()
        .expect("SMT-03 卡应存在");
    assert_eq!(smt["zone"], "mine", "{smt}");
    assert_eq!(smt["entity_id"], "TASK-2026-018", "{smt}");
    assert_eq!(smt["card_type"], "task", "{smt}");
    assert_eq!(smt["simulated"], true, "fixture 卡必须标注模拟: {smt}");
    assert!(
        smt["updated_at"].as_str().unwrap_or_default().len() >= 10,
        "{smt}"
    );
    assert!(smt["source_refs"].is_array(), "{smt}");
    let agent = cards
        .iter()
        .find(|c| c["zone"] == "agent")
        .expect("agent 区卡");
    assert_eq!(agent["entity_id"], "TASK-2026-021", "{agent}");
    // 关注区是真实读模型：fixture 不种 attention，可为空（结构仍有效）
    assert!(v["counts"]["tasks_active"].is_u64(), "{v}");
    assert!(v["counts"]["attention_unread"].is_u64(), "{v}");
    assert!(
        !v["cursor"].as_str().unwrap_or_default().is_empty(),
        "cursor 非空: {v}"
    );
}

/// RT-ADSNAP-02：真实任务（store 权威行）出现在快照且不标模拟；fixture 卡保持模拟标注。
/// （B1 受理的 task_id 由后台分流出，探针主题是读模型反映 store 真实状态，故经 store 直插真实行。）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adsnap_02_real_task_reflects() {
    let (api, rt) = boot().await;
    let (s, v) = api
        .post("/api/desktop/v1/snapshot/fixture/reset", json!({}))
        .await;
    assert_eq!(s, 200, "{v}");
    // 经 store 插入一条真实任务行（与 orchestrator 建任务同一路径）
    let ts = an_core::now();
    let real = an_core::TaskRecord {
        id: "TASK-REAL-SNAP-01".into(),
        conversation_id: "conv-snap".into(),
        goal: "整理测试清单".into(),
        role: an_core::Role::General,
        status: an_core::TaskStatus::Queued,
        route: an_core::RouteDecision {
            task_class: an_core::TaskClass::L1,
            execution_role: an_core::Role::General,
            required_tools: vec![],
            risk_level: an_core::Risk::Low,
            reason: "rt_adsnap_02 real task".into(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: an_core::PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        },
        result: None,
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: ts.clone(),
        updated_at: ts,
    };
    rt.store
        .insert_task(&real, "rt_adsnap_02")
        .expect("插入真实任务行");
    let (s, v) = api.get("/api/desktop/v1/snapshot").await;
    assert_eq!(s, 200, "{v}");
    let cards = v["cards"].as_array().unwrap();
    let hit = cards
        .iter()
        .find(|c| c["entity_id"] == json!("TASK-REAL-SNAP-01"))
        .unwrap_or_else(|| panic!("真实任务应出现在快照: {v}"));
    assert_eq!(hit["simulated"], false, "真实任务不得标模拟: {hit}");
    assert_eq!(hit["zone"], "agent", "Queued 任务归 Agent 工作: {hit}");
    let smt = cards.iter().find(|c| c["id"] == "card-smt-03").unwrap();
    assert_eq!(smt["simulated"], true, "fixture 卡保持模拟标注: {smt}");
}

/// RT-ADSNAP-03：fixture reset 幂等可复现。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adsnap_03_fixture_reproducible() {
    let (api, _rt) = boot().await;
    for _ in 0..2 {
        let (s, v) = api
            .post("/api/desktop/v1/snapshot/fixture/reset", json!({}))
            .await;
        assert_eq!(s, 200, "{v}");
        let (s, v) = api.get("/api/desktop/v1/snapshot").await;
        assert_eq!(s, 200, "{v}");
        assert!(
            v["cards"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "card-smt-03"),
            "reset 后 SMT-03 卡应在: {v}"
        );
    }
}

/// RT-ADSNAP-04：全新库未播种时快照结构仍有效（不报错、不静默回退静态卡）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adsnap_04_empty_snapshot_valid_structure() {
    let (api, _rt) = boot().await;
    let (s, v) = api.get("/api/desktop/v1/snapshot").await;
    assert_eq!(s, 200, "{v}");
    assert!(v["cards"].is_array(), "{v}");
    assert!(v["counts"].is_object(), "{v}");
    assert!(
        !v["cursor"].as_str().unwrap_or_default().is_empty(),
        "cursor 非空: {v}"
    );
}
