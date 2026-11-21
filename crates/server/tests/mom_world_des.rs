//! World Adapter M1 远程 DES 读合同：RT-WADES-01～04（见 docs/testing/CASES.md）。
//! M1-1 Red：`/api/world/des` 尚未挂载；业务断言应失败，直到接线 Green。
//! 本文件不改 DesConnector / engine；仅冻结合同。

use serde_json::{json, Value};

/// 远程 DES 世界适配前缀（相对 M0 `/api/world/mock`）。
const DES_API: &str = "/api/world/des";

async fn bare_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}

fn experimenter() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "experimenter".parse().unwrap());
    h
}

/// RT-WADES-01：DesConnector 创建运行后世界绑定可读；未连接明确错误。
#[tokio::test]
async fn rt_wades_01_bind_world_after_des_connector_create() {
    let (url, _task) = bare_server().await;
    let client = reqwest::Client::new();

    let missing = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({ "run_id": "no-connector", "request_id": "bind-missing" }))
        .send()
        .await
        .unwrap();
    assert!(
        missing.status().is_client_error(),
        "未连接 DesConnector 时绑定须明确错误，got {}",
        missing.status()
    );

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
    assert_eq!(
        bound.status(),
        200,
        "M1-1 Red：远程 DES 绑定合同尚未挂载（期望 200 + provenance=des）"
    );
    let body: Value = bound.json().await.unwrap();
    assert_eq!(body["protocol_version"], 1);
    assert_eq!(body["provenance"], "des");
    assert!(
        !body["world_id"].as_str().unwrap_or("").is_empty(),
        "绑定须返回 world_id"
    );
}

/// RT-WADES-02：tick 完成 MOM 投递后 Observe 才见新事件。
#[tokio::test]
async fn rt_wades_02_observe_after_formal_delivery_boundary() {
    let (url, _task) = bare_server().await;
    let client = reqwest::Client::new();
    let world_id = "des-world-red";

    let before = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(before.status(), 200, "M1-1 Red：Observe 合同尚未挂载");

    let tick = client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(tick.status(), 200, "M1-1 Red：远程 step/tick 尚未挂载");

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
    let (url, _task) = bare_server().await;
    let client = reqwest::Client::new();
    let world_id = "des-world-red";

    let sub = client
        .post(format!("{url}{DES_API}/subscribe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "cursor": "0" }))
        .send()
        .await
        .unwrap();
    assert_eq!(sub.status(), 200, "M1-1 Red：Subscribe 合同尚未挂载");
    let ok: Value = sub.json().await.unwrap();
    let cursor = ok["cursor"].as_str().unwrap_or("").to_string();
    assert!(!cursor.is_empty());

    let expired = client
        .post(format!("{url}{DES_API}/subscribe"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "cursor": "expired-cursor" }))
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), 409, "过期游标须 CURSOR_EXPIRED / 409");
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
    let (url, _task) = bare_server().await;
    let client = reqwest::Client::new();

    let trial = client
        .post(format!("{url}{DES_API}/experiment/trials"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": 60,
            "request_id": "trial-red"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(trial.status(), 200, "M1-1 Red：远程试跑合同尚未挂载");
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
            "start_key": "official-red"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(official.status(), 200, "M1-1 Red：远程正式启动合同尚未挂载");
    let off: Value = official.json().await.unwrap();
    assert!(off["delivery_count"].as_u64().unwrap_or(0) >= 1);
    assert!(!off["case_id"].as_str().unwrap_or("").is_empty());
    let official_id = off["official_run_id"].as_str().unwrap_or("");
    assert!(!official_id.is_empty());
    assert_ne!(trial_id, official_id);
}
