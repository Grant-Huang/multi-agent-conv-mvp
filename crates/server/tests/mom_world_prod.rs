//! World Adapter M3-2 生产只读合同：RT-WAPROD-01～02（见 docs/testing/CASES.md）。
//! 服务层 `/api/world/prod` 内存 stub；不改 engine / store / 凭据 egress。

use an_server::mom_world_prod::{router, WorldProdState};
use serde_json::{json, Value};

const PROD_API: &str = "/api/world/prod";

async fn prod_server() -> (String, tokio::task::JoinHandle<()>) {
    let app = router(WorldProdState::new());
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

/// RT-WAPROD-01：已登记权威源 bind 成功；未配置权威源明确错误。
#[tokio::test]
async fn rt_waprod_01_bind_requires_registered_authority() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();

    let missing = client
        .post(format!("{url}{PROD_API}/bind"))
        .headers(operator())
        .json(&json!({
            "site_id": "plant-unknown",
            "authority": "mes-stub",
            "request_id": "bind-missing"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        missing.status().as_u16() != 200,
        "未配置权威源须明确错误（非 200），got {}",
        missing.status()
    );
    let missing_body: Value = missing.json().await.unwrap();
    assert!(
        missing_body["code"]
            .as_str()
            .unwrap_or("")
            .contains("AUTHORITY")
            || missing_body["code"].as_str() == Some("NOT_CONFIGURED")
            || missing_body["message"]
                .as_str()
                .unwrap_or("")
                .contains("权威"),
        "错误须可辨：got {missing_body}"
    );

    let no_auth = client
        .post(format!("{url}{PROD_API}/bind"))
        .headers(operator())
        .json(&json!({
            "site_id": "plant-east",
            "request_id": "bind-no-authority"
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(no_auth.status().as_u16(), 200, "缺 authority 字段须拒绝");

    let ok = client
        .post(format!("{url}{PROD_API}/bind"))
        .headers(operator())
        .json(&json!({
            "site_id": "plant-east",
            "authority": "mes-stub",
            "request_id": "bind-ok"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let body: Value = ok.json().await.unwrap();
    assert_eq!(body["protocol_version"], 1);
    assert_eq!(body["provenance"], "production");
    assert!(
        !body["world_id"].as_str().unwrap_or("").is_empty(),
        "绑定须返回 world_id"
    );
}

/// RT-WAPROD-02：Observe / Subscribe 对齐正式水位；含 source / as_of / protocol_version。
#[tokio::test]
async fn rt_waprod_02_observe_subscribe_watermark() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();

    let bound: Value = client
        .post(format!("{url}{PROD_API}/bind"))
        .headers(operator())
        .json(&json!({
            "site_id": "plant-east",
            "authority": "mes-stub",
            "request_id": "obs-bind"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let world_id = bound["world_id"].as_str().unwrap().to_string();

    let obs = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator())
        .json(&json!({ "world_id": world_id, "request_id": "obs-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(obs.status(), 200);
    let snap: Value = obs.json().await.unwrap();
    assert_eq!(snap["protocol_version"], 1);
    assert_eq!(snap["provenance"], "production");
    assert!(!snap["as_of"].as_str().unwrap_or("").is_empty());
    assert!(!snap["cursor"].as_str().unwrap_or("").is_empty());
    let publish_seq = snap["publish_seq"]
        .as_u64()
        .expect("observe 须含 publish_seq");
    let objects = snap["objects"].as_array().expect("objects");
    assert!(!objects.is_empty());
    assert_eq!(objects[0]["source"].as_str(), Some("mes"));
    let events = snap["events"].as_array().cloned().unwrap_or_default();
    for e in &events {
        assert_eq!(e["source"].as_str(), Some("mes"));
    }

    let sub = client
        .post(format!("{url}{PROD_API}/subscribe"))
        .headers(operator())
        .json(&json!({
            "world_id": world_id,
            "cursor": snap["cursor"],
            "request_id": "sub-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(sub.status(), 200);
    let page: Value = sub.json().await.unwrap();
    assert_eq!(page["protocol_version"], 1);
    assert_eq!(page["provenance"], "production");
    assert!(!page["as_of"].as_str().unwrap_or("").is_empty());
    assert_eq!(
        page["publish_seq"].as_u64(),
        Some(publish_seq),
        "Subscribe 水位须与 Observe 对齐"
    );
    assert_eq!(page["as_of"], snap["as_of"]);
    // 空续读页仍返回下一游标；事件不应重复抬水位
    assert!(!page["cursor"].as_str().unwrap_or("").is_empty());
}
