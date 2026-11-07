//! agent 准入接口（E3）：`/api/admission/*`。未开启 `[admission]` 时一律返回 404。
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET  | /api/admission/agents | 全部准入记录 |
//! | POST | /api/admission/agents | S1 提交 Agent Card，返回澄清问题 |
//! | GET  | /api/admission/agents/{id} | 某个 agent 的记录与审计事件 |
//! | POST | /api/admission/agents/{id}/clarify | S2 回答澄清问题 `{"answers": [...]}` |
//! | POST | /api/admission/agents/{id}/score | S3/S4 三项分数（0–1），返回总分与推荐等级 |
//! | POST | /api/admission/agents/{id}/contract | S5 协作契约 |
//! | POST | /api/admission/agents/{id}/approve | S6 人工审批 `{"approver"}` |
//! | POST | /api/admission/agents/{id}/activate | S7 登记接入信息并激活，注册为外部 agent |
//! | POST | /api/admission/agents/{id}/checks | 回写试用期抽检 `{"task_id", "passed"}` |
//! | POST | /api/admission/agents/{id}/revoke | 撤权 `{"reason"}`，中止其进行中的任务 |
//!
//! 与现有的任务审批接口一样，这些接口没有单独的身份认证（服务默认只监听本机，见 DESIGN）；
//! 企业身份与职责权限属于后续工作。

use std::sync::Arc;

use an_core::codes;
use an_runtime::admission::{
    A2aBinding, AdmissionLedger, AgentClaim, CheckOutcome, Contract, Scores,
};
use an_runtime::Runtime;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppError;

type AppState = Arc<Runtime>;
type ApiResult = Result<Json<Value>, AppError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admission/agents", get(list).post(submit))
        .route("/api/admission/agents/{id}", get(detail))
        .route("/api/admission/agents/{id}/clarify", post(clarify))
        .route("/api/admission/agents/{id}/score", post(score))
        .route("/api/admission/agents/{id}/contract", post(contract))
        .route("/api/admission/agents/{id}/approve", post(approve))
        .route("/api/admission/agents/{id}/activate", post(activate))
        .route("/api/admission/agents/{id}/checks", post(check))
        .route("/api/admission/agents/{id}/revoke", post(revoke))
}

/// 未开启准入时返回 404
fn ledger(rt: &Runtime) -> Result<&Arc<AdmissionLedger>, AppError> {
    rt.admission
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "准入未开启".into()))
}

/// 已开启准入且 agent 存在
fn known(rt: &Runtime, id: &str) -> Result<Arc<AdmissionLedger>, AppError> {
    let l = ledger(rt)?;
    if l.get(id).is_none() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            format!("agent {id} 不存在"),
        ));
    }
    Ok(l.clone())
}

/// 账本错误码 → HTTP 状态：步骤顺序不对 409，输入不合法 400，其他 500
fn map_err(e: anyhow::Error) -> AppError {
    let msg = format!("{e:#}");
    let status = if msg.contains(codes::ADMISSION_STATE) {
        StatusCode::CONFLICT
    } else if msg.contains(codes::ADMISSION_INVALID) {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    AppError(status, msg)
}

async fn list(State(rt): State<AppState>) -> ApiResult {
    Ok(Json(json!(ledger(&rt)?.list())))
}

async fn submit(State(rt): State<AppState>, Json(claim): Json<AgentClaim>) -> ApiResult {
    let questions = ledger(&rt)?.submit(claim).map_err(map_err)?;
    Ok(Json(json!({ "questions": questions })))
}

async fn detail(State(rt): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let l = known(&rt, &id)?;
    let events = l.events(&id, 500)?;
    Ok(Json(json!({ "record": l.get(&id), "events": events })))
}

#[derive(Deserialize)]
struct Answers {
    answers: Vec<String>,
}

async fn clarify(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Answers>,
) -> ApiResult {
    known(&rt, &id)?.clarify(&id, b.answers).map_err(map_err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn score(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(s): Json<Scores>,
) -> ApiResult {
    let (total, level) = known(&rt, &id)?.score(&id, s).map_err(map_err)?;
    Ok(Json(json!({ "total": total, "recommended": level })))
}

async fn contract(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(c): Json<Contract>,
) -> ApiResult {
    known(&rt, &id)?.contract(&id, c).map_err(map_err)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Approver {
    approver: String,
}

async fn approve(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Approver>,
) -> ApiResult {
    known(&rt, &id)?
        .approve(&id, &b.approver)
        .map_err(map_err)?;
    Ok(Json(json!({ "ok": true })))
}

/// S7：先登记接入信息，再激活，最后注册为外部 agent
async fn activate(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<A2aBinding>,
) -> ApiResult {
    let l = known(&rt, &id)?;
    l.set_binding(&id, b).map_err(map_err)?;
    let epoch = l.activate(&id).map_err(map_err)?;
    rt.register_a2a(&id)?;
    Ok(Json(json!({ "epoch": epoch })))
}

#[derive(Deserialize)]
struct Check {
    task_id: String,
    passed: bool,
}

async fn check(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(c): Json<Check>,
) -> ApiResult {
    let l = known(&rt, &id)?;
    let outcome = l
        .record_check_for(&id, &c.task_id, c.passed)
        .map_err(map_err)?;
    if outcome == CheckOutcome::Downgraded {
        // 降级后 epoch 已变：唤醒排队任务按新等级重新挑选
        rt.registry.wake();
    }
    let outcome = match outcome {
        CheckOutcome::Recorded => "recorded",
        CheckOutcome::Warning => "warning",
        CheckOutcome::Downgraded => "downgraded",
    };
    Ok(Json(json!({ "outcome": outcome, "record": l.get(&id) })))
}

#[derive(Deserialize)]
struct Reason {
    reason: String,
}

async fn revoke(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<Reason>,
) -> ApiResult {
    let l = ledger(&rt)?;
    let known_agent = l.get(&id).is_some() || rt.registry.cards().iter().any(|c| c.id == id);
    if !known_agent {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            format!("agent {id} 不存在"),
        ));
    }
    let started = std::time::Instant::now();
    let report = rt
        .orchestrator
        .revoke_agent(&id, &b.reason)
        .map_err(map_err)?;
    Ok(Json(json!({
        "epoch": report.epoch,
        "aborted": report.aborted,
        "elapsed_ms": started.elapsed().as_millis() as u64,
    })))
}
