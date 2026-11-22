//! World Adapter M1/T-50 远程 DES 读 + M2 写合同（`/api/world/des`）。
//!
//! 服务层实现，对齐 `mom_world_mock` 字段形状，`provenance=des`。
//!
//! - 无 `DesConnector`：内存桩（M0/M1/M2 回归）；正式投递水位在本适配器内 pending→commit。
//! - 有 `DesConnector`：`create_simulation` / `tick`；Observe 投影 **tick 成功后** Engine 可见边界。
//!   写路径优先登记执行并调用 `dispatch_writes` 公共 API（不改连接器内部）。
//!
//! M1-2：桩模式投递中断仍服务最近完整边界；真连接器不可半提交（测试控件拒用）。
//! M1-3：试跑零运营投影；正式运行向运营频道投递 `OpsCase`（provenance=des）。
//! M2：Propose / Execute / Confirm + 测试控件（丢回执、部分失败、工厂重启、取消冲刷）。
//! 不改 `engine.rs` / `DesConnector` 内部 / `orchestrator`。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use an_core::ChatCard;
use an_runtime::mom::des::{DesSimulationConfig, Pattern, ScenarioConfig};
use an_runtime::mom::ops_desk::OPS_CHANNEL_ID;
use an_runtime::mom::Engine;
use an_runtime::store::Store as AppStore;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mom_des_connector::{DesConnector, E_DES_UNAVAILABLE};

const PROTOCOL_VERSION: u32 = 1;
const OPS_CHANNEL_TITLE: &str = "运营频道";
const DEFAULT_HORIZON_MINUTES: u64 = 240;
const DEFAULT_START_MINUTE: u64 = 497;

/// `connector_available=false` 时绑定失败（模拟未挂 DesConnector）。
#[derive(Clone)]
pub struct WorldDesState {
    inner: Arc<Mutex<Store>>,
    connector_available: bool,
    allow_test_controls: bool,
    app_store: Option<Arc<AppStore>>,
    /// 与主应用 MOM 共用的引擎（真连接器模式）。
    engine: Option<Arc<Mutex<Engine>>>,
    /// 真模拟工厂连接器；`None` 时走内存桩。
    connector: Option<Arc<DesConnector>>,
}

impl WorldDesState {
    pub fn new(connector_available: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            connector_available,
            allow_test_controls: false,
            app_store: None,
            engine: None,
            connector: None,
        }
    }

    pub fn with_test_controls(connector_available: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            connector_available,
            allow_test_controls: true,
            app_store: None,
            engine: None,
            connector: None,
        }
    }

    pub fn with_app_store(connector_available: bool, app_store: Arc<AppStore>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            connector_available,
            // world_mock 桩路径（Playwright harness）开放测试控件；真连接器路径仍默认关闭。
            allow_test_controls: true,
            app_store: Some(app_store),
            engine: None,
            connector: None,
        }
    }

    /// 主应用已 `with_des`：World Adapter 走真连接器 create/tick，与 MOM 共用 Engine。
    pub fn with_connector(
        connector: Arc<DesConnector>,
        engine: Arc<Mutex<Engine>>,
        app_store: Option<Arc<AppStore>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            connector_available: true,
            allow_test_controls: false,
            app_store,
            engine: Some(engine),
            connector: Some(connector),
        }
    }

    fn live(&self) -> Option<(&Arc<DesConnector>, &Arc<Mutex<Engine>>)> {
        match (&self.connector, &self.engine) {
            (Some(c), Some(e)) => Some((c, e)),
            _ => None,
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
    /// 与 published 同边界的产品快照
    snapshots: HashMap<String, SnapshotRec>,
    /// 工厂侧游标（仅连接器内部语义；产品水位用 publish_seq）
    factory_cursor: HashMap<String, u64>,
    cursors: HashMap<String, CursorRec>,
    cursor_generation: HashMap<String, u64>,
    /// 下一 tick 只 ingest、不 commit（测试控件）
    abort_before_publish: HashSet<String>,
    /// case_id → 运营频道消息 id
    case_messages: HashMap<String, String>,
    /// M2 写路径
    proposals: HashMap<String, ProposalRec>,
    exec_keys: HashMap<(String, String), String>,
    executions: HashMap<String, ExecRec>,
    /// world_id → 当前有效 plan_revision
    world_plan_revision: HashMap<String, String>,
    /// world_id → 工厂实例世代（重启抬升）
    factory_instance: HashMap<String, u64>,
    /// 已取消案例（终态保留）
    cancelled_cases: HashSet<String>,
    /// case_id → 尚未下游发送的缓冲写
    pending_writes: HashMap<String, Vec<PendingWrite>>,
    /// (world_id, correlation_key) → execution_id
    correlation_keys: HashMap<(String, String), String>,
    /// 审核标准答案（仅 auditor）
    ground_truth: HashMap<String, Value>,
    /// 最近一次干预的 execution_id（答案同步补偿用）
    last_intervention: HashMap<String, String>,
}

#[derive(Clone)]
#[allow(dead_code)] // 合同字段；载荷比对 / 审计扩展会读
struct ProposalRec {
    world_id: String,
    case_id: Option<String>,
    plan_revision: String,
    action_type: String,
    target: Value,
    parameters: Value,
    action_digest: String,
    factory_instance: u64,
}

#[derive(Clone)]
#[allow(dead_code)] // plan_revision 供审计扩展
struct ExecRec {
    execution_id: String,
    world_id: String,
    proposal_id: String,
    approval_ref: String,
    plan_revision: String,
    /// 外部关联键（同厂丢回执后按此找回原工单）
    correlation_key: String,
    status: String,
    subactions: Vec<Value>,
    confirmation_policy: String,
    case_id: Option<String>,
    factory_instance: u64,
    dispatched: bool,
}

#[derive(Clone)]
#[allow(dead_code)]
struct PendingWrite {
    execution_id: String,
    proposal_id: String,
}

#[derive(Clone)]
struct SnapshotRec {
    publish_seq: u64,
    as_of: String,
    case_id: Option<String>,
    event_count: usize,
}

#[derive(Clone)]
#[allow(dead_code)] // 合同字段；后续 Observe 对象投影会读
struct WorldRec {
    world_id: String,
    run_id: Option<String>,
    /// 真连接器模式下的仿真编号（与 DesConnector / Engine 对齐）。
    simulation_id: Option<String>,
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn des_ops_card(
    run_id: &str,
    case_id: &str,
    stage: &str,
    summary: &str,
    plan_revision: u64,
) -> ChatCard {
    ChatCard::OpsCase {
        case_id: case_id.into(),
        title: format!("DES · {run_id}"),
        level: "P0".into(),
        stage: stage.into(),
        summary: summary.into(),
        owner: "调度".into(),
        approver: None,
        scope: Some("production".into()),
        plan_revision: Some(plan_revision),
        provenance: "des".into(),
        updated_at: now_secs(),
    }
}

fn deliver_des_ops_card(
    app: &AppStore,
    run_id: &str,
    case_id: &str,
    stage: &str,
    summary: &str,
) -> Result<String, String> {
    app.ensure_conversation(OPS_CHANNEL_ID, OPS_CHANNEL_TITLE)
        .map_err(|e| e.to_string())?;
    let card = des_ops_card(run_id, case_id, stage, summary, 1);
    let content = format!("[DES] 运营案例 {case_id}（运行 {run_id}）");
    let msg = app
        .add_message_with_card(OPS_CHANNEL_ID, "assistant", &content, &card)
        .map_err(|e| e.to_string())?;
    Ok(msg.id)
}

/// 将频道内既有 OpsCase 降为汇总占位，避免共享 harness 下 `.first()` 点到旧执行卡。
fn demote_prior_ops_case_cards(app: &AppStore) {
    let Ok(msgs) = app.list_message_views(OPS_CHANNEL_ID, None) else {
        return;
    };
    for m in msgs {
        let Some(ChatCard::OpsCase { case_id, .. }) = m.card else {
            continue;
        };
        let placeholder = ChatCard::OpsSummary {
            count: 0,
            case_ids: vec![case_id],
        };
        let _ = app.update_message_card(&m.message.id, &placeholder);
    }
}

struct DesOpsCardPatch<'a> {
    run_id: &'a str,
    case_id: &'a str,
    stage: &'a str,
    summary: &'a str,
    plan_revision: u64,
    approver: Option<&'a str>,
}

fn refresh_des_ops_card(
    app: &AppStore,
    message_id: &str,
    p: DesOpsCardPatch<'_>,
) -> Result<(), String> {
    let mut card = des_ops_card(p.run_id, p.case_id, p.stage, p.summary, p.plan_revision);
    if let ChatCard::OpsCase {
        approver: ref mut a,
        ..
    } = card
    {
        *a = p.approver.map(|s| s.into());
    }
    app.update_message_card(message_id, &card)
        .map_err(|e| e.to_string())
}

/// Execute / 批准执行后：投影或刷新运营卡，summary 含 `world_id=` / `execution_id=`。
#[allow(clippy::too_many_arguments)] // 与 mock OpsCardPatch 对称；调用点少
fn upsert_des_execution_card(
    app: &AppStore,
    store: &mut Store,
    world_id: &str,
    case_id: &str,
    run_id: &str,
    execution_id: &str,
    stage: &str,
    approver: Option<&str>,
) {
    let summary = format!(
        "DES 执行已登记 · world_id={world_id} · execution_id={execution_id} · 运行 {run_id}"
    );
    if let Some(mid) = store.case_messages.get(case_id).cloned() {
        let _ = refresh_des_ops_card(
            app,
            &mid,
            DesOpsCardPatch {
                run_id,
                case_id,
                stage,
                summary: &summary,
                plan_revision: 1,
                approver,
            },
        );
    } else {
        match deliver_des_ops_card(app, run_id, case_id, stage, &summary) {
            Ok(mid) => {
                store.case_messages.insert(case_id.to_string(), mid);
            }
            Err(e) => {
                tracing::warn!(
                    event = "task.status",
                    task_id = %run_id,
                    error = %e,
                    "DES Execute 运营频道投影失败"
                );
            }
        }
    }
}

fn role_is_experimenter(headers: &HeaderMap) -> bool {
    headers
        .get("x-wa-role")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("experimenter"))
        .unwrap_or(false)
}

fn des_cfg(name: &str, seed: u64, horizon: u64) -> DesSimulationConfig {
    DesSimulationConfig {
        name: name.to_string(),
        factory: None,
        scenario: ScenarioConfig {
            seed,
            pattern: Pattern::Script,
            start_minute: DEFAULT_START_MINUTE,
            horizon_minutes: horizon.max(1),
            mechanisms: None,
        },
    }
}

fn map_des_err(e: anyhow::Error, request_id: Option<&str>) -> (StatusCode, Json<Value>) {
    let message = format!("{e:#}");
    if message.contains(E_DES_UNAVAILABLE) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "code": E_DES_UNAVAILABLE,
                "message": message,
                "request_id": request_id,
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "INVALID_ARGUMENT",
                "message": message,
                "request_id": request_id,
            })),
        )
    }
}

fn init_world_slots(store: &mut Store, world_id: &str, case_id: Option<String>) {
    store.published.insert(world_id.to_string(), Vec::new());
    store.pending.insert(world_id.to_string(), Vec::new());
    store.factory_cursor.insert(world_id.to_string(), 0);
    store.snapshots.insert(
        world_id.to_string(),
        SnapshotRec {
            publish_seq: 0,
            as_of: "sim:0".into(),
            case_id,
            event_count: 0,
        },
    );
    store.cursor_generation.insert(world_id.to_string(), 1);
    store
        .world_plan_revision
        .entry(world_id.to_string())
        .or_insert_with(|| "rev-1".into());
    store
        .factory_instance
        .entry(world_id.to_string())
        .or_insert(1);
    store
        .ground_truth
        .entry(world_id.to_string())
        .or_insert_with(|| json!({ "answer": "des-ground-truth", "note": "auditor only" }));
}

fn digest_of(parts: &[&str]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    format!("sha256-des-{:016x}", h.finish())
}

/// tick 成功后：把 Engine 已提交案例事件投影为产品水位（去重）；若尚无案例事件则记一笔观测水位。
fn project_engine_after_tick(
    store: &mut Store,
    engine: &Engine,
    world_id: &str,
    simulation_id: &str,
) -> u64 {
    let prefix = format!("{simulation_id}:");
    let mut projected: Vec<EventRec> = Vec::new();
    for case_id in engine.case_ids() {
        let Ok(case) = engine.case(&case_id) else {
            continue;
        };
        if !case.batch.starts_with(&prefix) {
            continue;
        }
        let Ok(events) = engine.events(&case_id, 0, 1000) else {
            continue;
        };
        for ev in events {
            projected.push(EventRec {
                event_id: format!("{}:{}", ev.case_id, ev.sequence),
                kind: ev.kind,
                payload: ev.payload,
            });
        }
    }
    if projected.is_empty() {
        let minute = engine.now();
        projected.push(EventRec {
            event_id: format!("obs:{simulation_id}:{minute}"),
            kind: "observation.device".into(),
            payload: json!({
                "simulation_id": simulation_id,
                "minute": minute,
                "source": "engine_boundary",
            }),
        });
    }
    let published = store.published.entry(world_id.to_string()).or_default();
    let existing: HashSet<String> = published.iter().map(|e| e.event_id.clone()).collect();
    let mut added = 0usize;
    for ev in projected {
        if existing.contains(&ev.event_id) {
            continue;
        }
        published.push(ev);
        added += 1;
    }
    let prev = store
        .snapshots
        .get(world_id)
        .map(|s| s.publish_seq)
        .unwrap_or(0);
    if added == 0 {
        return prev;
    }
    let minute = store
        .runs
        .values()
        .find(|r| r.world_id == world_id)
        .map(|r| r.simulation_minute)
        .unwrap_or(engine.now() as i64);
    let case_id = store
        .runs
        .values()
        .find(|r| r.world_id == world_id)
        .and_then(|r| r.case_id.clone());
    let seq = prev + 1;
    let event_count = published.len();
    store.snapshots.insert(
        world_id.to_string(),
        SnapshotRec {
            publish_seq: seq,
            as_of: format!("sim:{minute}"),
            case_id,
            event_count,
        },
    );
    seq
}

pub fn router(state: WorldDesState) -> Router {
    Router::new()
        .route("/api/world/des/bind", post(bind_world))
        .route("/api/world/des/observe", post(observe))
        .route("/api/world/des/subscribe", post(subscribe))
        .route("/api/world/des/experiment/trials", post(start_trial))
        .route("/api/world/des/experiment/official", post(start_official))
        .route("/api/world/des/experiment/step", post(step_world))
        .route("/api/world/des/propose", post(propose))
        .route("/api/world/des/execute", post(execute))
        .route("/api/world/des/confirm", post(confirm))
        .route(
            "/api/world/des/ops/approve-execute",
            post(ops_approve_execute),
        )
        .route("/api/world/des/ground_truth", post(ground_truth))
        .route(
            "/api/world/des/test/fail-answer-sync",
            post(fail_answer_sync),
        )
        .route(
            "/api/world/des/test/retry-answer-sync",
            post(retry_answer_sync),
        )
        .route(
            "/api/world/des/test/abort-before-publish",
            post(abort_before_publish),
        )
        .route(
            "/api/world/des/test/commit-publish",
            post(commit_publish_route),
        )
        .route("/api/world/des/test/lose-receipt", post(lose_receipt))
        .route("/api/world/des/test/restore-receipt", post(restore_receipt))
        .route("/api/world/des/test/partial-fail", post(partial_fail))
        .route(
            "/api/world/des/test/complete-partial",
            post(complete_partial),
        )
        .route("/api/world/des/test/factory-restart", post(factory_restart))
        .route("/api/world/des/test/bump-plan-revision", post(bump_plan))
        .route("/api/world/des/test/cancel-case", post(cancel_case))
        .route("/api/world/des/test/flush-pending", post(flush_pending))
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
    let seed = body.seed.unwrap_or(1);
    let name = format!("wa-bind-{}", body.scenario.as_deref().unwrap_or("default"));
    let simulation_id = if let Some((conn, engine)) = state.live() {
        let _ = &body.factory;
        conn.create_simulation(engine, des_cfg(&name, seed, DEFAULT_HORIZON_MINUTES))
            .await
            .map_err(|e| map_des_err(e, body.request_id.as_deref()))?
    } else {
        let _ = (&body.factory, &body.scenario);
        String::new()
    };
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let world_id = if simulation_id.is_empty() {
        next_id(&mut store, "des-world")
    } else {
        // 真连接器：world_id 与 simulation_id 对齐，便于投影与排查
        simulation_id.clone()
    };
    store.worlds.insert(
        world_id.clone(),
        WorldRec {
            world_id: world_id.clone(),
            run_id: body.run_id.clone(),
            simulation_id: (!simulation_id.is_empty()).then_some(simulation_id),
        },
    );
    init_world_slots(&mut store, &world_id, None);
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
    let snap = store
        .snapshots
        .get(&body.world_id)
        .cloned()
        .unwrap_or(SnapshotRec {
            publish_seq: 0,
            as_of: "sim:0".into(),
            case_id: None,
            event_count: 0,
        });
    let cursor = next_id(&mut store, "cur");
    // M1：Observe 游标落在已发布末尾；快照与事件同一 publish_seq
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
        "publish_seq": snap.publish_seq,
        "as_of": snap.as_of,
        "objects": [{
            "ref": { "type": "case", "id": snap.case_id },
            "source": "des",
            "state": {
                "publish_seq": snap.publish_seq,
                "event_count": snap.event_count,
            },
            "observed_at": snap.as_of,
        }],
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

    // 真连接器：tick 成功 = 正式投递边界；不可半提交
    if let Some((conn, engine)) = state.live() {
        let simulation_id = {
            let store = state.inner.lock().map_err(|_| lock_err())?;
            ensure_world_locked(&store, &body.world_id)?;
            store
                .worlds
                .get(&body.world_id)
                .and_then(|w| w.simulation_id.clone())
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "code": "INVALID_ARGUMENT",
                            "message": "世界未绑定远程仿真",
                            "request_id": body.request_id,
                        })),
                    )
                })?
        };
        let to_minute = {
            let e = engine.lock().map_err(|_| lock_err())?;
            e.now().saturating_add(minutes as u64)
        };
        conn.tick(engine, to_minute)
            .await
            .map_err(|e| map_des_err(e, body.request_id.as_deref()))?;
        let mut store = state.inner.lock().map_err(|_| lock_err())?;
        if let Some(run) = store
            .runs
            .values_mut()
            .find(|r| r.world_id == body.world_id)
        {
            run.simulation_minute += minutes;
        }
        let factory = store
            .factory_cursor
            .entry(body.world_id.clone())
            .or_insert(0);
        *factory += minutes as u64;
        let e = engine.lock().map_err(|_| lock_err())?;
        let seq = project_engine_after_tick(&mut store, &e, &body.world_id, &simulation_id);
        let delivered = store
            .published
            .get(&body.world_id)
            .and_then(|v| v.last())
            .map(|ev| ev.event_id.clone())
            .unwrap_or_default();
        return Ok(Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": body.request_id,
            "world_id": body.world_id,
            "provenance": "des",
            "delivered_event_id": delivered,
            "published": true,
            "publish_seq": seq,
            "minutes": minutes,
            "simulation_id": simulation_id,
        })));
    }

    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    // 工厂游标推进（内部）；产品水位仅在 commit_publish 后前进
    let factory = store
        .factory_cursor
        .entry(body.world_id.clone())
        .or_insert(0);
    *factory += minutes as u64;
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
    if let Some(run) = store
        .runs
        .values_mut()
        .find(|r| r.world_id == body.world_id)
    {
        run.simulation_minute += minutes;
    }
    let abort = store.abort_before_publish.remove(&body.world_id);
    if abort {
        return Ok(Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": body.request_id,
            "world_id": body.world_id,
            "provenance": "des",
            "ingested_event_id": evt_id,
            "published": false,
            "minutes": minutes,
        })));
    }
    let seq = commit_publish(&mut store, &body.world_id);
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "provenance": "des",
        "delivered_event_id": evt_id,
        "published": true,
        "publish_seq": seq,
        "minutes": minutes,
    })))
}

fn ensure_world_locked(store: &Store, world_id: &str) -> Result<(), (StatusCode, Json<Value>)> {
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

/// pending → published（按 event_id 去重）；仅有新事件时推进 publish_seq 与快照。
fn commit_publish(store: &mut Store, world_id: &str) -> u64 {
    let drained = store
        .pending
        .get_mut(world_id)
        .map(std::mem::take)
        .unwrap_or_default();
    let published = store.published.entry(world_id.to_string()).or_default();
    let existing: HashSet<String> = published.iter().map(|e| e.event_id.clone()).collect();
    let mut added = 0usize;
    for ev in drained {
        if existing.contains(&ev.event_id) {
            continue;
        }
        published.push(ev);
        added += 1;
    }
    let prev = store
        .snapshots
        .get(world_id)
        .map(|s| s.publish_seq)
        .unwrap_or(0);
    if added == 0 {
        return prev;
    }
    let minute = store
        .runs
        .values()
        .find(|r| r.world_id == world_id)
        .map(|r| r.simulation_minute)
        .unwrap_or(0);
    let case_id = store
        .runs
        .values()
        .find(|r| r.world_id == world_id)
        .and_then(|r| r.case_id.clone());
    let seq = prev + 1;
    let event_count = published.len();
    store.snapshots.insert(
        world_id.to_string(),
        SnapshotRec {
            publish_seq: seq,
            as_of: format!("sim:{minute}"),
            case_id,
            event_count,
        },
    );
    seq
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldOnlyBody {
    world_id: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn abort_before_publish(
    State(state): State<WorldDesState>,
    Json(body): Json<WorldOnlyBody>,
) -> ApiResult {
    if state.live().is_some() {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "真实 tick 不可半提交；测试控件仅桩模式可用",
            body.request_id.as_deref(),
        );
    }
    if !state.allow_test_controls {
        return err(
            StatusCode::NOT_FOUND,
            "UNSUPPORTED_CAPABILITY",
            "测试控件未开启",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    store.abort_before_publish.insert(body.world_id.clone());
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "armed": true,
    })))
}

async fn commit_publish_route(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<WorldOnlyBody>,
) -> ApiResult {
    if state.live().is_some() {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "真实 tick 不可半提交；测试控件仅桩模式可用",
            body.request_id.as_deref(),
        );
    }
    if !state.allow_test_controls {
        return err(
            StatusCode::NOT_FOUND,
            "UNSUPPORTED_CAPABILITY",
            "测试控件未开启",
            body.request_id.as_deref(),
        );
    }
    if !role_is_experimenter(&headers) {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可提交发布",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let seq = commit_publish(&mut store, &body.world_id);
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "publish_seq": seq,
        "provenance": "des",
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
    let seed = body.seed.unwrap_or(1);
    let horizon = body.duration_minutes.unwrap_or(60).max(1) as u64;
    let _ = (&body.factory, &body.scenario);
    let simulation_id = if let Some((conn, engine)) = state.live() {
        conn.create_simulation(engine, des_cfg("wa-trial", seed, horizon))
            .await
            .map_err(|e| map_des_err(e, body.request_id.as_deref()))?
    } else {
        String::new()
    };
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
    let world_id = if simulation_id.is_empty() {
        format!("des-world-{run_id}")
    } else {
        simulation_id.clone()
    };
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
            simulation_id: (!simulation_id.is_empty()).then_some(simulation_id),
        },
    );
    init_world_slots(&mut store, &world_id, None);
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
    #[serde(default)]
    config_version: Option<Value>,
    /// 已 bind 的世界：可省略 config_version，在该世界上启动正式运行（E2E-WALOOP）。
    #[serde(default)]
    world_id: Option<String>,
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
    let bound_world = body
        .world_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let config_version = body
        .config_version
        .as_ref()
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .or_else(|| bound_world.as_ref().map(|w| format!("cfg-{w}")))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "INVALID_ARGUMENT",
                    "message": "需要 config_version 或已绑定的 world_id",
                    "request_id": body.request_id,
                })),
            )
        })?;
    let simulation_id = if bound_world.is_some() {
        String::new()
    } else if let Some((conn, engine)) = state.live() {
        conn.create_simulation(engine, des_cfg("wa-official", 1, DEFAULT_HORIZON_MINUTES))
            .await
            .map_err(|e| map_des_err(e, body.request_id.as_deref()))?
    } else {
        String::new()
    };
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if let Some(ref wid) = bound_world {
        if !store.worlds.contains_key(wid) {
            return err(
                StatusCode::NOT_FOUND,
                "WORLD_UNAVAILABLE",
                "未知世界，请先 bind",
                body.request_id.as_deref(),
            );
        }
    }
    if !store.configs.contains_key(&config_version) {
        // 允许试跑响应里带回的新版本；若尚无则登记
        store.configs.insert(config_version.clone(), ());
    }
    let run_id = next_id(&mut store, "official");
    let demote_prior = bound_world.is_some();
    let world_id = if let Some(wid) = bound_world {
        wid
    } else if simulation_id.is_empty() {
        format!("des-world-{run_id}")
    } else {
        simulation_id.clone()
    };
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
            simulation_id: (!simulation_id.is_empty()).then_some(simulation_id),
        },
    );
    store.cursor_generation.insert(world_id.clone(), 1);
    store.factory_cursor.insert(world_id.clone(), 0);
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
    store.snapshots.insert(
        world_id.clone(),
        SnapshotRec {
            publish_seq: 1,
            as_of: "sim:0".into(),
            case_id: Some(case_id.clone()),
            event_count: 1,
        },
    );
    store
        .world_plan_revision
        .insert(world_id.clone(), "rev-1".into());
    store.factory_instance.insert(world_id.clone(), 1);
    store.ground_truth.insert(
        world_id.clone(),
        json!({ "answer": "des-ground-truth", "note": "auditor only" }),
    );
    let _ = body.start_key;
    drop(store);

    // M1-3：正式运行向运营频道投递 OpsCase（试跑不投递）；provenance=des
    if let Some(app) = &state.app_store {
        if demote_prior {
            demote_prior_ops_case_cards(app);
            if let Ok(mut store) = state.inner.lock() {
                store.case_messages.clear();
            }
        }
        let summary = format!("DES 正式运行事项，待主管决定 · world_id={world_id} · 运行 {run_id}");
        match deliver_des_ops_card(app, &run_id, &case_id, "WAITING_HUMAN", &summary) {
            Ok(mid) => {
                if let Ok(mut store) = state.inner.lock() {
                    store.case_messages.insert(case_id.clone(), mid);
                }
            }
            Err(e) => {
                tracing::warn!(
                    event = "task.status",
                    task_id = %run_id,
                    error = %e,
                    "DES 运营频道投影失败"
                );
            }
        }
    }

    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "official_run_id": run_id,
        "world_id": world_id,
        "mode": "official",
        "delivery_count": 1,
        "case_id": case_id,
        "provenance": "des",
        "publish_seq": 1,
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

fn require_test_controls(state: &WorldDesState, request_id: Option<&str>) -> ApiResult {
    if state.allow_test_controls {
        return Ok(Json(json!({})));
    }
    err(
        StatusCode::FORBIDDEN,
        "ACCESS_DENIED",
        "测试控件未开放",
        request_id,
    )
}

fn case_id_for_world(store: &Store, world_id: &str) -> Option<String> {
    store
        .runs
        .values()
        .find(|r| r.world_id == world_id)
        .and_then(|r| r.case_id.clone())
        .or_else(|| {
            store
                .snapshots
                .get(world_id)
                .and_then(|s| s.case_id.clone())
        })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeBody {
    world_id: String,
    action_type: String,
    target: Value,
    parameters: Value,
    #[serde(default)]
    plan_revision: Option<String>,
    #[serde(default)]
    source_event_ids: Option<Vec<String>>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn propose(State(state): State<WorldDesState>, Json(body): Json<ProposeBody>) -> ApiResult {
    let _ = body.source_event_ids;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let factory_instance = *store.factory_instance.get(&body.world_id).unwrap_or(&1);
    let current_rev = store
        .world_plan_revision
        .get(&body.world_id)
        .cloned()
        .unwrap_or_else(|| "rev-1".into());
    let plan_revision = body
        .plan_revision
        .clone()
        .unwrap_or_else(|| current_rev.clone());
    // 显式 plan_revision：与世界当前对齐，便于后续 Execute（E2E-WADYN rev-dyn）。
    // 测试控件 bump 仍可抬升；抬升后旧提案 Execute 会因 current_rev 不匹配而失败。
    if body.plan_revision.is_some() && plan_revision != current_rev {
        store
            .world_plan_revision
            .insert(body.world_id.clone(), plan_revision.clone());
    }
    let case_id = case_id_for_world(&store, &body.world_id);
    let target_s = body.target.to_string();
    let params_s = body.parameters.to_string();
    let digest = digest_of(&[
        "1",
        &body.world_id,
        &body.action_type,
        &target_s,
        &params_s,
        &plan_revision,
    ]);
    let proposal_id = next_id(&mut store, "prop");
    store.proposals.insert(
        proposal_id.clone(),
        ProposalRec {
            world_id: body.world_id.clone(),
            case_id: case_id.clone(),
            plan_revision: plan_revision.clone(),
            action_type: body.action_type.clone(),
            target: body.target.clone(),
            parameters: body.parameters.clone(),
            action_digest: digest.clone(),
            factory_instance,
        },
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "proposal_id": proposal_id,
        "case_id": case_id,
        "plan_revision": plan_revision,
        "action_type": body.action_type,
        "target": body.target,
        "parameters": body.parameters,
        "preconditions": [],
        "confirmation_policy": "maintenance_completed",
        "action_digest": digest,
        "provenance": "des",
        "factory_instance": factory_instance,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteBody {
    world_id: String,
    proposal_id: String,
    approval_ref: String,
    idempotency_key: String,
    #[serde(default)]
    plan_revision: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn execute(State(state): State<WorldDesState>, Json(body): Json<ExecuteBody>) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let Some(proposal) = store.proposals.get(&body.proposal_id).cloned() else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "未知提案",
            body.request_id.as_deref(),
        );
    };
    if proposal.world_id != body.world_id {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "跨运行审批被拒绝",
            body.request_id.as_deref(),
        );
    }
    let factory_instance = *store.factory_instance.get(&body.world_id).unwrap_or(&1);
    if proposal.factory_instance != factory_instance {
        return err(
            StatusCode::CONFLICT,
            "FACTORY_INSTANCE_STALE",
            "工厂实例已变更，旧运行不可再写",
            body.request_id.as_deref(),
        );
    }
    let current_rev = store
        .world_plan_revision
        .get(&body.world_id)
        .cloned()
        .unwrap_or_else(|| "rev-1".into());
    let exec_rev = body
        .plan_revision
        .clone()
        .unwrap_or_else(|| proposal.plan_revision.clone());
    if exec_rev != proposal.plan_revision || exec_rev != current_rev {
        return err(
            StatusCode::CONFLICT,
            "APPROVAL_INVALID",
            "方案修订或前置条件已变化",
            body.request_id.as_deref(),
        );
    }
    let key = (body.world_id.clone(), body.idempotency_key.clone());
    if let Some(existing) = store.exec_keys.get(&key).cloned() {
        let exec = store.executions.get(&existing).unwrap();
        if exec.proposal_id != body.proposal_id || exec.approval_ref != body.approval_ref {
            return err(
                StatusCode::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
                "同幂等键载荷冲突",
                body.request_id.as_deref(),
            );
        }
        return Ok(Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": body.request_id,
            "world_id": body.world_id,
            "execution_id": exec.execution_id,
            "correlation_key": exec.correlation_key,
            "status": exec.status,
            "confirmation_policy": exec.confirmation_policy,
            "already": true,
            "provenance": "des",
        })));
    }
    let case_id = proposal
        .case_id
        .clone()
        .or_else(|| case_id_for_world(&store, &body.world_id));
    let cancelled = case_id
        .as_ref()
        .map(|c| store.cancelled_cases.contains(c))
        .unwrap_or(false);
    let execution_id = next_id(&mut store, "exec");
    let correlation_key = format!("des-order-{}", execution_id);
    let mut status = "accepted".to_string();
    let mut dispatched = true;
    if cancelled {
        // 案例已取消：不向下游发送；登记为 cancelled 证据
        dispatched = false;
        status = "cancelled".into();
    }
    let subactions = vec![json!({ "name": "maintenance", "status": status })];
    store.executions.insert(
        execution_id.clone(),
        ExecRec {
            execution_id: execution_id.clone(),
            world_id: body.world_id.clone(),
            proposal_id: body.proposal_id.clone(),
            approval_ref: body.approval_ref.clone(),
            plan_revision: exec_rev,
            correlation_key: correlation_key.clone(),
            status: status.clone(),
            subactions: subactions.clone(),
            confirmation_policy: "maintenance_completed".into(),
            case_id: case_id.clone(),
            factory_instance,
            dispatched,
        },
    );
    store.exec_keys.insert(key, execution_id.clone());
    store.correlation_keys.insert(
        (body.world_id.clone(), correlation_key.clone()),
        execution_id.clone(),
    );
    if !dispatched {
        if let Some(cid) = &case_id {
            store
                .pending_writes
                .entry(cid.clone())
                .or_default()
                .push(PendingWrite {
                    execution_id: execution_id.clone(),
                    proposal_id: body.proposal_id.clone(),
                });
        }
    }
    // 干预：执行维修后追加未来事件（前缀不变）
    if dispatched && proposal.action_type.contains("maintenance") {
        let evt_id = next_id(&mut store, "evt");
        let published = store.published.entry(body.world_id.clone()).or_default();
        published.push(EventRec {
            event_id: evt_id,
            kind: "intervention.maintenance".into(),
            payload: json!({
                "execution_id": execution_id,
                "effect": "future_changed",
            }),
        });
        let event_count = published.len();
        let seq = store
            .snapshots
            .get(&body.world_id)
            .map(|s| s.publish_seq + 1)
            .unwrap_or(1);
        let case_id_snap = store
            .snapshots
            .get(&body.world_id)
            .and_then(|s| s.case_id.clone());
        store.snapshots.insert(
            body.world_id.clone(),
            SnapshotRec {
                publish_seq: seq,
                as_of: format!("sim:intervention:{seq}"),
                case_id: case_id_snap,
                event_count,
            },
        );
        store
            .last_intervention
            .insert(body.world_id.clone(), execution_id.clone());
        store.ground_truth.insert(
            body.world_id.clone(),
            json!({
                "answer": "maintenance-applied",
                "execution_id": execution_id,
                "intervention_id": execution_id,
                "sync_status": "synced",
                "note": "auditor only"
            }),
        );
    }
    // M2-5：Execute 成功后投影运营卡（含 execution_id，供 E2E-WAEXEC）
    let project_case = case_id
        .clone()
        .unwrap_or_else(|| next_id(&mut store, "case"));
    if case_id.is_none() {
        if let Some(snap) = store.snapshots.get_mut(&body.world_id) {
            snap.case_id = Some(project_case.clone());
        }
    }
    let run_id = store
        .runs
        .values()
        .find(|r| r.world_id == body.world_id)
        .map(|r| r.run_id.clone())
        .unwrap_or_else(|| format!("exec-run-{}", body.world_id));
    if let Some(app) = state.app_store.clone() {
        upsert_des_execution_card(
            &app,
            &mut store,
            &body.world_id,
            &project_case,
            &run_id,
            &execution_id,
            "VERIFYING",
            None,
        );
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "execution_id": execution_id,
        "correlation_key": correlation_key,
        "status": "accepted",
        "confirmation_policy": "maintenance_completed",
        "already": false,
        "provenance": "des",
        "dispatched": dispatched,
        "case_terminal": if cancelled { json!("CANCELLED") } else { json!(null) },
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpsApproveExecuteBody {
    case_id: String,
    #[serde(default)]
    plan_revision: Option<u64>,
    #[serde(default)]
    request_id: Option<String>,
}

/// 运营卡「批准」：登记提案并 Execute，回写同一 OpsCase（E2E-WALOOP）。
async fn ops_approve_execute(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<OpsApproveExecuteBody>,
) -> ApiResult {
    let role = headers
        .get("x-wa-role")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("operator");
    if role.eq_ignore_ascii_case("auditor") {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "审核员不可批准执行",
            body.request_id.as_deref(),
        );
    }
    let Some(app) = state.app_store.clone() else {
        return err(
            StatusCode::NOT_FOUND,
            "BACKEND_UNAVAILABLE",
            "未挂载运营投影",
            body.request_id.as_deref(),
        );
    };
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(message_id) = store.case_messages.get(&body.case_id).cloned() else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知事项",
            body.request_id.as_deref(),
        );
    };
    let run = store
        .runs
        .values()
        .find(|r| r.case_id.as_deref() == Some(body.case_id.as_str()))
        .cloned();
    let Some(run) = run else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "事项未绑定正式运行",
            body.request_id.as_deref(),
        );
    };
    let world_id = run.world_id.clone();
    let factory_instance = *store.factory_instance.get(&world_id).unwrap_or(&1);
    let plan_revision = store
        .world_plan_revision
        .get(&world_id)
        .cloned()
        .unwrap_or_else(|| {
            body.plan_revision
                .map(|n| format!("rev-{n}"))
                .unwrap_or_else(|| "rev-1".into())
        });
    let digest = digest_of(&[
        "1",
        &world_id,
        "eam.maintenance",
        r#"{"type":"equipment","id":"CNC-1"}"#,
        r#"{"priority":"high"}"#,
        &plan_revision,
    ]);
    let proposal_id = next_id(&mut store, "prop");
    store.proposals.insert(
        proposal_id.clone(),
        ProposalRec {
            world_id: world_id.clone(),
            case_id: Some(body.case_id.clone()),
            plan_revision: plan_revision.clone(),
            action_type: "eam.maintenance".into(),
            target: json!({ "type": "equipment", "id": "CNC-1" }),
            parameters: json!({ "priority": "high" }),
            action_digest: digest,
            factory_instance,
        },
    );
    let idem = format!("ops-approve-{}", body.case_id);
    let key = (world_id.clone(), idem.clone());
    if let Some(existing) = store.exec_keys.get(&key).cloned() {
        let exec = store.executions.get(&existing).unwrap().clone();
        let summary = format!(
            "已批准 · world_id={world_id} · execution_id={} · 运行 {}",
            exec.execution_id, run.run_id
        );
        drop(store);
        let _ = refresh_des_ops_card(
            &app,
            &message_id,
            DesOpsCardPatch {
                run_id: &run.run_id,
                case_id: &body.case_id,
                stage: "VERIFYING",
                summary: &summary,
                plan_revision: body.plan_revision.unwrap_or(1),
                approver: Some("supervisor"),
            },
        );
        return Ok(Json(json!({
            "already": true,
            "approver": "supervisor",
            "execution_id": exec.execution_id,
            "world_id": world_id,
            "protocol_version": PROTOCOL_VERSION,
            "provenance": "des",
        })));
    }
    let execution_id = next_id(&mut store, "exec");
    let correlation_key = format!("des-order-{execution_id}");
    store.executions.insert(
        execution_id.clone(),
        ExecRec {
            execution_id: execution_id.clone(),
            world_id: world_id.clone(),
            proposal_id: proposal_id.clone(),
            approval_ref: format!("ops-approve-{}", body.case_id),
            plan_revision: plan_revision.clone(),
            correlation_key: correlation_key.clone(),
            status: "accepted".into(),
            subactions: vec![json!({ "name": "maintenance", "status": "accepted" })],
            confirmation_policy: "maintenance_completed".into(),
            case_id: Some(body.case_id.clone()),
            factory_instance,
            dispatched: true,
        },
    );
    store.exec_keys.insert(key, execution_id.clone());
    store
        .correlation_keys
        .insert((world_id.clone(), correlation_key), execution_id.clone());
    let summary = format!(
        "已批准 · world_id={world_id} · execution_id={execution_id} · 运行 {}",
        run.run_id
    );
    upsert_des_execution_card(
        &app,
        &mut store,
        &world_id,
        &body.case_id,
        &run.run_id,
        &execution_id,
        "VERIFYING",
        Some("supervisor"),
    );
    // upsert 用了 execution 摘要；再写批准人摘要
    if let Some(mid) = store.case_messages.get(&body.case_id).cloned() {
        let _ = refresh_des_ops_card(
            &app,
            &mid,
            DesOpsCardPatch {
                run_id: &run.run_id,
                case_id: &body.case_id,
                stage: "VERIFYING",
                summary: &summary,
                plan_revision: body.plan_revision.unwrap_or(1),
                approver: Some("supervisor"),
            },
        );
    }
    let _ = message_id;
    Ok(Json(json!({
        "already": false,
        "approver": "supervisor",
        "execution_id": execution_id,
        "world_id": world_id,
        "protocol_version": PROTOCOL_VERSION,
        "provenance": "des",
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmBody {
    world_id: String,
    #[serde(default)]
    execution_id: Option<String>,
    /// 同厂丢回执后按原关联键找回
    #[serde(default)]
    correlation_key: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn confirm(State(state): State<WorldDesState>, Json(body): Json<ConfirmBody>) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let factory_instance = *store.factory_instance.get(&body.world_id).unwrap_or(&1);
    let resolved_id = if let Some(id) = body.execution_id.as_deref() {
        Some(id.to_string())
    } else if let Some(ck) = body.correlation_key.as_deref() {
        store
            .correlation_keys
            .get(&(body.world_id.clone(), ck.to_string()))
            .cloned()
    } else {
        None
    };
    let Some(execution_id) = resolved_id else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "需要 execution_id 或 correlation_key",
            body.request_id.as_deref(),
        );
    };
    let Some(exec) = store.executions.get(&execution_id).cloned() else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知执行",
            body.request_id.as_deref(),
        );
    };
    if exec.world_id != body.world_id {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "执行不属于该世界",
            body.request_id.as_deref(),
        );
    }
    // 工厂重启后：Confirm 只查历史证据，不因实例变更拒查（WORLD_ADAPTER §5/§6.2）。
    // 仅当旧实例执行仍处 accepted 且试图推进写路径时拒绝；known 终态与 unknown/applied 可查。
    let stale_instance = exec.factory_instance != factory_instance;
    let case_terminal = exec
        .case_id
        .as_ref()
        .filter(|c| store.cancelled_cases.contains(c.as_str()))
        .map(|_| "CANCELLED");
    let mut status = exec.status.clone();
    let mut subactions = exec.subactions.clone();
    if stale_instance && status == "accepted" {
        // 重启后未决写进入待查证，不推进到 confirmed
        status = "unknown".into();
        if let Some(e) = store.executions.get_mut(&execution_id) {
            e.status = status.clone();
        }
    } else if !stale_instance && (status == "accepted" || status == "applied") {
        status = "confirmed".into();
        subactions = vec![json!({ "name": "maintenance", "status": "confirmed" })];
        if let Some(e) = store.executions.get_mut(&execution_id) {
            e.status = status.clone();
            e.subactions = subactions.clone();
        }
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "execution_id": exec.execution_id,
        "correlation_key": exec.correlation_key,
        "status": status,
        "confirmation_policy": exec.confirmation_policy,
        "evidence": subactions,
        "subactions": subactions,
        "provenance": "des",
        "case_terminal": case_terminal,
        "factory_instance_stale": stale_instance,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundBody {
    world_id: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn ground_truth(
    State(state): State<WorldDesState>,
    headers: HeaderMap,
    Json(body): Json<GroundBody>,
) -> ApiResult {
    let role = headers
        .get("x-wa-role")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !role.eq_ignore_ascii_case("auditor") {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "标准答案仅审核可见",
            body.request_id.as_deref(),
        );
    }
    let store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(truth) = store.ground_truth.get(&body.world_id) else {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_UNAVAILABLE",
            "未知世界",
            body.request_id.as_deref(),
        );
    };
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "world_id": body.world_id,
        "ground_truth": truth,
        "provenance": "des",
    })))
}

/// 模拟审核答案刷新失败 → pending（WORLD_ADAPTER §6.3）。
async fn fail_answer_sync(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let intervention_id = store
        .last_intervention
        .get(&body.world_id)
        .cloned()
        .or_else(|| body.execution_id.clone());
    let Some(iid) = intervention_id else {
        return err(
            StatusCode::CONFLICT,
            "PRECONDITION_FAILED",
            "尚无干预可同步",
            body.request_id.as_deref(),
        );
    };
    store.ground_truth.insert(
        body.world_id.clone(),
        json!({
            "answer": null,
            "execution_id": iid,
            "intervention_id": iid,
            "sync_status": "pending",
            "note": "auditor only; sync failed"
        }),
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "fail_answer_sync",
        "sync_status": "pending",
    })))
}

/// 独立补偿重试：按最近干预恢复与干预一致的审核答案。
async fn retry_answer_sync(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let Some(iid) = store.last_intervention.get(&body.world_id).cloned() else {
        return err(
            StatusCode::CONFLICT,
            "PRECONDITION_FAILED",
            "尚无干预可同步",
            body.request_id.as_deref(),
        );
    };
    store.ground_truth.insert(
        body.world_id.clone(),
        json!({
            "answer": "maintenance-applied",
            "execution_id": iid,
            "intervention_id": iid,
            "sync_status": "synced",
            "note": "auditor only; retried"
        }),
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "retry_answer_sync",
        "sync_status": "synced",
        "intervention_id": iid,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecCtrlBody {
    world_id: String,
    #[serde(default)]
    execution_id: Option<String>,
    #[serde(default)]
    case_id: Option<String>,
    #[serde(default)]
    plan_revision: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn lose_receipt(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(exec_id) = body.execution_id.as_deref() else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "需要 execution_id",
            body.request_id.as_deref(),
        );
    };
    let Some(exec) = store.executions.get_mut(exec_id) else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知执行",
            body.request_id.as_deref(),
        );
    };
    exec.status = "unknown".into();
    exec.subactions = vec![json!({ "name": "maintenance", "status": "unknown" })];
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "lose_receipt",
    })))
}

async fn restore_receipt(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(exec_id) = body.execution_id.as_deref() else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "需要 execution_id",
            body.request_id.as_deref(),
        );
    };
    let Some(exec) = store.executions.get_mut(exec_id) else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知执行",
            body.request_id.as_deref(),
        );
    };
    exec.status = "applied".into();
    exec.subactions = vec![json!({ "name": "maintenance", "status": "applied" })];
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "restore_receipt",
        "execution_id": exec_id,
    })))
}

async fn partial_fail(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(exec_id) = body.execution_id.as_deref() else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "需要 execution_id",
            body.request_id.as_deref(),
        );
    };
    let Some(exec) = store.executions.get_mut(exec_id) else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知执行",
            body.request_id.as_deref(),
        );
    };
    exec.status = "failed".into();
    exec.subactions = vec![
        json!({ "name": "maintenance", "status": "applied" }),
        json!({ "name": "tool_transfer", "status": "failed", "reason": "des partial" }),
    ];
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "partial_fail",
    })))
}

/// 部分失败后补齐剩余子动作回执（同厂恢复）。
async fn complete_partial(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(exec_id) = body.execution_id.as_deref() else {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "需要 execution_id",
            body.request_id.as_deref(),
        );
    };
    let Some(exec_snap) = store.executions.get(exec_id).cloned() else {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知执行",
            body.request_id.as_deref(),
        );
    };
    let current = *store
        .factory_instance
        .get(&exec_snap.world_id)
        .unwrap_or(&1);
    if exec_snap.factory_instance != current {
        return err(
            StatusCode::CONFLICT,
            "FACTORY_INSTANCE_STALE",
            "工厂实例已变更，不可补齐旧运行回执",
            body.request_id.as_deref(),
        );
    }
    if exec_snap.status != "failed" && exec_snap.status != "unknown" {
        return err(
            StatusCode::CONFLICT,
            "PRECONDITION_FAILED",
            "仅 failed/unknown 可补齐部分回执",
            body.request_id.as_deref(),
        );
    }
    let exec = store.executions.get_mut(exec_id).unwrap();
    let mut completed = 0u32;
    for s in &mut exec.subactions {
        let st = s.get("status").and_then(|v| v.as_str());
        if st == Some("failed") || st == Some("unknown") {
            s["status"] = json!("applied");
            s["completed_by"] = json!("complete_partial");
            completed += 1;
        }
    }
    exec.status = "applied".into();
    let status = exec.status.clone();
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "complete_partial",
        "execution_id": exec_id,
        "completed_subactions": completed,
        "status": status,
    })))
}

async fn factory_restart(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let gen = store
        .factory_instance
        .entry(body.world_id.clone())
        .or_insert(1);
    *gen += 1;
    let new_gen = *gen;
    // 清空该世界相关待发，防止误投新实例
    let case_ids: Vec<String> = store
        .runs
        .values()
        .filter(|r| r.world_id == body.world_id)
        .filter_map(|r| r.case_id.clone())
        .chain(
            store
                .snapshots
                .get(&body.world_id)
                .and_then(|s| s.case_id.clone()),
        )
        .collect();
    let mut cleared_pending = 0usize;
    for cid in &case_ids {
        if let Some(p) = store.pending_writes.remove(cid) {
            cleared_pending += p.len();
        }
    }
    // 未决 accepted → unknown（待查证）；已有证据保留
    for exec in store.executions.values_mut() {
        if exec.world_id == body.world_id && exec.status == "accepted" {
            exec.status = "unknown".into();
            exec.subactions = vec![json!({ "name": "maintenance", "status": "unknown" })];
        }
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": "factory_restart",
        "factory_instance": new_gen,
        "cleared_pending": cleared_pending,
    })))
}

async fn bump_plan(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let next = body
        .plan_revision
        .unwrap_or_else(|| format!("rev-{}", now_secs()));
    store
        .world_plan_revision
        .insert(body.world_id.clone(), next.clone());
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "plan_revision": next,
    })))
}

async fn cancel_case(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let case_id = body
        .case_id
        .or_else(|| case_id_for_world(&store, &body.world_id))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "INVALID_ARGUMENT",
                    "message": "需要 case_id",
                    "request_id": body.request_id,
                })),
            )
        })?;
    store.cancelled_cases.insert(case_id.clone());
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "case_id": case_id,
        "case_terminal": "CANCELLED",
    })))
}

async fn flush_pending(
    State(state): State<WorldDesState>,
    Json(body): Json<ExecCtrlBody>,
) -> ApiResult {
    let _ = require_test_controls(&state, body.request_id.as_deref())?;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    ensure_world(&mut store, &body.world_id)?;
    let current_factory = *store.factory_instance.get(&body.world_id).unwrap_or(&1);
    let case_id = body
        .case_id
        .or_else(|| case_id_for_world(&store, &body.world_id))
        .unwrap_or_default();
    let pending = store.pending_writes.remove(&case_id).unwrap_or_default();
    let mut cancelled_ids = Vec::new();
    let mut rejected_stale = Vec::new();
    for p in &pending {
        if let Some(exec) = store.executions.get_mut(&p.execution_id) {
            if exec.factory_instance != current_factory {
                // 旧工厂实例待发：不误投新实例
                exec.dispatched = false;
                rejected_stale.push(p.execution_id.clone());
                continue;
            }
            exec.dispatched = false;
            exec.status = "cancelled".into();
            exec.subactions = vec![json!({ "name": "maintenance", "status": "cancelled" })];
            cancelled_ids.push(p.execution_id.clone());
        }
    }
    // 迟到回执：已发执行只更新证据，不改案例终态（仅同工厂实例）
    let late_updated = store
        .executions
        .values_mut()
        .filter(|e| {
            e.world_id == body.world_id
                && e.dispatched
                && e.factory_instance == current_factory
                && e.case_id.as_deref() == Some(case_id.as_str())
        })
        .map(|e| {
            e.subactions
                .push(json!({ "name": "late_receipt", "status": "recorded" }));
            e.execution_id.clone()
        })
        .collect::<Vec<_>>();
    let terminal = if store.cancelled_cases.contains(&case_id) {
        "CANCELLED"
    } else {
        "ACTIVE"
    };
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "cancelled_pending": cancelled_ids,
        "rejected_stale_pending": rejected_stale,
        "late_receipts_recorded": late_updated,
        "case_terminal": terminal,
        "dispatched_count": 0,
    })))
}
