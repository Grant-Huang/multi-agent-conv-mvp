//! MOM 车间 DES 仿真的只读接口（设计文档 `docs/mom/DES_CLOSED_LOOP.md` §9，阶段②）。
//!
//! 供会话 agent 与制造桌面查询：工厂可观测状态、趋势预测、what-if 方案比较，以及创建 DES 仿真。
//! 返回值均标注 `provenance: simulation`，不含标准答案与隐状态；查询接口不改变仿真世界。
//! 与 `mom.rs` 共用同一个引擎和状态，路由在 `mom::router` 中合并。

use crate::mom::AppState;
use an_runtime::mom::des::{DesSimulationConfig, WhatIfRequest};
use an_runtime::mom::Engine;
use axum::{
    extract::{Query, State},
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

/// 在引擎锁内执行一次调用；计算都有上限，不会长时间占用锁。
fn with_engine(
    state: &AppState,
    f: impl FnOnce(&mut Engine) -> anyhow::Result<Value>,
) -> ApiResult {
    let mut engine = state
        .engine
        .lock()
        .map_err(|_| bad_request("runtime unavailable"))?;
    f(&mut engine).map(Json).map_err(bad_request)
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mom/des/simulations", post(create))
        .route("/api/mom/factory", get(factory))
        .route("/api/mom/factory/forecast", get(forecast))
        .route("/api/mom/factory/whatif", post(whatif))
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
    with_engine(&s, |e| {
        Ok(json!({"simulation_id": e.create_des_simulation(config)?, "provenance": "simulation"}))
    })
}

async fn factory(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    with_engine(&s, |e| e.des_factory_view(&q.simulation))
}

async fn forecast(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    with_engine(&s, |e| e.des_forecast(&q.simulation))
}

async fn whatif(State(s): State<AppState>, Json(body): Json<WhatIfBody>) -> ApiResult {
    let request = WhatIfRequest {
        asset: body.asset,
        quantity: body.quantity,
        due_minute: body.due_minute,
    };
    with_engine(&s, |e| e.des_whatif(&body.simulation_id, &request))
}
