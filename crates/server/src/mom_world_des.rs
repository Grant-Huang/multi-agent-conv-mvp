//! World Adapter M1 远程 DES 读合同（`/api/world/des`）。
//!
//! 服务层独立实现，对齐 `mom_world_mock` 字段形状，`provenance=des`。
//! 正式投递水位在本适配器内维护：step/tick 之后才向 Observe 暴露新事件。
//! 不改 `engine.rs` / `DesConnector` 内部；写路径仍走 `/api/world/mock`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

const PROTOCOL_VERSION: u32 = 1;

/// `connector_available=false` 时绑定失败（模拟未挂 DesConnector）。
#[derive(Clone)]
pub struct WorldDesState {
    inner: Arc<Mutex<Store>>,
    connector_available: bool,
}

impl WorldDesState {
    pub fn new(connector_available: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            connector_available,
        }
    }
}

#[derive(Default)]
struct Store {
    next: u64,
    configs: HashMap<String, ()>,
    worlds: HashMap<String, WorldRec>,
    runs: HashMap<String, RunRec>,
    /// 已正式投递、对 Observe/Subscribe 可见的事件
    published: HashMap<String, Vec<EventRec>>,
    /// tick 产生、尚未越过正式投递边界的缓冲
    pending: HashMap<String, Vec<EventRec>>,
    cursors: HashMap<String, CursorRec>,
    cursor_generation: HashMap<String, u64>,
}

#[derive(Clone)]
#[allow(dead_code)] // 合同字段；后续 Observe 对象投影会读
struct WorldRec {
    world_id: String,
    run_id: Option<String>,
}

#[derive(Clone)]
#[allow(dead_code)] // delivery/mode/case 供运行态扩展；现只推 simulation_minute
struct RunRec {
    run_id: String,
    world_id: String,
    mode: &'static str,
    config_version: String,
    delivery_count: u32,
    case_id: Option<String>,
    simulation_minute: i64,
}

#[derive(Clone)]
struct EventRec {
    event_id: String,
    kind: String,
    payload: Value,
}

#[derive(Clone)]
struct CursorRec {
    world_id: String,
    index: usize,
    generation: u64,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, code: &str, message: &str, request_id: Option<&str>) -> ApiResult {
    Err((
        status,
        Json(json!({
            "code": code,
            "message": message,
            "request_id": request_id,
        })),
    ))
}

fn lock_err() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code":"BACKEND_UNAVAILABLE","message":"lock"})),
    )
}

fn next_id(store: &mut Store, prefix: &str) -> String {
    store.next += 1;
    format!("{prefix}-{}", store.next)
}

fn role_is_experimenter(headers: &HeaderMap) -> bool {
    headers
        .get("x-wa-role")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("experimenter"))
        .unwrap_or(false)
}

pub fn router(state: WorldDesState) -> Router {
    Router::new()
        .route("/api/world/des/bind", post(bind_world))
        .route("/api/world/des/observe", post(observe))
        .route("/api/world/des/subscribe", post(subscribe))
        .route("/api/world/des/experiment/trials", post(start_trial))
        .route("/api/world/des/experiment/official", post(start_official))
        .route("/api/world/des/experiment/step", post(step_world))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindBody {
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    factory: Option<String>,
    #[serde(default)]
    scenario: Option<String>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn bind_world(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<BindBody>,
) -> ApiResult {
    if !role_is_experimenter(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可绑定远程世界",
            body.request_id.as_deref(),
        );
    }
    if !state.connector_available {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_DES_UNAVAILABLE",
            "DesConnector 未连接",
            body.request_id.as_deref(),
        );
    }
    // 显式未连接探测（合同：run_id=no-connector）
    if body.run_id.as_deref() == Some("no-connector") {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_DES_UNAVAILABLE",
            "DesConnector 未连接",
            body.request_id.as_deref(),
        );
    }
    let _ = (&body.factory, &body.scenario, body.seed);
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let world_id = next_id(&mut store, "des-world");
    store.worlds.insert(
        world_id.clone(),
        WorldRec {
            world_id: world_id.clone(),
            run_id: body.run_id.clone(),
        },
    );
    store.published.insert(world_id.clone(), Vec::new());
    store.pending.insert(world_id.clone(), Vec::new());
    store.cursor_generation.insert(world_id.clone(), 1);
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": world_id,
        "provenance": "des",
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveBody {
    world_id: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn observe(State(state): State<WorldDesState>, Json(body): Json<ObserveBody>) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let gen = *store.cursor_generation.get(&body.world_id).unwrap_or(&1);
    let events = store
        .published
        .get(&body.world_id)
        .cloned()
        .unwrap_or_default();
    let cursor = next_id(&mut store, "cur");
    // M1：Observe 游标落在已发布末尾；首读返回当前已投递事件快照
    store.cursors.insert(
        cursor.clone(),
        CursorRec {
            world_id: body.world_id.clone(),
            index: events.len(),
            generation: gen,
        },
    );
    let event_vals: Vec<Value> = events
        .iter()
        .map(|e| {
            json!({
                "event_id": e.event_id,
                "kind": e.kind,
                "source": "des",
                "payload": e.payload,
            })
        })
        .collect();
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "des",
        "cursor": cursor,
        "events": event_vals,
        "consistency": "consistent",
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscribeBody {
    world_id: String,
    cursor: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn subscribe(
    State(state): State<WorldDesState>,
    Json(body): Json<SubscribeBody>,
) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let Some(cur) = store.cursors.get(&body.cursor).cloned() else {
        return err(
            StatusCode::CONFLICT,
            "CURSOR_EXPIRED",
            "游标未知或已过期",
            body.request_id.as_deref(),
        );
    };
    if cur.world_id != body.world_id {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "游标与世界不匹配",
            body.request_id.as_deref(),
        );
    }
    let gen = *store.cursor_generation.get(&body.world_id).unwrap_or(&1);
    if cur.generation != gen {
        return err(
            StatusCode::CONFLICT,
            "CURSOR_EXPIRED",
            "游标已过期，请重新 Observe",
            body.request_id.as_deref(),
        );
    }
    let limit = body.limit.unwrap_or(100).clamp(1, 1000);
    let events = store
        .published
        .get(&body.world_id)
        .cloned()
        .unwrap_or_default();
    let slice: Vec<Value> = events
        .iter()
        .skip(cur.index)
        .take(limit)
        .map(|e| {
            json!({
                "event_id": e.event_id,
                "kind": e.kind,
                "source": "des",
                "payload": e.payload,
            })
        })
        .collect();
    let next_index = (cur.index + slice.len()).min(events.len());
    let next_cursor = next_id(&mut store, "cur");
    store.cursors.insert(
        next_cursor.clone(),
        CursorRec {
            world_id: body.world_id.clone(),
            index: next_index,
            generation: gen,
        },
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "des",
        "cursor": next_cursor,
        "events": slice,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepBody {
    world_id: String,
    #[serde(default)]
    minutes: Option<i64>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn step_world(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<StepBody>,
) -> ApiResult {
    if !role_is_experimenter(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可推进",
            body.request_id.as_deref(),
        );
    }
    if !state.connector_available {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_DES_UNAVAILABLE",
            "DesConnector 未连接",
            body.request_id.as_deref(),
        );
    }
    let minutes = body.minutes.unwrap_or(1).max(1);
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    // 先写入 pending（模拟工厂推进），再一次性越过正式投递边界发布
    let evt_id = next_id(&mut store, "evt");
    let pending_evt = EventRec {
        event_id: evt_id.clone(),
        kind: "observation.device".into(),
        payload: json!({
            "minutes": minutes,
            "note": "vibration",
            "metric": "Cpk",
        }),
    };
    store
        .pending
        .entry(body.world_id.clone())
        .or_default()
        .push(pending_evt);
    // 正式投递边界：pending → published（不把工厂游标当产品水位）
    let drained = store
        .pending
        .get_mut(&body.world_id)
        .map(std::mem::take)
        .unwrap_or_default();
    store
        .published
        .entry(body.world_id.clone())
        .or_default()
        .extend(drained);
    if let Some(run) = store
        .runs
        .values_mut()
        .find(|r| r.world_id == body.world_id)
    {
        run.simulation_minute += minutes;
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "des",
        "delivered_event_id": evt_id,
        "minutes": minutes,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialBody {
    #[serde(default)]
    config_version: Option<String>,
    #[serde(default)]
    factory: Option<String>,
    #[serde(default)]
    scenario: Option<String>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    duration_minutes: Option<i64>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn start_trial(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<TrialBody>,
) -> ApiResult {
    if !role_is_experimenter(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可试跑",
            body.request_id.as_deref(),
        );
    }
    if !state.connector_available {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_DES_UNAVAILABLE",
            "DesConnector 未连接",
            body.request_id.as_deref(),
        );
    }
    let _ = (
        &body.factory,
        &body.scenario,
        body.seed,
        body.duration_minutes,
    );
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let config_version = match body.config_version {
        Some(v) if store.configs.contains_key(&v) => v,
        Some(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                "未知配置版本",
                body.request_id.as_deref(),
            );
        }
        None => {
            let v = next_id(&mut store, "cfg");
            store.configs.insert(v.clone(), ());
            v
        }
    };
    let run_id = next_id(&mut store, "trial");
    let world_id = format!("des-world-{run_id}");
    store.runs.insert(
        run_id.clone(),
        RunRec {
            run_id: run_id.clone(),
            world_id: world_id.clone(),
            mode: "trial",
            config_version: config_version.clone(),
            delivery_count: 0,
            case_id: None,
            simulation_minute: 0,
        },
    );
    store.worlds.insert(
        world_id.clone(),
        WorldRec {
            world_id: world_id.clone(),
            run_id: Some(run_id.clone()),
        },
    );
    store.published.insert(world_id.clone(), Vec::new());
    store.pending.insert(world_id.clone(), Vec::new());
    store.cursor_generation.insert(world_id.clone(), 1);
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "trial_run_id": run_id,
        "world_id": world_id,
        "mode": "trial",
        "delivery_count": 0,
        "config_version": config_version,
        "provenance": "des",
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficialBody {
    config_version: Value,
    target_channel: String,
    start_key: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn start_official(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<OfficialBody>,
) -> ApiResult {
    if !role_is_experimenter(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可启动正式运行",
            body.request_id.as_deref(),
        );
    }
    if !state.connector_available {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_DES_UNAVAILABLE",
            "DesConnector 未连接",
            body.request_id.as_deref(),
        );
    }
    let config_version = body
        .config_version
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "INVALID_ARGUMENT",
                    "message": "需要 config_version",
                    "request_id": body.request_id,
                })),
            )
        })?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.configs.contains_key(&config_version) {
        // 允许试跑响应里带回的新版本；若尚无则登记
        store.configs.insert(config_version.clone(), ());
    }
    let run_id = next_id(&mut store, "official");
    let world_id = format!("des-world-{run_id}");
    let case_id = next_id(&mut store, "case");
    store.runs.insert(
        run_id.clone(),
        RunRec {
            run_id: run_id.clone(),
            world_id: world_id.clone(),
            mode: "official",
            config_version,
            delivery_count: 1,
            case_id: Some(case_id.clone()),
            simulation_minute: 0,
        },
    );
    store.worlds.insert(
        world_id.clone(),
        WorldRec {
            world_id: world_id.clone(),
            run_id: Some(run_id.clone()),
        },
    );
    store.cursor_generation.insert(world_id.clone(), 1);
    let event_id = next_id(&mut store, "evt");
    store.published.insert(
        world_id.clone(),
        vec![EventRec {
            event_id,
            kind: "case.opened".into(),
            payload: json!({ "case_id": case_id, "channel": body.target_channel }),
        }],
    );
    store.pending.insert(world_id.clone(), Vec::new());
    let _ = body.start_key;
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "official_run_id": run_id,
        "world_id": world_id,
        "mode": "official",
        "delivery_count": 1,
        "case_id": case_id,
        "provenance": "des",
    })))
}

fn ensure_world(store: &mut Store, world_id: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if store.worlds.contains_key(world_id) {
        return Ok(());
    }
    Err((
        StatusCode::NOT_FOUND,
        Json(json!({
            "code": "WORLD_UNAVAILABLE",
            "message": "未知世界",
        })),
    ))
}
