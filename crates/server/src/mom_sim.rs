//! MOM 车间 DES 仿真的只读接口（设计文档 `docs/mom/DES_CLOSED_LOOP.md` §9，阶段②）。
//!
//! 供会话 agent 与制造桌面查询：工厂可观测状态、趋势预测、what-if 方案比较，以及创建 DES 仿真。
//! 返回值均标注 `provenance: simulation`，不含标准答案与隐状态；查询接口不改变仿真世界。
//! 与 `mom.rs` 共用同一个引擎和状态，路由在 `mom::router` 中合并。
//!
//! 阶段⑤a-3（DES-28）：接了模拟工厂（`an-mom --des-mcp`）时，创建经连接器进行；远程仿真的工厂视图与趋势预测
//! 转调模拟工厂的 `equipment_status`、`equipment_trend`（与进程内同一套函数，格式不变）；
//! `/api/mom/des/tools` 只开放观测工具。模拟工厂不在线时返回 503（`E_DES_UNAVAILABLE`），不回退到进程内仿真。

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

/// 引擎业务错误：过期的 AI 审核版本返回 409（`E_REVIEW_STALE`），其余 400。
fn engine_err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    let message = e.to_string();
    if message.contains(an_core::codes::REVIEW_STALE) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": message, "code": an_core::codes::REVIEW_STALE})),
        );
    }
    bad_request(message)
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
        f(&mut engine).map(Json).map_err(engine_err)
    })
    .await
    .map_err(|_| bad_request("runtime unavailable"))?
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mom/des/simulations", post(create))
        .route("/api/mom/des/tools", get(des_tools))
        .route("/api/mom/des/tools/{tool}", post(des_tool_call))
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
        // 只做规则检查：报告照常返回，AI 审核标为不可用（不写入审核记录）。
        return with_engine(s, move |e| {
            let mut report = e.des_review_report(&case_id)?;
            report["ai"] =
                json!({"status": "unavailable", "reason": "no reviewer configured; rules only"});
            Ok(report)
        })
        .await;
    };
    // 两段需要引擎锁的计算放进阻塞线程（`with_engine`），调用审核模型在锁外 await（R-20261118-L1）。
    let input = {
        let case_id = case_id.clone();
        with_engine(s.clone(), move |e| e.des_ai_review_input(&case_id))
            .await?
            .0
    };
    let result =
        an_runtime::mom::des::review::call_reviewer(&input, reviewer.as_ref(), AI_REVIEW_TIMEOUT)
            .await;
    with_engine(s, move |e| e.des_record_ai_review(&case_id, result)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanReviewBody {
    decision: String,
    #[serde(default)]
    reason: String,
    actor: String,
    /// 人看到的 AI 审核版本（必填，R-20261119-M14）。
    ai_version: Option<u64>,
}

/// 人工确认或推翻 AI 审核结论（推翻必须写理由）。
async fn human_review(
    State(s): State<AppState>,
    Path(case_id): Path<String>,
    Json(body): Json<HumanReviewBody>,
) -> ApiResult {
    let Some(ai_version) = body.ai_version else {
        return Err(bad_request("ai_version is required"));
    };
    with_engine(s, move |e| {
        e.des_human_review_for(
            &case_id,
            ai_version,
            &body.decision,
            &body.reason,
            &body.actor,
        )?;
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
    if let Some(des) = s.des.clone() {
        let id = des
            .create_simulation(&s.engine, config)
            .await
            .map_err(crate::mom::des_error)?;
        return Ok(Json(
            json!({"simulation_id": id, "provenance": "simulation", "source": "mcp"}),
        ));
    }
    with_engine(s, move |e| {
        Ok(json!({"simulation_id": e.create_des_simulation(config)?, "provenance": "simulation", "source": "in_process"}))
    })
    .await
}

/// 远程仿真改由模拟工厂的观测工具回答；返回 `None` 表示按进程内仿真处理。
async fn remote_view(s: &AppState, tool: &str, simulation: &str) -> Option<ApiResult> {
    let des = s.des.clone()?;
    let sim = simulation.to_string();
    let remote = with_engine(s.clone(), move |e| Ok(json!(e.des_is_remote(&sim))))
        .await
        .map(|v| v.0 == json!(true))
        .unwrap_or(false);
    if !remote {
        return None;
    }
    Some(
        des.call_agent_tool(tool, json!({"simulation_id": simulation}))
            .await
            .map(Json)
            .map_err(crate::mom::des_error),
    )
}

async fn factory(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    if let Some(r) = remote_view(&s, "equipment_status", &q.simulation).await {
        return r;
    }
    with_engine(s, move |e| e.des_factory_view(&q.simulation)).await
}

async fn forecast(State(s): State<AppState>, Query(q): Query<SimulationQuery>) -> ApiResult {
    if let Some(r) = remote_view(&s, "equipment_trend", &q.simulation).await {
        return r;
    }
    with_engine(s, move |e| e.des_forecast(&q.simulation)).await
}

/// 未接模拟工厂时工具接口的错误。
fn no_factory() -> (StatusCode, Json<Value>) {
    bad_request("no simulated factory connected; start an-mom with --des-mcp")
}

/// 桌面与会话可调用的模拟工厂工具（只有观测类）。
async fn des_tools(State(s): State<AppState>) -> ApiResult {
    let des = s.des.clone().ok_or_else(no_factory)?;
    Ok(Json(
        json!({"tools": des.agent_tools(), "provenance": "simulation"}),
    ))
}

/// 以 agent 身份调用一个观测工具；其他工具返回 403，不发给模拟工厂。
async fn des_tool_call(
    State(s): State<AppState>,
    Path(tool): Path<String>,
    Json(args): Json<Value>,
) -> ApiResult {
    let des = s.des.clone().ok_or_else(no_factory)?;
    if !des.agent_tools().contains(&tool) {
        return Err((
            StatusCode::FORBIDDEN,
            // 不回显工具名：被拒绝的管理工具名本身含 ground_truth 等敏感字样。
            Json(json!({"error": "this tool is not allowed; only observation tools are exposed"})),
        ));
    }
    des.call_agent_tool(&tool, args)
        .await
        .map(Json)
        .map_err(crate::mom::des_error)
}

async fn whatif(State(s): State<AppState>, Json(body): Json<WhatIfBody>) -> ApiResult {
    let request = WhatIfRequest {
        asset: body.asset,
        quantity: body.quantity,
        due_minute: body.due_minute,
    };
    with_engine(s, move |e| e.des_whatif(&body.simulation_id, &request)).await
}
