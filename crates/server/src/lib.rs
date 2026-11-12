//! HTTP API 定义（main.rs 和集成测试共用）。
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET  | /api/health | LLM 连通性、模型列表、Agent 状态 |
//! | GET  | /api/agents | Agent 能力卡片 |
//! | GET/POST | /api/conversations | 会话列表 / 新建会话 |
//! | GET/POST | /api/conversations/{id}/messages | 消息列表 / 发送消息 |
//! | GET  | /api/tasks?conversation_id= | 任务列表 |
//! | GET  | /api/tasks/{id}?after= | 任务详情；`after` 为事件 id 时只返回它之后的新事件 |
//! | POST | /api/tasks/{id}/cancel、/approve | 取消 / 人工批准 |
//! | POST | /api/feedback | 记下一次派发纠错 |
//! | GET  | /api/route-logs?limit= | 路由日志 |
//! | GET  | /api/events?conversation_id= | SSE 事件流 |
//! | GET  | /api/nodes | 局域网发现的节点 + 已配对节点 |
//! | POST | /api/nodes/pair | 配对节点 `{"address", "code"}` |
//! | DELETE | /api/nodes/{id} | 解除配对 |
//! | GET  | /api/diagnostics | 诊断：最近的问题、按错误码统计、agent 与节点状态 |
//! | GET/POST | /api/admission/... | agent 准入（E3，开启 `[admission]` 时才有，见 `admission_api`） |

pub mod admission_api;
pub mod host_guard;
pub mod mom;
pub mod mom_relos;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use an_core::{Conversation, TaskEvent, TaskRecord};
use an_runtime::Runtime;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

type AppState = Arc<Runtime>;

/// 统一错误响应：`{"error": "..."}`
pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

fn not_found(what: &str) -> AppError {
    AppError(StatusCode::NOT_FOUND, format!("{what}不存在"))
}

type ApiResult<T> = Result<Json<T>, AppError>;

/// 构建 API 路由
pub fn api(rt: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/agents", get(agents))
        .route("/api/capabilities", get(capabilities))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/conversations/{id}/messages",
            get(list_messages).post(send_message),
        )
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks/{id}", get(get_task))
        .route("/api/tasks/{id}/cancel", post(cancel_task))
        .route("/api/tasks/{id}/approve", post(approve_task))
        .route("/api/feedback", post(post_feedback))
        .route("/api/route-logs", get(route_logs))
        .route("/api/events", get(events))
        .route("/api/nodes", get(list_nodes))
        .route("/api/nodes/pair", post(pair_node))
        .route("/api/nodes/{id}", axum::routing::delete(unpair_node))
        .route("/api/diagnostics", get(diagnostics))
        .merge(admission_api::routes())
        .with_state(rt)
}

async fn capabilities(State(rt): State<AppState>) -> Json<serde_json::Value> {
    Json(json!(rt.capabilities))
}

async fn health(State(rt): State<AppState>) -> Json<serde_json::Value> {
    let cfg = rt.llm.config();
    let backends = rt.backends.status().await;
    let conv = backends.iter().find(|b| b.roles.contains(&"conversation"));
    Json(json!({
        "llm": {
            "ok": conv.is_some_and(|b| b.ok),
            "base_url": conv.map(|b| b.base_url.as_str()).unwrap_or(cfg.base_url.as_str()),
            "model": conv.map(|b| b.model.as_str()).unwrap_or(cfg.model.as_str()),
            "server_models": conv.map(|b| b.server_models.clone()).unwrap_or_default(),
            "error": conv.and_then(|b| b.error.clone()),
            "structured_mode": cfg.structured_mode,
        },
        "backends": backends,
        "agents": agent_links(&rt),
    }))
}

/// 每个 agent 的卡片，附加 `link`：绿（connected）/ 灰（configured）/ 红（unavailable）及原因
fn agent_links(rt: &Runtime) -> Vec<serde_json::Value> {
    let now = std::time::Instant::now();
    rt.registry
        .cards()
        .into_iter()
        .map(|card| {
            let state = rt.registry.links.state(&card.id);
            // 内置 agent 在进程内运行，始终可用；节点的在线状态由心跳决定；
            // ACP / CLI 由握手探测和成功调用记录决定
            let (status, reason) = match card.kind.as_str() {
                "builtin" => (
                    an_runtime::link::LinkStatus::Connected,
                    "内置，随服务运行".to_string(),
                ),
                "node" if card.enabled => (
                    an_runtime::link::LinkStatus::Connected,
                    "节点在线".to_string(),
                ),
                _ => {
                    let status = an_runtime::link::link_status(now, card.enabled, &state);
                    let reason = match status {
                        an_runtime::link::LinkStatus::Unavailable => card
                            .disabled_reason
                            .clone()
                            .unwrap_or_else(|| "未安装".into()),
                        an_runtime::link::LinkStatus::Connected => "已连通".into(),
                        an_runtime::link::LinkStatus::Configured => match &state.last_probe {
                            Some((_, Err(e))) => format!("未连通：{e}"),
                            _ => "尚未探测到连通".into(),
                        },
                    };
                    (status, reason)
                }
            };
            let mut v = serde_json::to_value(&card).unwrap_or_else(|_| json!({}));
            v["link"] = json!({ "status": status, "reason": reason });
            v
        })
        .collect()
}

async fn agents(State(rt): State<AppState>) -> Json<Vec<an_core::AgentCard>> {
    Json(rt.registry.cards())
}

#[derive(Deserialize)]
struct NewConversation {
    title: Option<String>,
}

async fn create_conversation(
    State(rt): State<AppState>,
    body: Option<Json<NewConversation>>,
) -> ApiResult<Conversation> {
    let title = body
        .and_then(|b| b.0.title)
        .unwrap_or_else(|| "新会话".into());
    Ok(Json(rt.store.create_conversation(&title)?))
}

async fn list_conversations(State(rt): State<AppState>) -> ApiResult<Vec<Conversation>> {
    Ok(Json(rt.store.list_conversations()?))
}

async fn list_messages(
    State(rt): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Vec<an_core::Message>> {
    rt.store
        .get_conversation(&id)?
        .ok_or_else(|| not_found("会话"))?;
    Ok(Json(rt.store.list_messages(&id, None)?))
}

#[derive(Deserialize)]
struct SendMessage {
    text: String,
}

async fn send_message(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessage>,
) -> ApiResult<an_runtime::conversation::TurnOutcome> {
    if body.text.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "消息不能为空".into()));
    }
    rt.store
        .get_conversation(&id)?
        .ok_or_else(|| not_found("会话"))?;
    // 回合放进独立任务执行：刷新页面会断开这次 HTTP 请求，如果回合直接跑在请求的 future 里，
    // 请求被丢弃时回合也会中途终止（LLM 调用被断开，助手消息不落库）。
    // 独立任务不受请求生命周期影响，回合会照常完成；请求还连着时仍同步返回结果。
    let text = body.text.trim().to_string();
    let turn_rt = rt.clone();
    let turn = tokio::spawn(async move { turn_rt.conversation.handle_message(&id, &text).await });
    let outcome = turn.await.map_err(|e| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("回合执行异常：{e}"),
        )
    })??;
    Ok(Json(outcome))
}

#[derive(Deserialize)]
struct TaskQuery {
    conversation_id: Option<String>,
}

async fn list_tasks(
    State(rt): State<AppState>,
    Query(q): Query<TaskQuery>,
) -> ApiResult<Vec<TaskRecord>> {
    Ok(Json(rt.store.list_tasks(q.conversation_id.as_deref())?))
}

#[derive(Serialize)]
struct TaskDetail {
    task: TaskRecord,
    events: Vec<TaskEvent>,
}

#[derive(Deserialize)]
struct AfterQuery {
    /// 只返回这条事件之后的新事件（任务详情增量刷新）
    after: Option<String>,
}

async fn get_task(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<AfterQuery>,
) -> ApiResult<TaskDetail> {
    let task = rt.store.get_task(&id)?.ok_or_else(|| not_found("任务"))?;
    // 状态行已经落库，对应的事件可能还在写队列里。先等队列写完，详情里的时间线才完整。
    rt.store.flush_events();
    Ok(Json(TaskDetail {
        events: rt.store.list_events_after(&id, q.after.as_deref())?,
        task,
    }))
}

async fn cancel_task(
    State(rt): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    rt.store.get_task(&id)?.ok_or_else(|| not_found("任务"))?;
    let cancelled = rt.orchestrator.cancel(&id)?;
    Ok(Json(json!({ "cancelled": cancelled })))
}

#[derive(Deserialize)]
struct FeedbackBody {
    message_id: String,
    original_action: String,
    #[serde(default)]
    original_role: Option<String>,
    correct_action: String,
    #[serde(default)]
    correct_role: Option<String>,
    #[serde(default)]
    note: String,
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })
}

/// 记下一次派发纠错。成功和失败都用 `{status, data, message}`。
async fn post_feedback(State(rt): State<AppState>, Json(body): Json<FeedbackBody>) -> Response {
    let saved = an_runtime::store::RoutingFeedback {
        id: an_core::new_id("fb"),
        message_id: body.message_id,
        original_action: body.original_action,
        original_role: blank_to_none(body.original_role),
        correct_action: body.correct_action,
        correct_role: blank_to_none(body.correct_role),
        note: body.note,
        created_at: an_core::now(),
    };
    match rt.store.insert_feedback(&saved) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "status": "success", "data": { "id": saved.id }, "message": "" })),
        )
            .into_response(),
        Err(e) => {
            let message = e.to_string();
            let code = if message.contains("不能为空")
                || message.contains("动作只能")
                || message.contains("未知角色")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                code,
                Json(json!({ "status": "error", "data": null, "message": message })),
            )
                .into_response()
        }
    }
}

async fn approve_task(
    State(rt): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    rt.store.get_task(&id)?.ok_or_else(|| not_found("任务"))?;
    rt.orchestrator
        .approve(&id)
        .map_err(|e| AppError(StatusCode::CONFLICT, format!("{e:#}")))?;
    Ok(Json(json!({ "approved": true })))
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

async fn route_logs(
    State(rt): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Vec<an_core::RouteLog>> {
    Ok(Json(
        rt.store.list_route_logs(q.limit.unwrap_or(50).min(500))?,
    ))
}

#[derive(Deserialize)]
struct EventQuery {
    conversation_id: Option<String>,
}

/// SSE：推送任务事件；可按会话过滤
async fn events(
    State(rt): State<AppState>,
    Query(q): Query<EventQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(rt.bus.subscribe()).filter_map(move |item| {
        // 订阅者落后太多时 broadcast 会返回 Lagged 错误，这里直接跳过
        let ev = item.ok()?;
        if q.conversation_id
            .as_ref()
            .is_some_and(|c| *c != ev.conversation_id)
        {
            return None;
        }
        Some(Ok(Event::default()
            .event("task_event")
            .json_data(&ev)
            .unwrap_or_default()))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

// ───────────────────────── 节点 ─────────────────────────

async fn list_nodes(State(rt): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "discovered": rt.nodes.discovered(), "paired": rt.nodes.paired() }))
}

#[derive(Deserialize)]
struct PairBody {
    address: String,
    code: String,
}

async fn pair_node(
    State(rt): State<AppState>,
    Json(body): Json<PairBody>,
) -> ApiResult<an_runtime::nodes::PairedNode> {
    rt.nodes
        .pair(&body.address, &body.code)
        .await
        .map(Json)
        .map_err(|e| {
            let status = if e.code == an_core::codes::NODE_UNREACHABLE {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::BAD_REQUEST
            };
            AppError(status, e.to_string())
        })
}

async fn unpair_node(
    State(rt): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let removed = rt.nodes.unpair(&id)?;
    if !removed {
        return Err(not_found("节点"));
    }
    Ok(Json(json!({ "unpaired": true })))
}

// ───────────────────────── 诊断 ─────────────────────────

/// 诊断接口：给人看，也给自动排查脚本看。
/// 包含最近的 WARN / ERROR（带错误码和关联 id）、按错误码统计、agent 与执行组、节点状态、LLM 连通性。
async fn diagnostics(State(rt): State<AppState>) -> Json<serde_json::Value> {
    let groups: serde_json::Map<String, serde_json::Value> = an_core::Role::ALL
        .iter()
        .map(|r| {
            let v = match rt.registry.group_ids(*r) {
                Ok(ids) => json!({ "members": ids, "enabled": rt.registry.is_enabled(*r) }),
                Err(e) => json!({ "error": e.to_string() }),
            };
            (r.as_str().to_string(), v)
        })
        .collect();
    let backends = rt.backends.status().await;
    let llm = match backends.iter().find(|b| b.roles.contains(&"conversation")) {
        Some(b) if b.ok => json!({ "ok": true, "models": b.server_models }),
        Some(b) => {
            json!({ "ok": false, "code": an_core::codes::LLM_UNREACHABLE, "error": b.error })
        }
        None => {
            json!({ "ok": false, "code": an_core::codes::LLM_UNREACHABLE, "error": "没有 conversation 后端" })
        }
    };
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "time": an_core::now(),
        "llm": llm,
        "backends": backends,
        "groups": groups,
        "agents": rt.registry.cards(),
        "nodes": { "paired": rt.nodes.paired(), "discovered": rt.nodes.discovered() },
        "problem_counts": rt.diagnostics.counts_by_code(),
        "recent_problems": rt.diagnostics.recent(100),
    }))
}
