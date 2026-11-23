//! World Adapter M3 生产只读（`/api/world/prod`）。
//!
//! 服务层内存 stub：站点权威源登记 → Bind / Observe / Subscribe；
//! M3-3：跨站/无授权拒绝、游标过期续读、多源延迟控件、写路径显式关闭。
//! `provenance=production`；字段对齐 `mom_world_des` 读合同。
//! 不改 `engine.rs` / `store` 表结构 / 凭据 egress；真站点另开升级方案。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{json, Value};

const PROTOCOL_VERSION: u32 = 1;

/// 已登记的生产站点权威源（stub；非真实凭据）。
fn default_authorities() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("plant-east".into(), "mes-stub".into());
    m.insert("plant-west".into(), "mes-stub".into());
    m
}

#[derive(Clone)]
pub struct WorldProdState {
    inner: Arc<Mutex<Store>>,
    /// site_id → authority id
    authorities: HashMap<String, String>,
    allow_test_controls: bool,
    work_order_fixture: Option<Arc<WorkOrderFixtureReader>>,
}

impl WorldProdState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            authorities: default_authorities(),
            allow_test_controls: false,
            work_order_fixture: None,
        }
    }

    pub fn with_test_controls() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            authorities: default_authorities(),
            allow_test_controls: true,
            work_order_fixture: None,
        }
    }
}

type WorkOrderFixtureReader = dyn Fn(
        String,
        String,
        String,
    ) -> BoxFuture<'static, Result<WorkOrderFixture, WorkOrderFixtureError>>
    + Send
    + Sync;

pub struct WorkOrderFixture {
    pub source: String,
    pub observed_at: String,
    pub state: Value,
}

pub enum WorkOrderFixtureError {
    AccessDenied,
    NotVisible,
    SourceUnavailable,
    InvalidSourceResponse,
}

impl WorldProdState {
    /// Explicit injection for in-process acceptance using loopback fixture sources.
    pub fn with_work_order_fixture(
        reader: impl Fn(
                String,
                String,
                String,
            ) -> BoxFuture<'static, Result<WorkOrderFixture, WorkOrderFixtureError>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            work_order_fixture: Some(Arc::new(reader)),
            ..Self::new()
        }
    }
}

impl Default for WorldProdState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct Store {
    next: u64,
    worlds: HashMap<String, WorldRec>,
    published: HashMap<String, Vec<EventRec>>,
    snapshots: HashMap<String, SnapshotRec>,
    /// world_id → 多源对象（Observe 投影）
    source_objects: HashMap<String, Vec<SourceObj>>,
    cursors: HashMap<String, CursorRec>,
    cursor_generation: HashMap<String, u64>,
}

#[derive(Clone)]
struct WorldRec {
    #[allow(dead_code)]
    world_id: String,
    site_id: String,
    #[allow(dead_code)]
    authority: String,
}

#[derive(Clone)]
struct SnapshotRec {
    publish_seq: u64,
    as_of: String,
    source: String,
}

#[derive(Clone)]
struct SourceObj {
    source: String,
    observed_at: String,
    delayed: bool,
    delay_ms: u64,
    status: String,
}

#[derive(Clone)]
struct EventRec {
    event_id: String,
    kind: String,
    source: String,
    payload: Value,
}

#[derive(Clone)]
struct CursorRec {
    world_id: String,
    index: usize,
    generation: u64,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(code: StatusCode, err_code: &str, message: &str, request_id: Option<&str>) -> ApiResult {
    Err((
        code,
        Json(json!({
            "code": err_code,
            "message": message,
            "request_id": request_id.unwrap_or(""),
            "protocol_version": PROTOCOL_VERSION,
        })),
    ))
}

fn lock_err() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code":"BACKEND_UNAVAILABLE","message":"lock poisoned"})),
    )
}

fn next_id(store: &mut Store, prefix: &str) -> String {
    store.next += 1;
    format!("{prefix}-{}", store.next)
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("2026-11-22T00:00:00Z+{secs}")
}

fn role_allowed(headers: &HeaderMap) -> bool {
    matches!(
        headers
            .get("x-wa-role")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "operator" | "experimenter" | "auditor"
    )
}

fn header_site(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-wa-site").and_then(|v| v.to_str().ok())
}

fn deny(
    code: StatusCode,
    err_code: &str,
    message: &str,
    request_id: Option<&str>,
) -> (StatusCode, Json<Value>) {
    (
        code,
        Json(json!({
            "code": err_code,
            "message": message,
            "request_id": request_id.unwrap_or(""),
            "protocol_version": PROTOCOL_VERSION,
        })),
    )
}

fn authorize_read(
    headers: &HeaderMap,
    world: &WorldRec,
    request_id: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if !role_allowed(headers) {
        return Err(deny(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "无授权身份不可读生产世界",
            request_id,
        ));
    }
    if let Some(site) = header_site(headers) {
        if site != world.site_id {
            return Err(deny(
                StatusCode::FORBIDDEN,
                "ACCESS_DENIED",
                "跨站点读被拒绝",
                request_id,
            ));
        }
    }
    Ok(())
}

fn require_test_controls(
    state: &WorldProdState,
    request_id: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if state.allow_test_controls {
        Ok(())
    } else {
        Err(deny(
            StatusCode::FORBIDDEN,
            "TEST_CONTROLS_DISABLED",
            "测试控件未开启",
            request_id,
        ))
    }
}

pub fn router(state: WorldProdState) -> Router {
    Router::new()
        .route("/api/world/prod/bind", post(bind_world))
        .route("/api/world/prod/observe", post(observe))
        .route("/api/world/prod/subscribe", post(subscribe))
        .route("/api/world/prod/propose", post(propose))
        .route("/api/world/prod/execute", post(execute))
        .route("/api/world/prod/test/expire-cursor", post(expire_cursor))
        .route("/api/world/prod/test/inject-delay", post(inject_delay))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindBody {
    site_id: String,
    #[serde(default)]
    authority: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
}

async fn bind_world(
    State(state): State<WorldProdState>,
    headers: HeaderMap,
    Json(body): Json<BindBody>,
) -> ApiResult {
    if !role_allowed(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "无授权身份不可绑定生产世界",
            body.request_id.as_deref(),
        );
    }
    let Some(authority) = body.authority.as_deref().filter(|s| !s.is_empty()) else {
        return err(
            StatusCode::BAD_REQUEST,
            "AUTHORITY_REQUIRED",
            "生产绑定须提供权威源 authority",
            body.request_id.as_deref(),
        );
    };
    let Some(expected) = state.authorities.get(&body.site_id) else {
        return err(
            StatusCode::BAD_REQUEST,
            "AUTHORITY_NOT_CONFIGURED",
            "站点未配置权威源",
            body.request_id.as_deref(),
        );
    };
    if expected != authority {
        return err(
            StatusCode::BAD_REQUEST,
            "AUTHORITY_NOT_CONFIGURED",
            "权威源与站点登记不一致",
            body.request_id.as_deref(),
        );
    }

    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let world_id = next_id(&mut store, "prod-world");
    store.worlds.insert(
        world_id.clone(),
        WorldRec {
            world_id: world_id.clone(),
            site_id: body.site_id.clone(),
            authority: authority.to_string(),
        },
    );
    let as_of = now_rfc3339();
    let sources = body.sources.clone().unwrap_or_else(|| vec!["mes".into()]);
    let primary = sources.first().cloned().unwrap_or_else(|| "mes".into());
    let evt_id = next_id(&mut store, "evt");
    store.published.insert(
        world_id.clone(),
        vec![EventRec {
            event_id: evt_id,
            kind: "equipment.status".into(),
            source: primary.clone(),
            payload: json!({
                "site_id": body.site_id,
                "equipment": "CNC-1",
                "status": "running",
            }),
        }],
    );
    let objs: Vec<SourceObj> = sources
        .iter()
        .map(|src| SourceObj {
            source: src.clone(),
            observed_at: as_of.clone(),
            delayed: false,
            delay_ms: 0,
            status: "running".into(),
        })
        .collect();
    store.source_objects.insert(world_id.clone(), objs);
    store.snapshots.insert(
        world_id.clone(),
        SnapshotRec {
            publish_seq: 1,
            as_of: as_of.clone(),
            source: primary,
        },
    );
    store.cursor_generation.insert(world_id.clone(), 1);

    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": world_id,
        "provenance": "production",
        "site_id": body.site_id,
        "authority": authority,
        "capabilities": {
            "observations": true,
            "write": false,
            "actions": false,
            "mode": "production",
        },
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveBody {
    world_id: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    selection: Option<ObserveSelection>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ObserveSelection {
    WorkOrder { id: String },
}

async fn observe_work_order_fixture(
    state: &WorldProdState,
    headers: &HeaderMap,
    world_id: &str,
    request_id: Option<&str>,
    object_ref: String,
) -> ApiResult {
    let world = state
        .inner
        .lock()
        .map_err(|_| lock_err())?
        .worlds
        .get(world_id)
        .cloned();
    let Some(world) = world else {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            request_id,
        );
    };
    authorize_read(headers, &world, request_id)?;
    if header_site(headers) != Some(world.site_id.as_str()) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "工单查询须匹配绑定站点",
            request_id,
        );
    }
    let Some(read) = &state.work_order_fixture else {
        return err(
            StatusCode::FORBIDDEN,
            "TEST_CONTROLS_DISABLED",
            "工单验收夹具未配置",
            request_id,
        );
    };
    if object_ref.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_WORK_ORDER_SELECTION",
            "工单引用为空",
            request_id,
        );
    }

    let observed = match read(world_id.to_owned(), world.site_id, object_ref.clone()).await {
        Ok(observed) => observed,
        Err(error) => {
            let (status, code, message) = match error {
                WorkOrderFixtureError::AccessDenied => {
                    (StatusCode::FORBIDDEN, "ACCESS_DENIED", "工单引用未授权")
                }
                WorkOrderFixtureError::NotVisible => {
                    (StatusCode::NOT_FOUND, "ORDER_NOT_VISIBLE", "工单不可见")
                }
                WorkOrderFixtureError::SourceUnavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "SOURCE_UNAVAILABLE",
                    "来源不可用",
                ),
                WorkOrderFixtureError::InvalidSourceResponse => (
                    StatusCode::BAD_GATEWAY,
                    "INVALID_SOURCE_RESPONSE",
                    "来源响应无效",
                ),
            };
            return err(status, code, message, request_id);
        }
    };
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "world_id": world_id,
        "provenance": "fixture",
        "cursor": null,
        "publish_seq": null,
        "events": [],
        "as_of": observed.observed_at,
        "consistency": "per_source",
        "objects": [{
            "ref": { "type": "work_order", "id": object_ref },
            "source": observed.source,
            "observed_at": observed.observed_at,
            "state": observed.state
        }],
    })))
}

async fn observe(
    State(state): State<WorldProdState>,
    headers: HeaderMap,
    Json(body): Json<ObserveBody>,
) -> ApiResult {
    if let Some(ObserveSelection::WorkOrder { id }) = body.selection {
        return observe_work_order_fixture(
            &state,
            &headers,
            &body.world_id,
            body.request_id.as_deref(),
            id,
        )
        .await;
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(world) = store.worlds.get(&body.world_id).cloned() else {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    };
    authorize_read(&headers, &world, body.request_id.as_deref())?;
    let gen = *store.cursor_generation.get(&body.world_id).unwrap_or(&1);
    let events = store
        .published
        .get(&body.world_id)
        .cloned()
        .unwrap_or_default();
    let snap = store
        .snapshots
        .get(&body.world_id)
        .cloned()
        .unwrap_or(SnapshotRec {
            publish_seq: 0,
            as_of: "1970-01-01T00:00:00Z".into(),
            source: "mes".into(),
        });
    let objs = store
        .source_objects
        .get(&body.world_id)
        .cloned()
        .unwrap_or_default();
    let cursor = next_id(&mut store, "cur");
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
                "source": e.source,
                "payload": e.payload,
            })
        })
        .collect();
    let object_vals: Vec<Value> = objs
        .iter()
        .map(|o| {
            json!({
                "ref": { "type": "equipment", "id": "CNC-1" },
                "source": o.source,
                "delayed": o.delayed,
                "state": {
                    "publish_seq": snap.publish_seq,
                    "status": o.status,
                    "delay_ms": o.delay_ms,
                },
                "observed_at": o.observed_at,
            })
        })
        .collect();
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "production",
        "cursor": cursor,
        "events": event_vals,
        "publish_seq": snap.publish_seq,
        "as_of": snap.as_of,
        "objects": object_vals,
        "consistency": "per_source",
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
    State(state): State<WorldProdState>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(world) = store.worlds.get(&body.world_id).cloned() else {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    };
    authorize_read(&headers, &world, body.request_id.as_deref())?;
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
    let snap = store
        .snapshots
        .get(&body.world_id)
        .cloned()
        .unwrap_or(SnapshotRec {
            publish_seq: 0,
            as_of: "1970-01-01T00:00:00Z".into(),
            source: "mes".into(),
        });
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
                "source": e.source,
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
        "provenance": "production",
        "cursor": next_cursor,
        "events": slice,
        "publish_seq": snap.publish_seq,
        "as_of": snap.as_of,
        "source": snap.source,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldOnlyBody {
    world_id: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn expire_cursor(
    State(state): State<WorldProdState>,
    Json(body): Json<WorldOnlyBody>,
) -> ApiResult {
    require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.worlds.contains_key(&body.world_id) {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    }
    let gen = store
        .cursor_generation
        .entry(body.world_id.clone())
        .or_insert(1);
    *gen += 1;
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "cursor_generation": *gen,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InjectDelayBody {
    world_id: String,
    source: String,
    delay_ms: u64,
    #[serde(default)]
    request_id: Option<String>,
}

async fn inject_delay(
    State(state): State<WorldProdState>,
    Json(body): Json<InjectDelayBody>,
) -> ApiResult {
    require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.worlds.contains_key(&body.world_id) {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    }
    let delayed_as_of = format!("delay:{}ms:{}", body.delay_ms, now_rfc3339());
    let objs = store
        .source_objects
        .entry(body.world_id.clone())
        .or_default();
    let mut found = false;
    for o in objs.iter_mut() {
        if o.source == body.source {
            o.delayed = true;
            o.delay_ms = body.delay_ms;
            o.observed_at = delayed_as_of.clone();
            found = true;
        }
    }
    if !found {
        objs.push(SourceObj {
            source: body.source.clone(),
            observed_at: delayed_as_of.clone(),
            delayed: true,
            delay_ms: body.delay_ms,
            status: "stale".into(),
        });
    }
    if let Some(snap) = store.snapshots.get_mut(&body.world_id) {
        // 水位对齐到延迟源截至时间（合同：水位与载荷截至时间一致）
        snap.as_of = delayed_as_of.clone();
        snap.source = body.source.clone();
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "source": body.source,
        "delay_ms": body.delay_ms,
        "as_of": delayed_as_of,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeBody {
    world_id: String,
    #[serde(default)]
    action_type: Option<String>,
    #[serde(default)]
    target: Option<Value>,
    #[serde(default)]
    parameters: Option<Value>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn propose(State(_state): State<WorldProdState>, Json(body): Json<ProposeBody>) -> ApiResult {
    let _ = (
        &body.world_id,
        &body.action_type,
        &body.target,
        &body.parameters,
    );
    err(
        StatusCode::FORBIDDEN,
        "WRITE_DISABLED",
        "生产世界不开放 Propose；写演示请使用 Mock/DES",
        body.request_id.as_deref(),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteBody {
    world_id: String,
    #[serde(default)]
    proposal_id: Option<String>,
    #[serde(default)]
    approval_ref: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn execute(State(_state): State<WorldProdState>, Json(body): Json<ExecuteBody>) -> ApiResult {
    let _ = (
        &body.world_id,
        &body.proposal_id,
        &body.approval_ref,
        &body.idempotency_key,
    );
    err(
        StatusCode::FORBIDDEN,
        "WRITE_DISABLED",
        "生产世界不开放 Execute；写演示请使用 Mock/DES",
        body.request_id.as_deref(),
    )
}
