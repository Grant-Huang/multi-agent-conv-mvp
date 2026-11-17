//! R-20261117-L1：受限 DES 创建不能长期阻塞并发读取。
use an_runtime::mom::Engine;
use an_server::mom;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_des_24_impossible_large_scenario_and_concurrent_read_finish_within_budget() {
    let state = mom::AppState::scripted(Arc::new(Mutex::new(Engine::memory().unwrap())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    let client = reqwest::Client::new();
    let machines: Vec<Value> = (0..32)
        .map(|i| json!({"id":format!("CNC-{i:02}"),"line":"Line-B","cycle_seconds":90}))
        .collect();
    let request = json!({"name":"impossible","factory":{"machines":machines},"scenario":{
        "seed":1,"pattern":{"kind":"concurrent_cluster","window_minutes":1,"min_disruptions":1000},
        "start_minute":497,"horizon_minutes":2880,"mechanisms":null}});
    let started = Instant::now();
    let post = client
        .post(format!("{url}/api/mom/des/simulations"))
        .json(&request);
    let get = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let at = Instant::now();
        let response = client
            .get(format!("{url}/api/mom/cases"))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        (status, body, at.elapsed())
    };
    let (created, (read_status, read_body, read_elapsed)) = tokio::join!(post.send(), get);
    let created = created.unwrap();
    let status = created.status();
    let error: Value = created.json().await.unwrap();
    let elapsed = started.elapsed();
    server.abort();
    let _ = server.await;
    assert_eq!(status, 400, "impossible constraint must be rejected");
    assert!(
        error["error"].as_str().unwrap().contains("constraint"),
        "{error}"
    );
    assert_eq!(read_status, 200);
    assert!(
        !read_body.to_string().contains("impossible"),
        "rejected creation must not persist"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "bounded creation took {elapsed:?}"
    );
    assert!(
        read_elapsed < Duration::from_secs(2),
        "concurrent read took {read_elapsed:?}"
    );
}
