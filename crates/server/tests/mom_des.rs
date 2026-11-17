//! MOM 车间 DES 阶段② HTTP 用例：HTTP-DES-01、HTTP-DES-02。
//! 用例定义见 `docs/testing/CASES.md`「MOM 车间 DES 阶段②」。
use an_runtime::mom::Engine;
use an_server::mom;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let state = mom::AppState::scripted(Arc::new(Mutex::new(Engine::memory().unwrap())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    (url, task)
}

/// 标准答案字段与隐状态名不得出现在接口返回中。
fn assert_no_truth(label: &str, v: &Value) {
    let text = v.to_string();
    for m in [
        "root_causes",
        "mechanisms",
        "ground_truth",
        "spindle_wear",
        "wear_to_dimension",
    ] {
        assert!(!text.contains(m), "{label} leaks {m}");
    }
}

/// 建一个 S1 案例（用于推进共享时钟），再创建 P0 固定剧本 DES 仿真并推进 30 分钟。
async fn setup(client: &reqwest::Client, url: &str) -> String {
    let case: Value = client
        .post(format!("{url}/api/mom/scenarios/s1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let case_id = case["case_id"].as_str().unwrap().to_string();
    let r = client
        .post(format!("{url}/api/mom/des/simulations"))
        .json(&json!({"name":"p0","factory":null,"scenario":{"seed":1,"pattern":{"kind":"script"},"start_minute":497,"horizon_minutes":240,"mechanisms":null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "DES simulation must be created");
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["provenance"], "simulation");
    let sim = v["simulation_id"]
        .as_str()
        .expect("simulation id")
        .to_string();
    let r = client
        .post(format!("{url}/api/mom/cases/{case_id}/commands"))
        .json(&json!({"command":"advance","minutes":30}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "clock must advance");
    sim
}

#[tokio::test]
async fn http_des_01_factory_forecast_and_whatif_are_read_only_simulation_views() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    let sim = setup(&client, &url).await;

    let r = client
        .get(format!("{url}/api/mom/factory?simulation={sim}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "factory view must be available");
    let f: Value = r.json().await.unwrap();
    assert_eq!(f["provenance"], "simulation");
    assert!(
        f["observed_at"].as_u64().is_some(),
        "factory view needs observed_at"
    );
    let xx = f["machines"]
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["id"] == "CNC-XX")
        .expect("CNC-XX listed")
        .clone();
    assert!(
        xx["vibration_mm_s"].as_f64().unwrap() >= 2.8,
        "CNC-XX vibration in zone C"
    );
    assert!(xx["cpk"].as_f64().is_some(), "CNC-XX Cpk reported");
    assert!(f["resources"].is_object(), "resources reported");
    assert!(!f.to_string().contains("wear\""), "no hidden wear value");
    assert_no_truth("factory", &f);

    let r = client
        .get(format!("{url}/api/mom/factory/forecast?simulation={sim}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "forecast must be available");
    let fc: Value = r.json().await.unwrap();
    assert_eq!(fc["provenance"], "simulation");
    assert_eq!(fc["method"], "linear_trend");
    assert!(
        fc["machines"].as_array().is_some_and(|m| m.len() == 3),
        "one forecast per machine"
    );
    assert_no_truth("forecast", &fc);

    let r = client
        .post(format!("{url}/api/mom/factory/whatif"))
        .json(&json!({"simulation_id":sim,"asset":"CNC-XX","quantity":500,"due_minute":960}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "what-if must be available");
    let w: Value = r.json().await.unwrap();
    assert_eq!(w["plans"].as_array().map(|p| p.len()), Some(3));
    assert_eq!(w["provenance"], "simulation");
    assert_no_truth("what-if", &w);
    task.abort();
}

#[tokio::test]
async fn http_des_02_invalid_requests_fail_fast_without_breaking_service() {
    let (url, task) = server().await;
    let client = reqwest::Client::new();
    let sim = setup(&client, &url).await;
    let bad = [
        json!({"simulation_id":sim,"asset":"CNC-XX","quantity":0,"due_minute":960}),
        json!({"simulation_id":sim,"asset":"CNC-XX","quantity":500,"due_minute":99_999}),
        json!({"simulation_id":"DES-404","asset":"CNC-XX","quantity":500,"due_minute":960}),
        json!({"simulation_id":sim,"asset":"CNC-NOPE","quantity":500,"due_minute":960}),
    ];
    for body in bad {
        let started = Instant::now();
        let r = client
            .post(format!("{url}/api/mom/factory/whatif"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "invalid what-if must fail fast"
        );
        assert_eq!(r.status(), 400, "invalid what-if {body} must be rejected");
        let v: Value = r.json().await.unwrap();
        assert!(
            v["error"].as_str().is_some_and(|e| !e.is_empty()),
            "error explains why"
        );
    }
    let machines: Vec<Value> = (0..33)
        .map(|i| json!({"id":format!("CNC-{i:02}"),"line":"Line-B","cycle_seconds":90}))
        .collect();
    let r = client
        .post(format!("{url}/api/mom/des/simulations"))
        .json(&json!({"name":"big","factory":{"machines":machines},"scenario":{"seed":1,"pattern":{"kind":"unconstrained"},"start_minute":497,"horizon_minutes":60,"mechanisms":null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "33 machines must be rejected");
    let r = client
        .get(format!("{url}/api/mom/factory?simulation={sim}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "service must stay responsive");
    task.abort();
}
