//! 运营桌面服务端接口（D1-2），用例 RT-OPSDESK-10，见 docs/testing/CASES.md。
use an_runtime::mom::ops_desk::{OpsConfig, OpsPublisher, OPS_CHANNEL_ID};
use an_runtime::mom::Engine;
use an_runtime::store::Store;
use an_server::{mom, mom_ops};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

async fn ops_server() -> (String, tokio::task::JoinHandle<()>, Arc<Store>) {
    let engine = Arc::new(Mutex::new(Engine::memory().unwrap()));
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let publisher = Arc::new(Mutex::new(
        OpsPublisher::memory(
            store.clone(),
            OpsConfig {
                merge_secs: 0,
                hourly_cap: 10,
            },
        )
        .unwrap(),
    ));
    let mom_state = mom::AppState::scripted(engine.clone());
    let app = axum::Router::new()
        .merge(mom_ops::router(mom_ops::OpsState { engine, publisher }))
        .nest("/mom", mom::router(mom_state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task, store)
}

#[tokio::test]
async fn rt_opsdesk_10_desk_api_matches_engine_and_open_is_idempotent() {
    let (url, task, _store) = ops_server().await;
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{url}/mom/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["case_id"].as_str().expect("case_id").to_string();

    let list: Value = client
        .get(format!("{url}/mom/api/mom/cases"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mom_ids: Vec<String> = list["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["case_id"].as_str().unwrap().to_string())
        .collect();
    assert!(mom_ids.contains(&id), "{mom_ids:?}");

    let desk_resp = client
        .get(format!("{url}/api/mom/desk"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        desk_resp.status(),
        200,
        "{}",
        desk_resp.text().await.unwrap()
    );
    let desk: Value = desk_resp.json().await.unwrap();
    let desk_ids: Vec<String> = desk["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["case_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        {
            let mut a = mom_ids.clone();
            a.sort();
            a
        },
        {
            let mut b = desk_ids.clone();
            b.sort();
            b
        },
        "桌面 rows 与 /mom/api/mom/cases 案例集合相同"
    );
    assert_eq!(desk["mode"], "simulation");

    let detail = client
        .get(format!("{url}/api/mom/desk/cases/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), 200, "{}", detail.text().await.unwrap());
    let detail: Value = detail.json().await.unwrap();
    assert_eq!(detail["case_id"], id);
    assert!(detail.get("projection_seq").is_some(), "{detail}");

    let missing = client
        .get(format!("{url}/api/mom/desk/cases/no-such-case"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    let missing_body = missing.text().await.unwrap();
    assert!(
        missing_body.contains("no-such-case"),
        "404 正文应含案例 id：{missing_body}"
    );

    let open1 = client
        .post(format!("{url}/api/mom/desk/cases/{id}/open-in-channel"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(open1.status(), 200, "{}", open1.text().await.unwrap());
    let open1: Value = open1.json().await.unwrap();
    let mid = open1["message_id"].as_str().unwrap().to_string();
    assert!(!mid.is_empty(), "message_id 非空");
    assert_eq!(open1["conversation_id"], OPS_CHANNEL_ID);

    let open2 = client
        .post(format!("{url}/api/mom/desk/cases/{id}/open-in-channel"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(open2.status(), 200);
    let open2: Value = open2.json().await.unwrap();
    assert_eq!(open2["message_id"], mid, "open-in-channel 幂等");

    let open_missing = client
        .post(format!(
            "{url}/api/mom/desk/cases/no-such-case/open-in-channel"
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(open_missing.status(), 404);

    task.abort();
}

#[tokio::test]
async fn rt_opsdesk_11_approve_is_idempotent_and_rejects_stale() {
    let (url, task, store) = ops_server().await;
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{url}/mom/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["case_id"].as_str().unwrap().to_string();
    let view: Value = client
        .get(format!("{url}/mom/api/mom/cases/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rev = view["plan_revision"].as_u64().unwrap();

    // 先发卡，批准后能立刻看到 approver
    let open = client
        .post(format!("{url}/api/mom/desk/cases/{id}/open-in-channel"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(open.status(), 200, "{}", open.text().await.unwrap());

    let a1 = client
        .post(format!("{url}/api/mom/desk/cases/{id}/approve"))
        .json(&json!({"scope":"production","plan_revision":rev}))
        .send()
        .await
        .unwrap();
    assert_eq!(a1.status(), 200, "{}", a1.text().await.unwrap());
    let a1: Value = a1.json().await.unwrap();
    assert_eq!(a1["already"], false);
    assert_eq!(a1["approver"], "supervisor");

    let a2 = client
        .post(format!("{url}/api/mom/desk/cases/{id}/approve"))
        .json(&json!({"scope":"production","plan_revision":rev}))
        .send()
        .await
        .unwrap();
    assert_eq!(a2.status(), 200);
    let a2: Value = a2.json().await.unwrap();
    assert_eq!(a2["already"], true);
    assert_eq!(a2["approver"], "supervisor");

    let card = store
        .list_message_views(OPS_CHANNEL_ID, None)
        .unwrap()
        .into_iter()
        .find_map(|m| m.card)
        .expect("频道应有案例卡");
    match card {
        an_core::ChatCard::OpsCase {
            approver,
            scope,
            plan_revision,
            ..
        } => {
            assert_eq!(approver.as_deref(), Some("supervisor"));
            assert_eq!(scope.as_deref(), Some("production"));
            assert_eq!(plan_revision, Some(rev));
        }
        other => panic!("应是 OpsCase：{other:?}"),
    }

    let stale = client
        .post(format!("{url}/api/mom/desk/cases/{id}/approve"))
        .json(&json!({"scope":"production","plan_revision":rev + 99}))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409, "{}", stale.text().await.unwrap());

    let missing = client
        .post(format!("{url}/api/mom/desk/cases/no-such-case/approve"))
        .json(&json!({"scope":"production","plan_revision":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    task.abort();
}
