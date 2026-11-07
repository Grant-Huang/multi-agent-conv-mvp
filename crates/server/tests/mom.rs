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
