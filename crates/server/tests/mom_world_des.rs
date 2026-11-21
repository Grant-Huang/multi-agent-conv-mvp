//! World Adapter M1 远程 DES 读合同：RT-WADES-01～04（见 docs/testing/CASES.md）。
//! Green：挂载 `/api/world/des`；不改 DesConnector / engine。

use an_server::mom_world_des::{router, WorldDesState};
use serde_json::{json, Value};

const DES_API: &str = "/api/world/des";

async fn des_server(connected: bool) -> (String, tokio::task::JoinHandle<()>) {
    let app = router(WorldDesState::new(connected));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}

async fn des_server_with_controls() -> (String, tokio::task::JoinHandle<()>) {
    let app = router(WorldDesState::with_test_controls(true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}

async fn bind_world(client: &reqwest::Client, url: &str) -> String {
    let bound: Value = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({ "factory": "plant-a", "scenario": "s1", "seed": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    bound["world_id"].as_str().unwrap().to_string()
}

fn experimenter() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "experimenter".parse().unwrap());
    h
}

/// RT-WADES-01：DesConnector 可用时绑定成功；未连接明确错误。
#[tokio::test]
async fn rt_wades_01_bind_world_after_des_connector_create() {
    let (url_off, _t0) = des_server(false).await;
    let client = reqwest::Client::new();

    let missing = client
        .post(format!("{url_off}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({ "run_id": "no-connector", "request_id": "bind-missing" }))
        .send()
        .await
        .unwrap();
    assert!(
        missing.status().is_client_error() || missing.status().as_u16() == 503,
        "未连接 DesConnector 时绑定须明确错误，got {}",
        missing.status()
    );

    let (url, _task) = des_server(true).await;
    let bound = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "request_id": "bind-ok"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bound.status(), 200);
    let body: Value = bound.json().await.unwrap();
    assert_eq!(body["protocol_version"], 1);
    assert_eq!(body["provenance"], "des");
    assert!(
        !body["world_id"].as_str().unwrap_or("").is_empty(),
        "绑定须返回 world_id"
    );
}

/// RT-WADES-02：tick 完成正式投递后 Observe 才见新事件。
#[tokio::test]
async fn rt_wades_02_observe_after_formal_delivery_boundary() {
    let (url, _task) = des_server(true).await;
    let client = reqwest::Client::new();

    let bound: Value = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({ "factory": "plant-a", "scenario": "s1", "seed": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = bound["world_id"].as_str().unwrap().to_string();

    let before: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(before["events"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));

    let tick = client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(tick.status(), 200);

    let after: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        after["events"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "投递边界之后 Observe 须见到新事件"
    );
    assert!(!after["cursor"].as_str().unwrap_or("").is_empty());
}

/// RT-WADES-03：Subscribe 续读无漏无重；游标过期 → CURSOR_EXPIRED。
#[tokio::test]
async fn rt_wades_03_subscribe_resume_and_cursor_expiry() {
    let (url, _task) = des_server(true).await;
    let client = reqwest::Client::new();

    let bound: Value = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({ "factory": "plant-a", "scenario": "s1", "seed": 2 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = bound["world_id"].as_str().unwrap().to_string();

    client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let obs: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let cursor = obs["cursor"].as_str().unwrap().to_string();
    let first_ids: Vec<String> = obs["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_id"].as_str().unwrap().to_string())
        .collect();

    let sub: Value = client
        .post(format!("{url}{DES_API}/subscribe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "cursor": cursor }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let sub_ids: Vec<String> = sub["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_id"].as_str().unwrap().to_string())
        .collect();
    for id in &sub_ids {
        assert!(
            !first_ids.contains(id),
            "续读不得重复已 Observe 的事件 {id}"
        );
    }
    assert!(!sub["cursor"].as_str().unwrap_or("").is_empty());

    let expired = client
        .post(format!("{url}{DES_API}/subscribe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "cursor": "expired-cursor" }))
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), 409);
    let err: Value = expired.json().await.unwrap();
    let code = err["code"].as_str().unwrap_or("");
    assert!(
        code.contains("CURSOR") || code.contains("EXPIRED"),
        "错误码须标识游标过期，got {code:?}"
    );
}

/// RT-WADES-04：试跑零投递；正式运行投递 ≥1 且有 case_id。
#[tokio::test]
async fn rt_wades_04_trial_zero_delivery_official_delivers() {
    let (url, _task) = des_server(true).await;
    let client = reqwest::Client::new();

    let trial = client
        .post(format!("{url}{DES_API}/experiment/trials"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": 60,
            "request_id": "trial-green"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(trial.status(), 200);
    let trial_body: Value = trial.json().await.unwrap();
    assert_eq!(trial_body["delivery_count"], 0);
    assert_eq!(trial_body["mode"], "trial");
    let trial_id = trial_body["trial_run_id"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(!trial_id.is_empty());

    let official = client
        .post(format!("{url}{DES_API}/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": trial_body["config_version"],
            "target_channel": "ops-channel",
            "start_key": "official-green"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(official.status(), 200);
    let off: Value = official.json().await.unwrap();
    assert!(off["delivery_count"].as_u64().unwrap_or(0) >= 1);
    assert!(!off["case_id"].as_str().unwrap_or("").is_empty());
    let official_id = off["official_run_id"].as_str().unwrap_or("");
    assert!(!official_id.is_empty());
    assert_ne!(trial_id, official_id);
}

/// RT-WADES-05：投递中断后仍服务最近完整边界；pending 不可见。
#[tokio::test]
async fn rt_wades_05_interrupt_keeps_last_complete_boundary() {
    let (url, _task) = des_server_with_controls().await;
    let client = reqwest::Client::new();
    let world_id = bind_world(&client, &url).await;

    client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let after_ok: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let seq0 = after_ok["publish_seq"].as_u64().unwrap();
    let n0 = after_ok["events"].as_array().unwrap().len();
    assert!(n0 >= 1);
    assert!(seq0 >= 1);

    client
        .post(format!("{url}{DES_API}/test/abort-before-publish"))
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let aborted: Value = client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(aborted["published"], false);

    let mid: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(mid["publish_seq"], seq0);
    assert_eq!(mid["events"].as_array().unwrap().len(), n0);
    assert_eq!(mid["objects"][0]["state"]["publish_seq"], seq0);

    let committed: Value = client
        .post(format!("{url}{DES_API}/test/commit-publish"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(committed["publish_seq"].as_u64().unwrap() > seq0);

    let after: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(after["events"].as_array().unwrap().len() > n0);
    assert_eq!(after["publish_seq"], committed["publish_seq"]);
    assert_eq!(
        after["objects"][0]["state"]["event_count"]
            .as_u64()
            .unwrap() as usize,
        after["events"].as_array().unwrap().len()
    );
}

/// RT-WADES-06：快照与事件同一 publish_seq；重复 event_id 不双发。
#[tokio::test]
async fn rt_wades_06_snapshot_aligns_and_event_ids_dedupe() {
    let (url, _task) = des_server_with_controls().await;
    let client = reqwest::Client::new();
    let world_id = bind_world(&client, &url).await;

    client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let obs: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!obs["as_of"].as_str().unwrap_or("").is_empty());
    assert_eq!(
        obs["publish_seq"],
        obs["objects"][0]["state"]["publish_seq"]
    );
    let ids: Vec<&str> = obs["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event_id"].as_str().unwrap())
        .collect();
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(ids.len(), uniq.len(), "Observe 不得含重复 event_id");

    // 空 pending 再 commit：水位与事件集合均不变
    let seq = obs["publish_seq"].as_u64().unwrap();
    let empty: Value = client
        .post(format!("{url}{DES_API}/test/commit-publish"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty["publish_seq"], seq);
    let again: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(again["events"].as_array().unwrap().len(), ids.len());
    assert_eq!(again["publish_seq"], seq);
}
