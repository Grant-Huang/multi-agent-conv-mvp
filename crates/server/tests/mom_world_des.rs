//! World Adapter M1/T-50 远程 DES 读合同：RT-WADES-01～08（见 docs/testing/CASES.md）。
//! 桩模式 + 真 `DesConnector`/`with_des` 补接；不改 DesConnector / engine 内部。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::ChatCard;
use an_runtime::config::MomSection;
use an_runtime::mom::ops_desk::OPS_CHANNEL_ID;
use an_runtime::mom::Engine;
use an_runtime::store::Store;
use an_server::mom_des_connector::DesConnector;
use an_server::mom_world_des::{router, WorldDesState};
use serde_json::{json, Value};

#[path = "support/des_mcp.rs"]
mod des_mcp;
use des_mcp::des_mcp_bin;

const DES_API: &str = "/api/world/des";
const WAIT: Duration = Duration::from_secs(60);

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

async fn des_server_with_ops(connected: bool) -> (String, Arc<Store>, tokio::task::JoinHandle<()>) {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let app = router(WorldDesState::with_app_store(connected, store.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, store, task)
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

/// RT-WADES-07：试跑不向运营频道投递事项卡。
#[tokio::test]
async fn rt_wades_07_trial_does_not_deliver_ops_channel() {
    let (url, store, _task) = des_server_with_ops(true).await;
    let client = reqwest::Client::new();

    let trial = client
        .post(format!("{url}{DES_API}/experiment/trials"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": 60,
            "request_id": "trial-ops-iso"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(trial.status(), 200);
    let trial_body: Value = trial.json().await.unwrap();
    assert_eq!(trial_body["delivery_count"], 0);
    assert_eq!(trial_body["mode"], "trial");

    let msgs = store
        .list_message_views(OPS_CHANNEL_ID, None)
        .unwrap_or_default();
    assert!(
        msgs.is_empty(),
        "试跑不得向运营频道投影，got {} 条",
        msgs.len()
    );
}

/// T-50：默认配置不挂真连接器命令。
#[test]
fn rt_wades_des_mcp_default_empty() {
    assert!(
        MomSection::default().des_mcp.is_empty(),
        "生产默认 des_mcp 须为空"
    );
}

async fn live_des_server() -> (String, Arc<DesConnector>, tokio::task::JoinHandle<()>) {
    let connector = Arc::new(
        tokio::time::timeout(
            WAIT,
            DesConnector::spawn(&des_mcp_bin(), &[], "wa-des-test-admin"),
        )
        .await
        .expect("spawn timed out")
        .expect("an-des-mcp starts"),
    );
    let engine = Arc::new(Mutex::new(Engine::memory().unwrap()));
    let app = router(WorldDesState::with_connector(
        connector.clone(),
        engine,
        None,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, connector, task)
}

/// RT-WADES-01（真连接器）：`with_des` 路径 bind 成功；`world_id` 即仿真编号。
#[tokio::test]
async fn rt_wades_01_bind_with_live_des_connector() {
    let (url, _connector, _task) = live_des_server().await;
    let client = reqwest::Client::builder().timeout(WAIT).build().unwrap();
    let bound = client
        .post(format!("{url}{DES_API}/bind"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "request_id": "bind-live"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bound.status(), 200);
    let body: Value = bound.json().await.unwrap();
    assert_eq!(body["protocol_version"], 1);
    assert_eq!(body["provenance"], "des");
    let world_id = body["world_id"].as_str().unwrap_or("");
    assert!(
        world_id.starts_with("DES-"),
        "真连接器 world_id 应对齐 simulation_id，got {world_id}"
    );
}

/// RT-WADES-02（真连接器）：tick 后 Observe 见 Engine 投影事件，`provenance=des`。
#[tokio::test]
async fn rt_wades_02_observe_after_live_connector_tick() {
    let (url, _connector, _task) = live_des_server().await;
    let client = reqwest::Client::builder().timeout(WAIT).build().unwrap();
    let world_id = bind_world(&client, &url).await;
    assert!(world_id.starts_with("DES-"));

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
    let tick_body: Value = tick.json().await.unwrap();
    assert_eq!(tick_body["published"], true);
    assert_eq!(tick_body["provenance"], "des");

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
    assert_eq!(after["provenance"], "des");
    assert!(
        after["events"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "tick 成功后 Observe 须见 Engine 投影事件"
    );
    assert!(after["publish_seq"].as_u64().unwrap_or(0) >= 1);
}

/// 真连接器拒用半提交测试控件。
#[tokio::test]
async fn rt_wades_live_rejects_abort_before_publish() {
    let (url, _connector, _task) = live_des_server().await;
    let client = reqwest::Client::builder().timeout(WAIT).build().unwrap();
    let world_id = bind_world(&client, &url).await;
    let denied = client
        .post(format!("{url}{DES_API}/test/abort-before-publish"))
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let err: Value = denied.json().await.unwrap();
    assert_eq!(err["code"], "ACCESS_DENIED");
}

/// RT-WADES-08：正式运行向运营频道投递 OpsCase（provenance=des）。
#[tokio::test]
async fn rt_wades_08_official_delivers_ops_case_to_channel() {
    let (url, store, _task) = des_server_with_ops(true).await;
    let client = reqwest::Client::new();

    let trial: Value = client
        .post(format!("{url}{DES_API}/experiment/trials"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 2,
            "duration_minutes": 30,
            "request_id": "trial-before-official"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let official = client
        .post(format!("{url}{DES_API}/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": trial["config_version"],
            "target_channel": "ops-channel",
            "start_key": "official-ops-deliver"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(official.status(), 200);
    let off: Value = official.json().await.unwrap();
    assert!(off["delivery_count"].as_u64().unwrap_or(0) >= 1);
    let case_id = off["case_id"].as_str().unwrap_or("").to_string();
    assert!(!case_id.is_empty());

    let msgs = store
        .list_message_views(OPS_CHANNEL_ID, None)
        .expect("运营频道应已创建");
    let card = msgs
        .into_iter()
        .find_map(|m| m.card)
        .expect("正式运行须投递 OpsCase");
    match card {
        ChatCard::OpsCase {
            case_id: cid,
            provenance,
            stage,
            ..
        } => {
            assert_eq!(cid, case_id);
            assert_eq!(provenance, "des");
            assert_eq!(stage, "WAITING_HUMAN");
        }
        other => panic!("应是 OpsCase，got {other:?}"),
    }
}
