//! World Adapter M2 远程 DES 写合同：RT-WADES-09～14（见 docs/testing/CASES.md）。
//! 服务层优先；不改 DesConnector / engine 内部。

use an_server::mom_world_des::{router, WorldDesState};
use serde_json::{json, Value};

const DES_API: &str = "/api/world/des";

async fn des_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = router(WorldDesState::with_test_controls(true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}

fn operator() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "operator".parse().unwrap());
    h
}

fn experimenter() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "experimenter".parse().unwrap());
    h
}

fn auditor() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-wa-role", "auditor".parse().unwrap());
    h
}

async fn bind(client: &reqwest::Client, url: &str) -> String {
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

async fn propose_ok(client: &reqwest::Client, url: &str, world_id: &str, rev: &str) -> Value {
    client
        .post(format!("{url}{DES_API}/propose"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": { "priority": "high" },
            "plan_revision": rev
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// RT-WADES-09：Propose 绑定 plan_revision；修订变化后旧批准不可执行。
#[tokio::test]
async fn rt_wades_09_propose_binds_plan_revision() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;

    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    assert!(!prop["proposal_id"].as_str().unwrap().is_empty());
    assert_eq!(prop["plan_revision"], "rev-1");
    assert!(!prop["action_digest"].as_str().unwrap().is_empty());
    assert_eq!(prop["provenance"], "des");

    let bump = client
        .post(format!("{url}{DES_API}/test/bump-plan-revision"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "plan_revision": "rev-2" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bump.status().as_u16(), 200);

    let stale = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-1",
            "idempotency_key": "idem-stale",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        stale.status().as_u16() == 409 || stale.status().as_u16() == 400,
        "got {}",
        stale.status()
    );
    let body: Value = stale.json().await.unwrap();
    let code = body["code"].as_str().unwrap_or("");
    assert!(
        code == "APPROVAL_INVALID" || code == "PRECONDITION_FAILED",
        "{body}"
    );
}

/// RT-WADES-10：同幂等键只执行一次；键同载荷异 → IDEMPOTENCY_CONFLICT。
#[tokio::test]
async fn rt_wades_10_execute_idempotent() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;
    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;

    let exec_body = json!({
        "world_id": world_id,
        "proposal_id": prop["proposal_id"],
        "approval_ref": "ops-1",
        "idempotency_key": "idem-1",
        "plan_revision": "rev-1"
    });
    let first: Value = client
        .post(format!("{url}{DES_API}/execute"))
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
        .post(format!("{url}{DES_API}/execute"))
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

    let conflict = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-OTHER",
            "idempotency_key": "idem-1",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status().as_u16(), 409);
    let body: Value = conflict.json().await.unwrap();
    assert_eq!(body["code"], "IDEMPOTENCY_CONFLICT");
}

/// RT-WADES-11：取消后待发不发；迟到回执只更新证据，案例终态保留。
#[tokio::test]
async fn rt_wades_11_cancel_pending_not_dispatched() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();

    let official: Value = client
        .post(format!("{url}{DES_API}/experiment/official"))
        .headers(experimenter())
        .json(&json!({
            "config_version": "cfg-1",
            "target_channel": "ops-channel",
            "start_key": "c1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = official["world_id"].as_str().unwrap().to_string();
    let case_id = official["case_id"].as_str().unwrap().to_string();

    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    let cancel = client
        .post(format!("{url}{DES_API}/test/cancel-case"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "case_id": case_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status().as_u16(), 200);

    let exec: Value = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-1",
            "idempotency_key": "idem-cancel",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exec["dispatched"], false);

    let flush: Value = client
        .post(format!("{url}{DES_API}/test/flush-pending"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "case_id": case_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(flush["case_terminal"], "CANCELLED");
    assert_eq!(flush["dispatched_count"], 0);
}

/// RT-WADES-12：丢回执 Confirm 为 unknown；恢复后同键确认；部分失败含成功子动作。
#[tokio::test]
async fn rt_wades_12_confirm_after_lost_receipt() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;
    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    let exec: Value = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-1",
            "idempotency_key": "idem-recv",
            "plan_revision": "rev-1"
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
        .post(format!("{url}{DES_API}/test/lose-receipt"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let unknown: Value = client
        .post(format!("{url}{DES_API}/confirm"))
        .headers(operator())
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
        .post(format!("{url}{DES_API}/test/restore-receipt"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let confirmed: Value = client
        .post(format!("{url}{DES_API}/confirm"))
        .headers(operator())
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(confirmed["execution_id"], execution_id);
    assert!(
        confirmed["status"] == "confirmed" || confirmed["status"] == "applied",
        "{confirmed}"
    );

    let prop2 = propose_ok(&client, &url, &world_id, "rev-1").await;
    let exec2: Value = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop2["proposal_id"],
            "approval_ref": "ops-2",
            "idempotency_key": "idem-partial",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .post(format!("{url}{DES_API}/test/partial-fail"))
        .headers(experimenter())
        .json(&json!({
            "world_id": world_id,
            "execution_id": exec2["execution_id"]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let failed: Value = client
        .post(format!("{url}{DES_API}/confirm"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "execution_id": exec2["execution_id"]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(failed["status"], "failed");
    let subs = failed["subactions"].as_array().unwrap();
    assert!(subs.iter().any(|s| s["status"] == "applied"));
    assert!(subs.iter().any(|s| s["status"] == "failed"));
}

/// RT-WADES-13：工厂实例变更后旧运行提交失败可见。
#[tokio::test]
async fn rt_wades_13_factory_instance_change_rejects_stale() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;
    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;

    let restart = client
        .post(format!("{url}{DES_API}/test/factory-restart"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(restart.status().as_u16(), 200);

    let stale = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-1",
            "idempotency_key": "idem-restart",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        stale.status().as_u16() >= 400,
        "须失败可见，got {}",
        stale.status()
    );
    let body: Value = stale.json().await.unwrap();
    assert_eq!(body["code"], "FACTORY_INSTANCE_STALE");
}

/// RT-WADES-14：干预后前缀不变、未来改变；运营员无标准答案，审核员可见。
#[tokio::test]
async fn rt_wades_14_intervention_changes_future_events() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;

    client
        .post(format!("{url}{DES_API}/experiment/step"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id, "minutes": 10 }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let before: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(operator())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let prefix: Vec<String> = before["events"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|e| e["event_id"].as_str().map(str::to_string))
        .collect();

    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-dyn",
            "idempotency_key": "idem-dyn",
            "plan_revision": "rev-1"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let after: Value = client
        .post(format!("{url}{DES_API}/observe"))
        .headers(operator())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let after_ids: Vec<String> = after["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event_id"].as_str().map(str::to_string))
        .collect();
    for id in &prefix {
        assert!(after_ids.contains(id), "前缀丢失 {id}");
    }
    assert!(
        after_ids.len() > prefix.len(),
        "未来事件应改变: before={} after={}",
        prefix.len(),
        after_ids.len()
    );
    let blob = after.to_string();
    assert!(
        !blob.contains("ground_truth") && !blob.contains("标准答案"),
        "运营 Observe 不得含标准答案"
    );

    let denied = client
        .post(format!("{url}{DES_API}/ground_truth"))
        .headers(operator())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 403);

    let truth: Value = client
        .post(format!("{url}{DES_API}/ground_truth"))
        .headers(auditor())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(truth.get("ground_truth").is_some());
}

/// RT-WADES-18：干预后审核答案与干预一致；运营 Confirm/Observe 无标准答案。
#[tokio::test]
async fn rt_wades_18_answer_matches_intervention() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;
    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    let exec: Value = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-ans",
            "idempotency_key": "idem-ans",
            "plan_revision": "rev-1"
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

    let confirmed: Value = client
        .post(format!("{url}{DES_API}/confirm"))
        .headers(operator())
        .json(&json!({ "world_id": world_id, "execution_id": execution_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let blob = confirmed.to_string();
    assert!(
        !blob.contains("ground_truth") && !blob.contains("标准答案"),
        "Confirm 不得含标准答案"
    );

    let truth: Value = client
        .post(format!("{url}{DES_API}/ground_truth"))
        .headers(auditor())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let gt = &truth["ground_truth"];
    assert_eq!(gt["sync_status"], "synced");
    assert_eq!(gt["execution_id"], execution_id);
    assert_eq!(gt["intervention_id"], execution_id);
    assert_eq!(gt["answer"], "maintenance-applied");
}

/// RT-WADES-19：答案同步失败后独立补偿重试，恢复与干预一致的副本。
#[tokio::test]
async fn rt_wades_19_answer_sync_retry_after_failure() {
    let (url, _task) = des_server().await;
    let client = reqwest::Client::new();
    let world_id = bind(&client, &url).await;
    let prop = propose_ok(&client, &url, &world_id, "rev-1").await;
    let exec: Value = client
        .post(format!("{url}{DES_API}/execute"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "proposal_id": prop["proposal_id"],
            "approval_ref": "ops-sync",
            "idempotency_key": "idem-sync",
            "plan_revision": "rev-1"
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
        .post(format!("{url}{DES_API}/test/fail-answer-sync"))
        .headers(experimenter())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let pending: Value = client
        .post(format!("{url}{DES_API}/ground_truth"))
        .headers(auditor())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pending["ground_truth"]["sync_status"], "pending");
    assert_eq!(pending["ground_truth"]["intervention_id"], execution_id);

    let retried: Value = client
        .post(format!("{url}{DES_API}/test/retry-answer-sync"))
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
    assert_eq!(retried["sync_status"], "synced");
    assert_eq!(retried["intervention_id"], execution_id);

    let truth: Value = client
        .post(format!("{url}{DES_API}/ground_truth"))
        .headers(auditor())
        .json(&json!({ "world_id": world_id }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let gt = &truth["ground_truth"];
    assert_eq!(gt["sync_status"], "synced");
    assert_eq!(gt["intervention_id"], execution_id);
    assert_eq!(gt["execution_id"], execution_id);
    assert_eq!(gt["answer"], "maintenance-applied");
}
