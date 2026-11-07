use an_runtime::mom::Engine;
use an_server::mom;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let state = mom::AppState::scripted(Arc::new(Mutex::new(Engine::memory().unwrap())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    (url, task)
}
#[tokio::test]
async fn human_and_agent_routes_share_facts_and_gate_commands() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    let v: Value = client
        .post(format!("{url}/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = v["case_id"].as_str().unwrap();
    let human: Value = client
        .get(format!("{url}/api/mom/cases/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let agent: Value = client
        .get(format!("{url}/api/mom/cases/{id}/agent?limit=2"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(human["projection_seq"], agent["projection_seq"]);
    assert_eq!(human["provenance"], "mock");
    let forbidden = client
        .post(format!("{url}/api/mom/cases/{id}/commands"))
        .json(&json!({"command":"approve","scope":"production","plan_revision":1,"actor":"master"}))
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), 400);
    let html = client.get(&url).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("MOM"));
    assert!(html.contains("Agent"));
    task.abort();
}
#[tokio::test]
async fn s1_http_approval_execution_and_verification() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    let v: Value = client
        .post(format!("{url}/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = v["case_id"].as_str().unwrap();
    for body in [
        json!({"command":"approve","scope":"production","actor":"supervisor","plan_revision":1}),
        json!({"command":"execute","scope":"production"}),
        json!({"command":"advance","minutes":3}),
        json!({"command":"close"}),
    ] {
        let response = client
            .post(format!("{url}/api/mom/cases/{id}/commands"))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "{text}");
    }
    let v: Value = client
        .get(format!("{url}/api/mom/cases/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["stage"], "CLOSED");
    task.abort();
}
#[tokio::test]
async fn malformed_commands_and_missing_cases_are_errors() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(format!("{url}/api/mom/scenarios/unknown"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .get(format!("{url}/api/mom/cases/missing"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .post(format!("{url}/api/mom/events"))
            .json(&json!({"command":"shell"}))
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
    task.abort();
}

#[tokio::test]
async fn configurable_simulation_queues_events_and_exposes_gap_register() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    let r=client.post(format!("{url}/api/mom/simulations")).json(&json!({"name":"custom","scenario":"s1","seed":42,"world":{"quantity":900},"event_count":2,"event_interval_minutes":5,"disturbances":[]})).send().await.unwrap();
    assert_eq!(
        r.status(),
        200,
        "configurable simulation route must accept valid scenarios"
    );
    let v: Value = r.json().await.unwrap();
    let id = v["case_id"].as_str().unwrap();
    let sims: Value = client
        .get(format!("{url}/api/mom/simulations"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(sims["simulations"][0]["pending"].as_u64().unwrap() >= 4);
    let v: Value = client
        .post(format!("{url}/api/mom/cases/{id}/commands"))
        .json(&json!({"command":"advance","minutes":17}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["proposal"]["quantity"], 900);
    assert_eq!(v["proposal"]["feasible"], false);
    let gaps: Value = client
        .get(format!("{url}/api/mom/gaps"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(gaps["items"].as_array().unwrap().len(), 24);
    task.abort();
}

#[tokio::test]
async fn safety_trip_rejects_http_patch_and_dispatch_without_side_effects() {
    let mut engine = Engine::memory().unwrap();
    let id = engine
        .ingest(an_runtime::mom::EventInput::s1("order", 497))
        .unwrap();
    engine
        .ingest(an_runtime::mom::EventInput::s1("vibration", 503))
        .unwrap();
    engine
        .ingest(an_runtime::mom::EventInput::s1("quality", 511))
        .unwrap();
    engine.advance(514).unwrap();
    let mut trip = an_runtime::mom::EventInput::s1("vibration", 514);
    trip.id = "http-safety-trip".into();
    trip.payload = json!({"safety_trip": true});
    engine.ingest(trip).unwrap();
    let revision = engine.case(&id).unwrap().plan_revision;
    let before = serde_json::to_value(engine.case(&id).unwrap()).unwrap();
    let engine = Arc::new(Mutex::new(engine));
    let state = mom::AppState::scripted(engine.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/mom/cases/{id}/commands",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    let client = reqwest::Client::new();
    for command in [
        json!({"command":"patch", "node":"quality", "dependency":"maintenance"}),
        json!({"command":"refresh_plan"}),
        json!({"command":"approve", "scope":"production", "actor":"supervisor", "plan_revision":revision}),
        json!({"command":"execute", "scope":"production"}),
        json!({"command":"delegate", "kind":"equipment_investigation"}),
    ] {
        let response = client.post(&url).json(&command).send().await.unwrap();
        assert_eq!(response.status(), 400, "command {command}");
        assert_eq!(
            serde_json::to_value(engine.lock().unwrap().case(&id).unwrap()).unwrap(),
            before
        );
    }
    task.abort();
}

// ─── E1-C：an-mom 通过 HTTP 调用 RelOS 上下文编译接口（HTTP-MOM-RELOS-01/02，实现前定义） ───

/// 桩 RelOS：记录收到的请求体，返回固定的 /v1/context/compile 响应
async fn stub_relos() -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    use axum::{routing::post, Json, Router};
    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let app = Router::new().route(
        "/v1/context/compile",
        post(move |Json(body): Json<Value>| {
            let log = log.clone();
            async move {
                log.lock().unwrap().push(body);
                Json(json!({
                    "content": "## 工厂关系上下文（RelOS）",
                    "relation_count": 3, "estimated_tokens": 300, "pruned_count": 5,
                    "candidate_count": 8, "query_strategy": "causal_path",
                    "selected_ids": ["r-eq", "r-fm-def", "r-other"],
                    "selected_relations": [
                        {"id": "r-eq", "relation_type": "Equipment__HAS_FAILURE_MODE__FailureMode",
                         "source_node_id": "CNC-XX", "target_node_id": "FM-SPINDLE-BEARING",
                         "confidence": 0.81},
                        {"id": "r-fm-def", "relation_type": "FailureMode__LEADS_TO__Defect",
                         "source_node_id": "FM-SPINDLE-BEARING", "target_node_id": "Line-B",
                         "confidence": 0.72},
                        {"id": "r-other", "relation_type": "Equipment__TRIGGERS__Alarm",
                         "source_node_id": "CNC-XX", "target_node_id": "ALM-1",
                         "confidence": 0.9}
                    ]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, seen, task)
}

async fn server_with_relos(relos_url: &str) -> (String, tokio::task::JoinHandle<()>) {
    let state = mom::AppState::scripted(Arc::new(Mutex::new(Engine::memory().unwrap())))
        .with_relos(relos_url)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    (url, task)
}

async fn create_s1_agent_view(url: &str) -> Value {
    let client = reqwest::Client::new();
    let v: Value = client
        .post(format!("{url}/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = v["case_id"].as_str().unwrap().to_string();
    client
        .get(format!("{url}/api/mom/cases/{id}/agent"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn http_mom_relos_01_scenario_pulls_context_from_relos() {
    let (relos_url, seen, relos_task) = stub_relos().await;
    let (url, task) = server_with_relos(&relos_url).await;
    let agent = create_s1_agent_view(&url).await;
    let m = &agent["nodes"]["maintenance"];
    assert_eq!(m["status"], "SUCCEEDED");
    assert_eq!(
        m["output"]["candidate_causes"][0]["cause"],
        "FM-SPINDLE-BEARING"
    );
    assert_eq!(m["output"]["candidate_causes"][0]["relation_id"], "r-eq");
    let q = &agent["nodes"]["quality"];
    assert_eq!(
        q["output"]["candidate_causes"][0]["relation_id"],
        "r-fm-def"
    );
    // 请求以设备、批次、产线为锚点，并带有查询描述与预算
    let req = seen.lock().unwrap()[0].clone();
    assert_eq!(req["center_node_id"], "CNC-XX");
    assert!(req["anchor_node_ids"]
        .as_array()
        .unwrap()
        .contains(&json!("Line-B")));
    assert!(req["token_budget"].as_u64().unwrap() > 0);
    task.abort();
    relos_task.abort();
}

#[tokio::test]
async fn http_mom_relos_02_unreachable_relos_degrades_safely() {
    // 绑定后立即释放端口，得到一个确定无人监听的本地地址
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_url = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let (url, task) = server_with_relos(&dead_url).await;
    let started = std::time::Instant::now();
    let agent = create_s1_agent_view(&url).await;
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let m = &agent["nodes"]["maintenance"];
    assert_eq!(m["status"], "UNKNOWN");
    assert_eq!(m["output"]["relos_status"], "unavailable");
    assert_eq!(agent["stage"], "WAITING_HUMAN");
    task.abort();
}
