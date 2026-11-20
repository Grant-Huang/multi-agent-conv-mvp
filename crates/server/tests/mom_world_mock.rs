//! World Adapter M0-1 Mock HTTP 合同：RT-WA-01～08，见 docs/testing/CASES.md。
use an_server::mom_world_mock::{router, WorldMockState};
use serde_json::{json, Value};

async fn mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = router(WorldMockState::new(true));
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
fn operator() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "operator".parse().unwrap());
    h
}
fn auditor() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "auditor".parse().unwrap());
    h
}

async fn save_ok(client: &reqwest::Client, url: &str) -> String {
    let r: Value = client
        .post(format!("{url}/api/world/mock/experiment/configs"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": 480,
            "request_id": "req-cfg"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    r["config_version"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn rt_wa_01_trial_zero_delivery_and_invalid_duration() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();

    let bad = client
        .post(format!("{url}/api/world/mock/experiment/configs"))
        .headers(experimenter())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": -1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    let bad_body: Value = bad.json().await.unwrap();
    assert_eq!(bad_body["code"], "INVALID_ARGUMENT");

    let cfg = save_ok(&client, &url).await;
    let trial: Value = client
        .post(format!("{url}/api/world/mock/experiment/trials"))
        .headers(experimenter())
        .json(&json!({ "config_version": cfg }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let trial_id = trial["trial_run_id"].as_str().unwrap();
    assert_eq!(trial["delivery_count"], 0);
    assert_eq!(trial["mode"], "trial");

    let run: Value = client
        .get(format!("{url}/api/world/mock/experiment/runs/{trial_id}"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run["delivery_count"], 0);
    assert_eq!(run["mode"], "trial");
}

#[tokio::test]
async fn rt_wa_02_official_start_idempotent_distinct_from_trial() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;

    let trial: Value = client
        .post(format!("{url}/api/world/mock/experiment/trials"))
        .headers(experimenter())
        .json(&json!({ "config_version": cfg }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let trial_id = trial["trial_run_id"].as_str().unwrap().to_string();

    let body = json!({
        "config_version": cfg,
        "target_channel": "ops-channel",
        "start_key": "start-1"
    });
    let first: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["official_run_id"], second["official_run_id"]);
    assert_eq!(second["already"], true);
    assert_ne!(first["official_run_id"], trial_id);
    assert_eq!(first["delivery_count"], 1);
    assert!(first["case_id"].as_str().is_some());
}

#[tokio::test]
async fn rt_wa_03_observe_subscribe_and_cursor_expiry() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;
    let official: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "start-cur"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = official["world_id"].as_str().unwrap();

    let obs: Value = client
        .post(format!("{url}/api/world/mock/observe"))
        .json(&json!({ "world_id": world_id, "selection": {} }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(obs["protocol_version"], 1);
    let cursor = obs["cursor"].as_str().unwrap();

    let sub: Value = client
        .post(format!("{url}/api/world/mock/subscribe"))
        .json(&json!({ "world_id": world_id, "cursor": cursor }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!sub["events"].as_array().unwrap().is_empty());
    let next = sub["next_cursor"].as_str().unwrap().to_string();

    let fault = client
        .post(format!("{url}/api/world/mock/test/fault"))
        .json(&json!({ "kind": "expire_cursor", "world_id": world_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(fault.status(), 200);

    let expired = client
        .post(format!("{url}/api/world/mock/subscribe"))
        .json(&json!({ "world_id": world_id, "cursor": next }))
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), 409);
    let body: Value = expired.json().await.unwrap();
    assert_eq!(body["code"], "CURSOR_EXPIRED");
}

#[tokio::test]
async fn rt_wa_04_propose_execute_idempotent_confirm() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;
    let official: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "start-exec"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = official["world_id"].as_str().unwrap();

    let prop: Value = client
        .post(format!("{url}/api/world/mock/propose"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": { "priority": "high" }
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let proposal_id = prop["proposal_id"].as_str().unwrap();

    let exec_body = json!({
        "world_id": world_id,
        "proposal_id": proposal_id,
        "approval_ref": "ops-approve-1",
        "idempotency_key": "idem-1"
    });
    let first: Value = client
        .post(format!("{url}/api/world/mock/execute"))
        .headers(operator())
        .json(&exec_body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{url}/api/world/mock/execute"))
        .headers(operator())
        .json(&exec_body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["execution_id"], second["execution_id"]);
    assert_eq!(second["already"], true);
    assert_eq!(first["protocol_version"], 1);

    let conf: Value = client
        .post(format!("{url}/api/world/mock/confirm"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "execution_id": first["execution_id"]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let status = conf["status"].as_str().unwrap();
    assert!(
        matches!(
            status,
            "accepted" | "applied" | "confirmed" | "unknown" | "failed"
        ),
        "{status}"
    );
}

#[tokio::test]
async fn rt_wa_05_cross_run_and_operator_experiment_denied() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;

    let a: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "A"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let b: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "B"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_a = a["world_id"].as_str().unwrap();
    let world_b = b["world_id"].as_str().unwrap();
    let run_a = a["official_run_id"].as_str().unwrap();

    let denied_ctrl = client
        .post(format!("{url}/api/world/mock/experiment/runs/{run_a}"))
        .headers(operator())
        .json(&json!({ "action": "pause" }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_ctrl.status(), 403);
    let denied_body: Value = denied_ctrl.json().await.unwrap();
    assert_eq!(denied_body["code"], "ACCESS_DENIED");

    let prop: Value = client
        .post(format!("{url}/api/world/mock/propose"))
        .json(&json!({
            "world_id": world_a,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": {}
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let before_a: Value = client
        .get(format!("{url}/api/world/mock/experiment/runs/{run_a}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let before_b: Value = client
        .get(format!(
            "{url}/api/world/mock/experiment/runs/{}",
            b["official_run_id"]
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let cross = client
        .post(format!("{url}/api/world/mock/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_b,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "from-a",
            "idempotency_key": "cross-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), 403);
    let cross_body: Value = cross.json().await.unwrap();
    assert_eq!(cross_body["code"], "ACCESS_DENIED");

    let after_a: Value = client
        .get(format!("{url}/api/world/mock/experiment/runs/{run_a}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let after_b: Value = client
        .get(format!(
            "{url}/api/world/mock/experiment/runs/{}",
            b["official_run_id"]
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before_a["action_count"], after_a["action_count"]);
    assert_eq!(before_b["action_count"], after_b["action_count"]);
    let _ = world_a;
}

#[tokio::test]
async fn rt_wa_06_lose_receipt_then_partial_fail() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;
    let official: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "fault"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = official["world_id"].as_str().unwrap();
    let prop: Value = client
        .post(format!("{url}/api/world/mock/propose"))
        .json(&json!({
            "world_id": world_id,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": {}
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let exec: Value = client
        .post(format!("{url}/api/world/mock/execute"))
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ap",
            "idempotency_key": "f1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let execution_id = exec["execution_id"].as_str().unwrap();

    client
        .post(format!("{url}/api/world/mock/test/fault"))
        .json(&json!({ "kind": "lose_receipt", "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let unknown: Value = client
        .post(format!("{url}/api/world/mock/confirm"))
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unknown["status"], "unknown");

    client
        .post(format!("{url}/api/world/mock/test/fault"))
        .json(&json!({ "kind": "partial_fail", "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let failed: Value = client
        .post(format!("{url}/api/world/mock/confirm"))
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(failed["status"], "failed");
    let sub = failed["evidence"].as_array().unwrap();
    assert!(sub.iter().any(|s| s["status"] == "applied"));
    assert!(sub.iter().any(|s| s["status"] == "failed"));
}

#[tokio::test]
async fn rt_wa_07_pause_wait_not_failed_then_step() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;
    let official: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "pause"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = official["official_run_id"].as_str().unwrap();
    let world_id = official["world_id"].as_str().unwrap();

    let paused: Value = client
        .post(format!("{url}/api/world/mock/experiment/runs/{run_id}"))
        .headers(experimenter())
        .json(&json!({ "action": "pause" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(paused["run_status"], "paused");
    assert_eq!(paused["failed"], false);
    assert!(paused["wait_label"].as_str().unwrap().contains("等待"));

    let prop: Value = client
        .post(format!("{url}/api/world/mock/propose"))
        .json(&json!({
            "world_id": world_id,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": {}
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let exec: Value = client
        .post(format!("{url}/api/world/mock/execute"))
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ap",
            "idempotency_key": "step-1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let stepped: Value = client
        .post(format!("{url}/api/world/mock/experiment/runs/{run_id}"))
        .headers(experimenter())
        .json(&json!({ "action": "step", "minutes": 1 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stepped["simulation_minute"], 1);

    let conf: Value = client
        .post(format!("{url}/api/world/mock/confirm"))
        .json(&json!({
            "world_id": world_id,
            "execution_id": exec["execution_id"]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(conf["status"], "applied");
}

#[tokio::test]
async fn rt_wa_08_ground_truth_auditor_only() {
    let (url, _task) = mock_server().await;
    let client = reqwest::Client::new();
    let cfg = save_ok(&client, &url).await;
    let official: Value = client
        .post(format!("{url}/api/world/mock/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": cfg,
            "target_channel": "ops-channel",
            "start_key": "truth"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = official["world_id"].as_str().unwrap();

    let denied = client
        .get(format!(
            "{url}/api/world/mock/ground-truth?world_id={world_id}"
        ))
        .headers(operator())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let ok: Value = client
        .get(format!(
            "{url}/api/world/mock/ground-truth?world_id={world_id}"
        ))
        .headers(auditor())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(ok["ground_truth"].is_object());
}
