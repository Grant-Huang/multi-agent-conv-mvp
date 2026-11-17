//! MOM 车间 DES 仿真的只读接口（设计文档 `docs/mom/DES_CLOSED_LOOP.md` §9，阶段②）。
//!
//! 供会话 agent 与制造桌面查询：工厂可观测状态、趋势预测、what-if 方案比较，以及创建 DES 仿真。
//! 返回值均标注 `provenance: simulation`，不含标准答案与隐状态；查询接口不改变仿真世界。
//! 与 `mom.rs` 共用同一个引擎和状态，路由在 `mom::router` 中合并。

use crate::mom::AppState;
use an_runtime::mom::des::{DesSimulationConfig, WhatIfRequest};
use an_runtime::mom::Engine;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

/// 业务错误统一返回 400 和说明文字。
fn bad_request(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": e.to_string()})),
    )
}

/// 在阻塞线程中等待引擎锁并执行一次调用，不占用 Tokio 异步工作线程（R-20261117-L1，移植自 PR #85）。
///
/// 计算量有确定性上限：创建时的拒绝抽样受设备·分钟预算限制，查询复用遥测缓存。
/// 阻塞闭包开始后不能通过取消 HTTP 请求强制中断；它会在有界时间内完成并释放锁，被拒绝的创建不写状态。
async fn with_engine(
    state: AppState,
    f: impl FnOnce(&mut Engine) -> anyhow::Result<Value> + Send + 'static,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let mut engine = state
            .engine
            .lock()
            .map_err(|_| bad_request("runtime unavailable"))?;
        f(&mut engine).map(Json).map_err(bad_request)
    })
    .await
    .map_err(|_| bad_request("runtime unavailable"))?
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mom/des/simulations", post(create))
        .route("/api/mom/factory", get(factory))
        .route("/api/mom/factory/forecast", get(forecast))
        .route("/api/mom/factory/whatif", post(whatif))
        .route("/api/mom/reviews/{case_id}", get(review_report))
        .route("/api/mom/reviews/{case_id}/ai", post(ai_review))
        .route("/api/mom/reviews/{case_id}/decision", post(human_review))
}

/// 审核报告（阶段④）：规则检查、AI 审核记录、人工结论与分歧。
async fn review_report(State(s): State<AppState>, Path(case_id): Path<String>) -> ApiResult {
    with_engine(s, move |e| e.des_review_report(&case_id)).await
}

/// AI 审核的超时（阶段④）。
const AI_REVIEW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 触发 AI 审核；未配置审核模型时返回规则检查结果并标注 AI 审核不可用。
async fn ai_review(State(s): State<AppState>, Path(case_id): Path<String>) -> ApiResult {
    let Some(reviewer) = s.reviewer.clone() else {
        return with_engine(s, move |e| e.des_review_report(&case_id)).await;
    };
    an_runtime::mom::des::review::run_ai_review(
        &s.engine,
        &case_id,
        reviewer.as_ref(),
        AI_REVIEW_TIMEOUT,
    )
    .await
    .map(Json)
    .map_err(bad_request)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanReviewBody {
    decision: String,
    #[serde(default)]
    reason: String,
    actor: String,
}

/// 人工确认或推翻 AI 审核结论（推翻必须写理由）。
async fn human_review(
    State(s): State<AppState>,
    Path(case_id): Path<String>,
    Json(body): Json<HumanReviewBody>,
) -> ApiResult {
    with_engine(s, move |e| {
        e.des_human_review(&case_id, &body.decision, &body.reason, &body.actor)?;
        e.des_review_report(&case_id)
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimulationQuery {
    simulation: String,
}

/// what-if 请求体：仿真编号加 `WhatIfRequest` 的字段（不用 flatten，以便严格拒绝未知字段）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhatIfBody {
    simulation_id: String,
    asset: String,
    quantity: u64,
    due_minute: u64,
}

async fn create(State(s): State<AppState>, Json(config): Json<DesSimulationConfig>) -> ApiResult {
    with_engine(s, move |e| {
        Ok(json!({"simulation_id": e.create_des_simulation(config)?, "provenance": "simulation"}))
    })
    .await
}

async fn factory(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    with_engine(s, move |e| e.des_factory_view(&q.simulation)).await
}

async fn forecast(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    with_engine(s, move |e| e.des_forecast(&q.simulation)).await
}

async fn whatif(State(s): State<AppState>, Json(body): Json<WhatIfBody>) -> ApiResult {
    let request = WhatIfRequest {
        asset: body.asset,
        quantity: body.quantity,
        due_minute: body.due_minute,
    };
    with_engine(s, move |e| e.des_whatif(&body.simulation_id, &request)).await
}
