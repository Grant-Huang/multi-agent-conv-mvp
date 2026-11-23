//! World Adapter M3 生产只读合同：RT-WAPROD-01～06（见 docs/testing/CASES.md）。
//! 服务层 `/api/world/prod` 内存 stub；不改 engine / store / 凭据 egress。

use an_server::mom_world_mock::{router as mock_router, WorldMockState};
use an_server::mom_world_prod::{router, WorldProdState};
use serde_json::{json, Value};

const PROD_API: &str = "/api/world/prod";

async fn prod_server() -> (String, tokio::task::JoinHandle<()>) {
    spawn_router(router(WorldProdState::with_test_controls())).await
}

async fn spawn_router(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
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

fn operator_site(site: &str) -> reqwest::header::HeaderMap {
    let mut h = operator();
    h.insert("x-wa-site", site.parse().unwrap());
    h
}

async fn bind_east(client: &reqwest::Client, url: &str, request_id: &str) -> String {
    let bound: Value = client
        .post(format!("{url}{PROD_API}/bind"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "site_id": "plant-east",
            "authority": "mes-stub",
            "request_id": request_id,
            "sources": ["mes", "erp", "gateway"]
        }))
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

/// RT-WAPROD-03：跨 scope / 无授权 → ACCESS_DENIED；无跨站泄露。
#[tokio::test]
async fn rt_waprod_03_access_denied_cross_scope() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();
    let world_id = bind_east(&client, &url, "iso-bind").await;

    let cross = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator_site("plant-west"))
        .json(&json!({ "world_id": world_id, "request_id": "iso-cross" }))
        .send()
        .await
        .unwrap();
    assert!(
        matches!(cross.status().as_u16(), 401 | 403),
        "跨站点读须拒绝，got {}",
        cross.status()
    );
    let cross_body: Value = cross.json().await.unwrap();
    assert_eq!(cross_body["code"], "ACCESS_DENIED");
    assert!(
        cross_body.get("objects").is_none() && cross_body.get("events").is_none(),
        "拒绝响应不得泄露对象/事件：{cross_body}"
    );

    let mut no_role = reqwest::header::HeaderMap::new();
    no_role.insert("x-wa-role", "unauthorized".parse().unwrap());
    no_role.insert("x-wa-site", "plant-east".parse().unwrap());
    let denied = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(no_role)
        .json(&json!({ "world_id": world_id, "request_id": "iso-role" }))
        .send()
        .await
        .unwrap();
    assert!(
        matches!(denied.status().as_u16(), 401 | 403),
        "无授权身份须拒绝，got {}",
        denied.status()
    );
    let denied_body: Value = denied.json().await.unwrap();
    assert_eq!(denied_body["code"], "ACCESS_DENIED");
}

/// RT-WAPROD-04：过期游标 → CURSOR_EXPIRED；恢复后续读无漏无重复。
#[tokio::test]
async fn rt_waprod_04_cursor_expired_then_recover() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();
    let world_id = bind_east(&client, &url, "cur-bind").await;

    let obs: Value = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator_site("plant-east"))
        .json(&json!({ "world_id": world_id, "request_id": "cur-obs" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let cursor = obs["cursor"].as_str().unwrap().to_string();
    let before_ids: Vec<String> = obs["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event_id"].as_str().map(str::to_string))
        .collect();
    assert!(!before_ids.is_empty());

    let expire = client
        .post(format!("{url}{PROD_API}/test/expire-cursor"))
        .headers(operator_site("plant-east"))
        .json(&json!({ "world_id": world_id, "request_id": "cur-exp" }))
        .send()
        .await
        .unwrap();
    assert_eq!(expire.status(), 200);

    let stale = client
        .post(format!("{url}{PROD_API}/subscribe"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id": world_id,
            "cursor": cursor,
            "request_id": "cur-stale"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);
    let stale_body: Value = stale.json().await.unwrap();
    assert_eq!(stale_body["code"], "CURSOR_EXPIRED");

    let again: Value = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator_site("plant-east"))
        .json(&json!({ "world_id": world_id, "request_id": "cur-reobs" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let after_ids: Vec<String> = again["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event_id"].as_str().map(str::to_string))
        .collect();
    let mut sorted_before = before_ids.clone();
    sorted_before.sort();
    let mut sorted_after = after_ids.clone();
    sorted_after.sort();
    assert_eq!(sorted_before, sorted_after, "恢复后事件集合须一致（无漏）");
    assert_eq!(
        after_ids.len(),
        after_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "恢复后不得重复 event_id"
    );

    let sub = client
        .post(format!("{url}{PROD_API}/subscribe"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id": world_id,
            "cursor": again["cursor"],
            "request_id": "cur-ok"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(sub.status(), 200);
}

/// RT-WAPROD-05：注入延迟后水位与载荷截至时间一致。
#[tokio::test]
async fn rt_waprod_05_inject_delay_aligns_as_of() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();
    let world_id = bind_east(&client, &url, "delay-bind").await;

    let delay = client
        .post(format!("{url}{PROD_API}/test/inject-delay"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id": world_id,
            "source": "erp",
            "delay_ms": 5000,
            "request_id": "delay-inj"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(delay.status(), 200);

    let obs: Value = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator_site("plant-east"))
        .json(&json!({ "world_id": world_id, "request_id": "delay-obs" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(obs["consistency"], "per_source");
    let objects = obs["objects"].as_array().expect("objects");
    let erp = objects
        .iter()
        .find(|o| o["source"].as_str() == Some("erp"))
        .expect("须含 erp 来源对象");
    assert_eq!(erp["delayed"], true);
    assert_eq!(
        erp["observed_at"], obs["as_of"],
        "延迟源 observed_at 须与水位 as_of 一致"
    );
    assert!(
        obs["as_of"].as_str().unwrap_or("").contains("delay")
            || erp["state"]["delay_ms"].as_u64() == Some(5000),
        "延迟须可见：as_of={} state={}",
        obs["as_of"],
        erp["state"]
    );
}

/// RT-WAPROD-06：生产 Propose/Execute 拒绝；Mock 写路径仍可用。
#[tokio::test]
async fn rt_waprod_06_write_closed_on_prod_mock_still_works() {
    let (url, _task) = prod_server().await;
    let client = reqwest::Client::new();
    let world_id = bind_east(&client, &url, "write-bind").await;

    let propose = client
        .post(format!("{url}{PROD_API}/propose"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id": world_id,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": { "priority": "high" },
            "request_id": "write-propose"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        matches!(propose.status().as_u16(), 400 | 403 | 405 | 501),
        "生产 Propose 须拒绝，got {}",
        propose.status()
    );
    let propose_body: Value = propose.json().await.unwrap();
    assert!(
        propose_body["code"]
            .as_str()
            .unwrap_or("")
            .contains("WRITE")
            || propose_body["code"].as_str() == Some("CAPABILITY_DISABLED"),
        "须明确写关闭码：{propose_body}"
    );

    let execute = client
        .post(format!("{url}{PROD_API}/execute"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id": world_id,
            "proposal_id": "none",
            "approval_ref": "ops-1",
            "idempotency_key": "k1",
            "request_id": "write-exec"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        matches!(execute.status().as_u16(), 400 | 403 | 405 | 501),
        "生产 Execute 须拒绝，got {}",
        execute.status()
    );

    // Mock 写路径仍可用（隔离世界）
    let mock_app = mock_router(WorldMockState::new(true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_url = format!("http://{}", listener.local_addr().unwrap());
    let _mock_task = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let mut experimenter = reqwest::header::HeaderMap::new();
    experimenter.insert("x-wa-role", "experimenter".parse().unwrap());
    let cfg: Value = client
        .post(format!("{mock_url}/api/world/mock/experiment/configs"))
        .headers(experimenter.clone())
        .json(&json!({
            "factory": "plant-a",
            "scenario": "s1",
            "seed": 1,
            "duration_minutes": 480,
            "request_id": "mock-cfg"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let config_version = cfg["config_version"].as_str().expect("config_version");
    let official: Value = client
        .post(format!("{mock_url}/api/world/mock/experiment/official"))
        .headers(experimenter)
        .json(&json!({
            "config_version": config_version,
            "target_channel": "ops-channel",
            "start_key": "m3-3-start",
            "request_id": "mock-off"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mock_world = official["world_id"].as_str().expect("mock world_id");
    let mock_prop = client
        .post(format!("{mock_url}/api/world/mock/propose"))
        .header("x-wa-role", "operator")
        .json(&json!({
            "world_id": mock_world,
            "action_type": "eam.maintenance",
            "target": { "type": "equipment", "id": "CNC-1" },
            "parameters": {},
            "request_id": "mock-still"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        mock_prop.status(),
        200,
        "Mock Propose 仍须可用，got {}",
        mock_prop.status()
    );
}

#[tokio::test]
async fn rt_waprod_07_selected_work_order_needs_explicit_fixture_injection() {
    let (url, task) = prod_server().await;
    let client = reqwest::Client::new();
    let world = bind_east(&client, &url, "fixture-disabled").await;
    let response = client
        .post(format!("{url}{PROD_API}/observe"))
        .headers(operator_site("plant-east"))
        .json(&json!({
            "world_id":world, "request_id":"fixture-disabled",
            "selection":{"type":"work_order","id":"opaque"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert_eq!(
        response.json::<Value>().await.unwrap()["code"],
        "TEST_CONTROLS_DISABLED"
    );
    task.abort();
}

#[tokio::test]
async fn rt_waprod_08_selected_work_order_reads_only_its_bound_loopback_adapter() {
    use an_server::mom_world_prod::{WorkOrderFixture, WorkOrderFixtureError};
    use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use mom_external_adapter::{
        external_http::{HttpSource, HttpSourceKind},
        external_observe::{
            AuthorizedWorkOrder, ExternalObserve, ObserveError, SourceScope, WorkOrderSelection,
        },
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    type Calls = Arc<Mutex<Vec<&'static str>>>;

    async fn odoo(State(calls): State<Calls>, Json(body): Json<Value>) -> Json<Value> {
        calls.lock().unwrap().push("odoo");
        Json(if body["domain"][0][2] == 42 {
            json!([{"id":42,"name":"WO-42","state":"confirmed"}])
        } else {
            json!([])
        })
    }
    async fn openmes(State(calls): State<Calls>, Path(id): Path<u64>) -> impl IntoResponse {
        calls.lock().unwrap().push("openmes");
        match id {
            42 => Json(json!({"data":{"id":42,"order_no":"WO-42","status":"PENDING"}}))
                .into_response(),
            43 => StatusCode::NOT_FOUND.into_response(),
            _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        }
    }
    async fn carbon(
        State(calls): State<Calls>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        calls.lock().unwrap().push("carbon");
        assert_eq!(query["companyId"], "eq.company-a");
        Json(if query["id"] == "eq.job-42" {
            json!([{"id":"job-42","jobId":"WO-42","status":"Ready","companyId":"company-a"}])
        } else {
            json!([])
        })
    }

    let calls: Calls = Arc::default();
    let (source_url, source_task) = spawn_router(
        Router::new()
            .route("/json/2/mrp.production/search_read", post(odoo))
            .route("/api/v1/work-orders/{id}", get(openmes))
            .route("/rest/v1/jobs", get(carbon))
            .with_state(calls.clone()),
    )
    .await;
    let bindings: Arc<Mutex<HashMap<String, &'static str>>> = Arc::default();
    let state = WorldProdState::with_work_order_fixture({
        let bindings = bindings.clone();
        move |world, site, reference| {
            let instance = bindings.lock().unwrap().get(&world).copied();
            let origin = source_url.clone();
            Box::pin(async move {
                let instance = instance.ok_or(WorkOrderFixtureError::AccessDenied)?;
                let kind = match instance {
                    "odoo" => HttpSourceKind::Odoo {
                        database: "fixture",
                    },
                    "openmes" => HttpSourceKind::OpenMes,
                    "carbon" => HttpSourceKind::CarbonRest {
                        company: "company-a",
                        project_key: "fixture",
                    },
                    _ => unreachable!("fixture sources are fixed"),
                };
                let source_id = match (reference.as_str(), instance) {
                    ("opaque-visible", "carbon") => "job-42",
                    ("opaque-hidden", "carbon") => "job-43",
                    ("opaque-visible", _) => "42",
                    ("opaque-hidden", _) => "43",
                    ("opaque-failure", "openmes") => "44",
                    _ => return Err(WorkOrderFixtureError::AccessDenied),
                };
                let scope = SourceScope {
                    tenant: "fixture-tenant",
                    site: &site,
                    instance,
                };
                let adapter = ExternalObserve::new([HttpSource {
                    kind,
                    base_url: &origin,
                    token: "fixture-token",
                    tenant: scope.tenant,
                    site: scope.site,
                    instance: scope.instance,
                }])
                .map_err(|_| WorkOrderFixtureError::SourceUnavailable)?;
                let order = adapter
                    .observe(
                        WorkOrderSelection {
                            world_id: &world,
                            object_ref: &reference,
                        },
                        |_| async { Some(AuthorizedWorkOrder { scope, source_id }) },
                    )
                    .await
                    .map_err(|error| match error {
                        ObserveError::AccessDenied => WorkOrderFixtureError::AccessDenied,
                        ObserveError::NotVisible => WorkOrderFixtureError::NotVisible,
                        ObserveError::SourceUnavailable => WorkOrderFixtureError::SourceUnavailable,
                        ObserveError::InvalidSourceResponse => {
                            WorkOrderFixtureError::InvalidSourceResponse
                        }
                    })?;
                Ok(WorkOrderFixture {
                    source: order.source_instance.to_owned(),
                    observed_at: order.observed_at,
                    state: json!({
                        "display_number": order.display_number, "source_status": order.source_status,
                        "normalized_status": order.normalized_status, "mapping_version": order.mapping_version,
                    }),
                })
            })
        }
    });
    let (origin, task) = spawn_router(router(state)).await;
    let client = reqwest::Client::new();
    let endpoint = format!("{origin}{PROD_API}/observe");
    let mut expected_calls = Vec::new();
    for (vendor, source_status) in [
        ("odoo", "confirmed"),
        ("openmes", "PENDING"),
        ("carbon", "Ready"),
    ] {
        let world = bind_east(&client, &origin, vendor).await;
        bindings.lock().unwrap().insert(world.clone(), vendor);
        let read = |headers, id| {
            client.post(&endpoint).headers(headers).json(&json!({
            "world_id": world, "request_id": "fixture-read", "selection":{"type":"work_order","id":id},
        })).send()
        };

        for headers in [Default::default(), operator(), operator_site("plant-west")] {
            let result = read(headers, "opaque-visible").await.unwrap();
            assert_eq!(result.status(), 403);
            assert_eq!(
                result.json::<Value>().await.unwrap()["code"],
                "ACCESS_DENIED"
            );
        }
        let invalid = client
            .post(&endpoint)
            .headers(operator_site("plant-east"))
            .json(&json!({
                "world_id": world,
                "selection": {"type":"work_order","id":"opaque-visible","source_url":"fixture-override"}
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid.status(), 422);
        let unknown = read(operator_site("plant-east"), "unbound").await.unwrap();
        assert_eq!(unknown.status(), 403);
        assert_eq!(*calls.lock().unwrap(), expected_calls);

        let visible = read(operator_site("plant-east"), "opaque-visible")
            .await
            .unwrap();
        assert_eq!(visible.status(), 200);
        let body = visible.json::<Value>().await.unwrap();
        assert_eq!(body["provenance"], "fixture");
        assert_eq!(body["request_id"], "fixture-read");
        assert!(body["cursor"].is_null());
        assert!(body["publish_seq"].is_null());
        assert_eq!(body["events"], json!([]));
        assert_eq!(body["objects"][0]["source"], vendor);
        assert_eq!(
            body["objects"][0]["ref"],
            json!({"type":"work_order","id":"opaque-visible"})
        );
        assert_eq!(body["objects"][0]["state"]["source_status"], source_status);
        assert_eq!(body["objects"][0]["observed_at"], body["as_of"]);
        let serialized = body.to_string();
        for private in ["fixture-tenant", "fixture-token", "company-a", "job-42"] {
            assert!(!serialized.contains(private));
        }

        let hidden = read(operator_site("plant-east"), "opaque-hidden")
            .await
            .unwrap();
        assert_eq!(hidden.status(), 404);
        assert_eq!(
            hidden.json::<Value>().await.unwrap()["code"],
            "ORDER_NOT_VISIBLE"
        );
        expected_calls.extend([vendor, vendor]);

        if vendor == "openmes" {
            let failed = read(operator_site("plant-east"), "opaque-failure")
                .await
                .unwrap();
            assert_eq!(failed.status(), 503);
            assert_eq!(
                failed.json::<Value>().await.unwrap()["code"],
                "SOURCE_UNAVAILABLE"
            );
            expected_calls.push(vendor);
        }
        assert_eq!(*calls.lock().unwrap(), expected_calls);
    }
    task.abort();
    source_task.abort();
}
