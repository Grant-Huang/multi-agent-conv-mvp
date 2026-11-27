//! RT-ADLINK / RT-ADPREF：B3 关联持久与基础配置。用例见 docs/testing/CASES.md。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use an_core::{PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskRecord, TaskStatus};
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

fn sample_task(id: &str, conv: &str) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        conversation_id: conv.into(),
        goal: "SMT-03".into(),
        role: Role::General,
        status: TaskStatus::Running,
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: Role::General,
            required_tools: vec![],
            risk_level: Risk::Low,
            reason: "test".into(),

            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: PolicyVerdict {
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
        created_at: an_core::now(),
        updated_at: an_core::now(),
    }
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
    async fn delete(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .delete(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn patch(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .patch(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

async fn boot() -> (Api, Arc<Runtime>, String, String) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-b3"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let conv = rt.store.create_conversation("B3 关联测").unwrap();
    let task = sample_task("TASK-2026-018", &conv.id);
    rt.store.insert_task(&task, "", None).unwrap();
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (api, rt, conv.id, task.id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlink_01_post_creates_link_and_replays() {
    let (api, rt, conv_id, task_id) = boot().await;
    let path = format!("/api/desktop/v1/tasks/{task_id}/links");
    let body = json!({
        "client_operation_id": "op-link-1",
        "conversation_id": conv_id,
        "source_message_ids": ["msg-a"]
    });
    let (s1, v1) = api.post(&path, body.clone()).await;
    assert_eq!(s1, 201, "{v1}");
    assert_eq!(v1["replayed"], false);
    let link_id = v1["link"]["id"].as_str().unwrap().to_string();
    assert!(link_id.starts_with("dlink"));
    assert_eq!(v1["link"]["conversation_id"], conv_id);
    assert_eq!(v1["link"]["projected"], false);

    let (s2, v2) = api.post(&path, body).await;
    assert_eq!(s2, 201, "{v2}");
    assert_eq!(v2["replayed"], true);
    assert_eq!(v2["link"]["id"], link_id);
    let links = rt.store.list_desktop_task_links(&task_id, true).unwrap();
    let explicit = links.iter().filter(|l| !l.projected).count();
    assert_eq!(explicit, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlink_02_same_op_id_changed_conversation_conflicts() {
    let (api, rt, conv_id, task_id) = boot().await;
    let other = rt.store.create_conversation("另一会话").unwrap();
    let path = format!("/api/desktop/v1/tasks/{task_id}/links");
    let (s1, v1) = api
        .post(
            &path,
            json!({
                "client_operation_id": "op-conflict",
                "conversation_id": conv_id
            }),
        )
        .await;
    assert_eq!(s1, 201, "{v1}");
    let (s2, v2) = api
        .post(
            &path,
            json!({
                "client_operation_id": "op-conflict",
                "conversation_id": other.id
            }),
        )
        .await;
    assert_eq!(s2, 409, "{v2}");
    assert_eq!(v2["code"], "operation_conflict");
    assert_eq!(v2["link_id"], v1["link"]["id"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlink_03_revoke_is_idempotent_and_hidden_by_default() {
    let (api, _rt, conv_id, task_id) = boot().await;
    let path = format!("/api/desktop/v1/tasks/{task_id}/links");
    let (s1, v1) = api
        .post(
            &path,
            json!({
                "client_operation_id": "op-revoke",
                "conversation_id": conv_id
            }),
        )
        .await;
    assert_eq!(s1, 201, "{v1}");
    let link_id = v1["link"]["id"].as_str().unwrap();
    let del = format!("/api/desktop/v1/tasks/{task_id}/links/{link_id}");
    let (d1, r1) = api.delete(&del).await;
    assert_eq!(d1, 200, "{r1}");
    assert!(r1["link"]["revoked_at"].as_str().is_some());
    let (d2, r2) = api.delete(&del).await;
    assert_eq!(d2, 200, "{r2}");
    assert_eq!(r2["link"]["revoked_at"], r1["link"]["revoked_at"]);

    let (g, listed) = api.get(&path).await;
    assert_eq!(g, 200, "{listed}");
    let active = listed["links"].as_array().unwrap();
    assert!(
        active.iter().all(|l| l["id"] != link_id),
        "默认列表不含已撤回: {listed}"
    );
    let (g2, all) = api.get(&format!("{path}?include_revoked=true")).await;
    assert_eq!(g2, 200, "{all}");
    assert!(all["links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["id"] == link_id && l["revoked_at"].as_str().is_some()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adlink_04_legacy_owner_projected_survives_restart() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-b3-legacy"));
    std::fs::create_dir_all(&dir).unwrap();
    let db: PathBuf = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let conv = rt.store.create_conversation("归属会话").unwrap();
    let task = sample_task("t-legacy", &conv.id);
    rt.store.insert_task(&task, "", None).unwrap();
    let base = serve(an_server::api(rt.clone())).await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let (s, v) = {
        let r = http
            .get(format!("{base}/api/desktop/v1/tasks/t-legacy/links"))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap())
    };
    assert_eq!(s, 200, "{v}");
    let links = v["links"].as_array().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["projected"], true);
    assert_eq!(links[0]["conversation_id"], conv.id);
    drop(rt);

    let rt2 = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let base2 = serve(an_server::api(rt2)).await;
    let r2 = http
        .get(format!("{base2}/api/desktop/v1/tasks/t-legacy/links"))
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status().as_u16(), 200);
    let v2: Value = r2.json().await.unwrap();
    assert_eq!(v2["links"][0]["projected"], true);
    assert_eq!(v2["links"][0]["conversation_id"], conv.id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adpref_01_get_returns_defaults_with_version() {
    let (api, _, _, _) = boot().await;
    let (s, v) = api.get("/api/desktop/v1/preferences").await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["version"], 1);
    assert_eq!(v["user_id"], "local");
    assert!(v["name"].as_str().unwrap().contains("助理"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adpref_02_patch_increments_version() {
    let (api, rt, _, _) = boot().await;
    let (s, v) = api
        .patch(
            "/api/desktop/v1/preferences",
            json!({
                "expected_version": 1,
                "name": "桌面助理"
            }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["version"], 2);
    assert_eq!(v["name"], "桌面助理");
    let stored = rt.store.get_desktop_preferences("local").unwrap();
    assert_eq!(stored.version, 2);
    assert_eq!(stored.name, "桌面助理");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adpref_03_stale_version_conflicts_without_overwrite() {
    let (api, rt, _, _) = boot().await;
    let (s1, v1) = api
        .patch(
            "/api/desktop/v1/preferences",
            json!({ "expected_version": 1, "name": "V2名称" }),
        )
        .await;
    assert_eq!(s1, 200, "{v1}");
    let (s2, v2) = api
        .patch(
            "/api/desktop/v1/preferences",
            json!({ "expected_version": 1, "name": "应被拒绝" }),
        )
        .await;
    assert_eq!(s2, 409, "{v2}");
    assert_eq!(v2["code"], "version_conflict");
    assert_eq!(v2["current"]["name"], "V2名称");
    assert_eq!(v2["current"]["version"], 2);
    let stored = rt.store.get_desktop_preferences("local").unwrap();
    assert_eq!(stored.name, "V2名称");
    assert_eq!(stored.version, 2);
}
