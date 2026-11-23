//! World Adapter M3 生产只读（`/api/world/prod`）。
//!
//! 服务层内存 stub：站点权威源登记 → Bind / Observe / Subscribe。
//! `provenance=production`；字段对齐 `mom_world_des` 读合同。
//! 不改 `engine.rs` / `store` 表结构 / 凭据 egress；真站点另开升级方案。
//! 写路径（Propose/Execute）本模块不挂载（M3-3/M4）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
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
}

impl WorldProdState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            authorities: default_authorities(),
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
    cursors: HashMap<String, CursorRec>,
    cursor_generation: HashMap<String, u64>,
}

#[derive(Clone)]
#[allow(dead_code)] // site/authority 供后续权限卡（M3-3）读取
struct WorldRec {
    world_id: String,
    site_id: String,
    authority: String,
}

#[derive(Clone)]
struct SnapshotRec {
    publish_seq: u64,
    as_of: String,
    source: String,
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
    // stub：固定日期 + epoch 秒，便于水位对齐断言
    format!("2026-11-22T00:00:00Z+{secs}")
}

pub fn router(state: WorldProdState) -> Router {
    Router::new()
        .route("/api/world/prod/bind", post(bind_world))
        .route("/api/world/prod/observe", post(observe))
        .route("/api/world/prod/subscribe", post(subscribe))
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

async fn bind_world(State(state): State<WorldProdState>, Json(body): Json<BindBody>) -> ApiResult {
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
    let _ = &body.sources;

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
    let source = "mes";
    let evt_id = next_id(&mut store, "evt");
    store.published.insert(
        world_id.clone(),
        vec![EventRec {
            event_id: evt_id,
            kind: "equipment.status".into(),
            source: source.into(),
            payload: json!({
                "site_id": body.site_id,
                "equipment": "CNC-1",
                "status": "running",
            }),
        }],
    );
    store.snapshots.insert(
        world_id.clone(),
        SnapshotRec {
            publish_seq: 1,
            as_of: as_of.clone(),
            source: source.into(),
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
}

async fn observe(State(state): State<WorldProdState>, Json(body): Json<ObserveBody>) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.worlds.contains_key(&body.world_id) {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    }
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
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "production",
        "cursor": cursor,
        "events": event_vals,
        "publish_seq": snap.publish_seq,
        "as_of": snap.as_of,
        "objects": [{
            "ref": { "type": "equipment", "id": "CNC-1" },
            "source": snap.source,
            "state": {
                "publish_seq": snap.publish_seq,
                "status": "running",
            },
            "observed_at": snap.as_of,
        }],
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
    Json(body): Json<SubscribeBody>,
) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.worlds.contains_key(&body.world_id) {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_NOT_FOUND",
            "世界不存在或未绑定",
            body.request_id.as_deref(),
        );
    }
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
