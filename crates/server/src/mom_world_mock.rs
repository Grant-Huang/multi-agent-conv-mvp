//! World Adapter M0 协议 / 实验控制 Mock（决定 61、62；WA_M0_TASKS M0-1）。
//!
//! 独立内存实现，不改 `engine.rs`。合同见 `docs/mom/WORLD_ADAPTER.md` §4。
//! 故障注入仅在 `allow_test_controls=true` 时开放（测试进程）。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone)]
pub struct WorldMockState {
    inner: Arc<Mutex<Store>>,
    allow_test_controls: bool,
}

impl WorldMockState {
    pub fn new(allow_test_controls: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store::default())),
            allow_test_controls,
        }
    }
}

#[derive(Default)]
struct Store {
    next: u64,
    configs: HashMap<String, ()>,
    runs: HashMap<String, RunRec>,
    start_index: HashMap<(String, String, String), String>,
    proposals: HashMap<String, ProposalRec>,
    exec_keys: HashMap<(String, String), String>,
    executions: HashMap<String, ExecRec>,
    events: HashMap<String, Vec<EventRec>>,
    cursors: HashMap<String, CursorRec>,
    cursor_generation: HashMap<String, u64>,
    ground_truth: HashMap<String, Value>,
}

#[derive(Clone)]
struct RunRec {
    run_id: String,
    world_id: String,
    mode: &'static str,
    config_version: String,
    target_channel: Option<String>,
    simulation_minute: i64,
    status: String,
    delivery_count: u32,
    case_id: Option<String>,
    action_count: u64,
}

#[derive(Clone)]
struct ProposalRec {
    world_id: String,
}

#[derive(Clone)]
struct ExecRec {
    execution_id: String,
    world_id: String,
    proposal_id: String,
    approval_ref: String,
    status: String,
    subactions: Vec<Value>,
    confirmation_policy: String,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Experimenter,
    Operator,
    Auditor,
}

fn role_from(headers: &HeaderMap) -> Role {
    match headers
        .get("x-wa-role")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("experimenter")
    {
        "operator" => Role::Operator,
        "auditor" => Role::Auditor,
        _ => Role::Experimenter,
    }
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

fn digest_of(parts: &[&str]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    format!("sha256-mock-{:016x}", h.finish())
}

fn run_json(run: &RunRec) -> Value {
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "run_id": run.run_id,
        "world_id": run.world_id,
        "mode": run.mode,
        "config_version": run.config_version,
        "target_channel": run.target_channel,
        "simulation_minute": run.simulation_minute,
        "run_status": run.status,
        "delivery_count": run.delivery_count,
        "case_id": run.case_id,
        "action_count": run.action_count,
        "wait_label": if run.status == "paused" { "已下发，等待仿真推进" } else { "" },
        "failed": false,
    })
}

pub fn router(state: WorldMockState) -> Router {
    Router::new()
        .route("/api/world/mock/experiment/configs", post(save_config))
        .route("/api/world/mock/experiment/trials", post(start_trial))
        .route("/api/world/mock/experiment/official", post(start_official))
        .route(
            "/api/world/mock/experiment/runs/{run_id}",
            get(get_run).post(control_run),
        )
        .route("/api/world/mock/observe", post(observe))
        .route("/api/world/mock/subscribe", post(subscribe))
        .route("/api/world/mock/propose", post(propose))
        .route("/api/world/mock/execute", post(execute))
        .route("/api/world/mock/confirm", post(confirm))
        .route("/api/world/mock/ground-truth", get(ground_truth))
        .route("/api/world/mock/test/fault", post(inject_fault))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveConfigBody {
    factory: String,
    scenario: String,
    seed: u64,
    duration_minutes: i64,
    #[serde(default)]
    request_id: Option<String>,
}

async fn save_config(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Json(body): Json<SaveConfigBody>,
) -> ApiResult {
    let _ = (&body.factory, &body.scenario, body.seed);
    if role_from(&headers) != Role::Experimenter {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可保存配置",
            body.request_id.as_deref(),
        );
    }
    if body.duration_minutes <= 0 {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "场景时长必须为正整数",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let version = next_id(&mut store, "cfg");
    store.configs.insert(version.clone(), ());
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "config_version": version,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialBody {
    config_version: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn start_trial(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Json(body): Json<TrialBody>,
) -> ApiResult {
    if role_from(&headers) != Role::Experimenter {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可试跑",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.configs.contains_key(&body.config_version) {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "未知配置版本",
            body.request_id.as_deref(),
        );
    }
    let run_id = next_id(&mut store, "trial");
    let world_id = format!("world-{run_id}");
    store.runs.insert(
        run_id.clone(),
        RunRec {
            run_id: run_id.clone(),
            world_id: world_id.clone(),
            mode: "trial",
            config_version: body.config_version,
            target_channel: None,
            simulation_minute: 0,
            status: "running".into(),
            delivery_count: 0,
            case_id: None,
            action_count: 0,
        },
    );
    store.events.insert(world_id.clone(), Vec::new());
    store.cursor_generation.insert(world_id.clone(), 1);
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "trial_run_id": run_id,
        "world_id": world_id,
        "mode": "trial",
        "delivery_count": 0,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficialBody {
    config_version: String,
    target_channel: String,
    start_key: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn start_official(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Json(body): Json<OfficialBody>,
) -> ApiResult {
    if role_from(&headers) != Role::Experimenter {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可启动正式运行",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.configs.contains_key(&body.config_version) {
        return err(
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            "未知配置版本",
            body.request_id.as_deref(),
        );
    }
    let key = (
        body.config_version.clone(),
        body.target_channel.clone(),
        body.start_key.clone(),
    );
    if let Some(existing) = store.start_index.get(&key).cloned() {
        let run = store.runs.get(&existing).expect("indexed run");
        return Ok(Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "request_id": body.request_id,
            "official_run_id": run.run_id,
            "world_id": run.world_id,
            "mode": "official",
            "delivery_count": run.delivery_count,
            "case_id": run.case_id,
            "already": true,
        })));
    }
    let run_id = next_id(&mut store, "official");
    let world_id = format!("world-{run_id}");
    let case_id = next_id(&mut store, "case");
    store.runs.insert(
        run_id.clone(),
        RunRec {
            run_id: run_id.clone(),
            world_id: world_id.clone(),
            mode: "official",
            config_version: body.config_version.clone(),
            target_channel: Some(body.target_channel.clone()),
            simulation_minute: 0,
            status: "running".into(),
            delivery_count: 1,
            case_id: Some(case_id.clone()),
            action_count: 0,
        },
    );
    store.start_index.insert(key, run_id.clone());
    store.cursor_generation.insert(world_id.clone(), 1);
    store.ground_truth.insert(
        world_id.clone(),
        json!({ "root_cause": "tool_wear", "visible_to": "auditor" }),
    );
    let event_id = next_id(&mut store, "evt");
    store.events.insert(
        world_id.clone(),
        vec![EventRec {
            event_id,
            kind: "case.opened".into(),
            payload: json!({ "case_id": case_id, "channel": body.target_channel }),
        }],
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "official_run_id": run_id,
        "world_id": world_id,
        "mode": "official",
        "delivery_count": 1,
        "case_id": case_id,
        "already": false,
    })))
}

async fn get_run(State(state): State<WorldMockState>, Path(run_id): Path<String>) -> ApiResult {
    let store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(run) = store.runs.get(&run_id) else {
        return err(StatusCode::NOT_FOUND, "INVALID_ARGUMENT", "未知运行", None);
    };
    Ok(Json(run_json(run)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlBody {
    action: String,
    #[serde(default)]
    minutes: Option<i64>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn control_run(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(body): Json<ControlBody>,
) -> ApiResult {
    if role_from(&headers) != Role::Experimenter {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "仅实验员可控制实验时钟",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.runs.contains_key(&run_id) {
        return err(
            StatusCode::NOT_FOUND,
            "INVALID_ARGUMENT",
            "未知运行",
            body.request_id.as_deref(),
        );
    }
    let world_id = store.runs.get(&run_id).unwrap().world_id.clone();
    match body.action.as_str() {
        "pause" => store.runs.get_mut(&run_id).unwrap().status = "paused".into(),
        "resume" => store.runs.get_mut(&run_id).unwrap().status = "running".into(),
        "step" => {
            let minutes = body.minutes.unwrap_or(1).max(1);
            {
                let run = store.runs.get_mut(&run_id).unwrap();
                run.simulation_minute += minutes;
                run.status = "running".into();
            }
            for exec in store.executions.values_mut() {
                if exec.world_id == world_id && exec.status == "accepted" {
                    exec.status = "applied".into();
                    if exec.subactions.is_empty() {
                        exec.subactions.push(json!({
                            "name": "maintenance",
                            "status": "applied",
                        }));
                    } else {
                        for s in &mut exec.subactions {
                            if s["status"] == "accepted" {
                                s["status"] = json!("applied");
                            }
                        }
                    }
                }
            }
        }
        other => {
            return err(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                &format!("未知动作 {other}"),
                body.request_id.as_deref(),
            );
        }
    }
    let run = store.runs.get(&run_id).unwrap().clone();
    let mut out = run_json(&run);
    out["request_id"] = json!(body.request_id);
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveBody {
    world_id: String,
    #[serde(default)]
    selection: Value,
    #[serde(default)]
    request_id: Option<String>,
}

async fn observe(State(state): State<WorldMockState>, Json(body): Json<ObserveBody>) -> ApiResult {
    let _ = body.selection;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(run) = store
        .runs
        .values()
        .find(|r| r.world_id == body.world_id)
        .cloned()
    else {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_UNAVAILABLE",
            "未知世界",
            body.request_id.as_deref(),
        );
    };
    let gen = *store.cursor_generation.get(&body.world_id).unwrap_or(&1);
    // M0 Mock：Observe 游标从流头开始，便于合同测试首读拉到已发布事项事件；
    // M1 起改为发布边界末尾语义。
    let cursor = next_id(&mut store, "cur");
    store.cursors.insert(
        cursor.clone(),
        CursorRec {
            world_id: body.world_id.clone(),
            index: 0,
            generation: gen,
        },
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "objects": [{
            "ref": { "type": "case", "id": run.case_id },
            "source": "mock",
            "state": { "run_status": run.status, "mode": run.mode },
            "observed_at": format!("sim:{}", run.simulation_minute),
        }],
        "as_of": format!("sim:{}", run.simulation_minute),
        "cursor": cursor,
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
    State(state): State<WorldMockState>,
    Json(body): Json<SubscribeBody>,
) -> ApiResult {
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
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
        .events
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
                "subject": { "type": "world", "id": body.world_id },
                "source": "mock",
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
        "events": slice,
        "next_cursor": next_cursor,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeBody {
    world_id: String,
    action_type: String,
    target: Value,
    parameters: Value,
    #[serde(default)]
    source_event_ids: Option<Vec<String>>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn propose(State(state): State<WorldMockState>, Json(body): Json<ProposeBody>) -> ApiResult {
    let _ = body.source_event_ids;
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    if !store.runs.values().any(|r| r.world_id == body.world_id) {
        return err(
            StatusCode::NOT_FOUND,
            "WORLD_UNAVAILABLE",
            "未知世界",
            body.request_id.as_deref(),
        );
    }
    let proposal_id = next_id(&mut store, "prop");
    let target_s = body.target.to_string();
    let params_s = body.parameters.to_string();
    let digest = digest_of(&["1", &body.world_id, &body.action_type, &target_s, &params_s]);
    store.proposals.insert(
        proposal_id.clone(),
        ProposalRec {
            world_id: body.world_id.clone(),
        },
    );
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "proposal_id": proposal_id,
        "action_type": body.action_type,
        "target": body.target,
        "parameters": body.parameters,
        "preconditions": [],
        "confirmation_policy": "maintenance_completed",
        "action_digest": digest,
        "expires_at_simulation_minute": 10_000,
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
    request_id: Option<String>,
}

async fn execute(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Json(body): Json<ExecuteBody>,
) -> ApiResult {
    let role = role_from(&headers);
    if role == Role::Experimenter {
        // 运营员执行；实验员也可在闭环里代测，但跨世界仍拒绝
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
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
            "status": exec.status,
            "confirmation_policy": exec.confirmation_policy,
            "already": true,
        })));
    }
    let execution_id = next_id(&mut store, "exec");
    store.executions.insert(
        execution_id.clone(),
        ExecRec {
            execution_id: execution_id.clone(),
            world_id: body.world_id.clone(),
            proposal_id: body.proposal_id.clone(),
            approval_ref: body.approval_ref.clone(),
            status: "accepted".into(),
            subactions: vec![json!({ "name": "maintenance", "status": "accepted" })],
            confirmation_policy: "maintenance_completed".into(),
        },
    );
    store.exec_keys.insert(key, execution_id.clone());
    if let Some(run) = store
        .runs
        .values_mut()
        .find(|r| r.world_id == body.world_id)
    {
        run.action_count += 1;
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "execution_id": execution_id,
        "status": "accepted",
        "confirmation_policy": "maintenance_completed",
        "already": false,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmBody {
    world_id: String,
    execution_id: String,
    #[serde(default)]
    request_id: Option<String>,
}

async fn confirm(State(state): State<WorldMockState>, Json(body): Json<ConfirmBody>) -> ApiResult {
    let store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(exec) = store.executions.get(&body.execution_id) else {
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
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "world_id": body.world_id,
        "execution_id": exec.execution_id,
        "status": exec.status,
        "confirmation_policy": exec.confirmation_policy,
        "evidence": exec.subactions,
        "subactions": exec.subactions,
    })))
}

#[derive(Debug, Deserialize)]
struct GroundQuery {
    world_id: String,
}

async fn ground_truth(
    State(state): State<WorldMockState>,
    headers: HeaderMap,
    Query(q): Query<GroundQuery>,
) -> ApiResult {
    if role_from(&headers) != Role::Auditor {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "标准答案仅审核可见",
            None,
        );
    }
    let store = state.inner.lock().map_err(|_| lock_err())?;
    let Some(truth) = store.ground_truth.get(&q.world_id) else {
        return err(StatusCode::NOT_FOUND, "WORLD_UNAVAILABLE", "未知世界", None);
    };
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "world_id": q.world_id,
        "ground_truth": truth,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultBody {
    kind: String,
    #[serde(default)]
    world_id: Option<String>,
    #[serde(default)]
    execution_id: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

async fn inject_fault(
    State(state): State<WorldMockState>,
    Json(body): Json<FaultBody>,
) -> ApiResult {
    if !state.allow_test_controls {
        return err(
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "测试控件未开放",
            body.request_id.as_deref(),
        );
    }
    let mut store = state.inner.lock().map_err(|_| lock_err())?;
    match body.kind.as_str() {
        "expire_cursor" => {
            let Some(world_id) = body.world_id.as_deref() else {
                return err(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ARGUMENT",
                    "需要 world_id",
                    body.request_id.as_deref(),
                );
            };
            let g = store
                .cursor_generation
                .entry(world_id.to_string())
                .or_insert(1);
            *g += 1;
        }
        "lose_receipt" => {
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
        }
        "partial_fail" => {
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
                json!({ "name": "tool_transfer", "status": "failed", "reason": "mock partial" }),
            ];
        }
        other => {
            return err(
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                &format!("未知故障 {other}"),
                body.request_id.as_deref(),
            );
        }
    }
    Ok(Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": body.request_id,
        "ok": true,
        "kind": body.kind,
    })))
}
