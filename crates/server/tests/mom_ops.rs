//! 运营桌面服务端接口（D1-2），用例 RT-OPSDESK-10，见 docs/testing/CASES.md。
use an_runtime::mom::ops_desk::{OpsConfig, OpsPublisher, OPS_CHANNEL_ID};
use an_runtime::mom::Engine;
use an_runtime::store::Store;
use an_server::{mom, mom_ops};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

async fn ops_server() -> (String, tokio::task::JoinHandle<()>) {
    let engine = Arc::new(Mutex::new(Engine::memory().unwrap()));
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let publisher = Arc::new(Mutex::new(
        OpsPublisher::memory(
            store,
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
    (url, task)
}

#[tokio::test]
async fn rt_opsdesk_10_desk_api_matches_engine_and_open_is_idempotent() {
    let (url, task) = ops_server().await;
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
