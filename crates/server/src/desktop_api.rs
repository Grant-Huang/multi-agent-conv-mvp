//! AI Desktop 扩展协议（B1 可靠受理；B2 游标 SSE；B3 关联与配置）。路径挂在 `/api/desktop/v1`。

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use an_core::{
    now, EventPublisher, PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskRecord,
    TaskStatus,
};
use an_runtime::store::{
    DesktopAcceptOutcome, DesktopApprovalDecideOutcome, DesktopApprovalExecuteOutcome,
    DesktopDraftRow, DesktopDraftSave, DesktopDraftSaveOutcome, DesktopEventCursor,
    DesktopEventsAfter, DesktopEvidenceAccess, DesktopLinkOutcome, DesktopMemory,
    DesktopPrefPatchOutcome, DesktopRequestRecord, DesktopTaskLinkAuditRow,
};
use an_runtime::Runtime;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::stream::{self, StreamExt as FuturesStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as TokioStreamExt;

type AppState = Arc<Runtime>;

#[derive(Debug, Deserialize)]
pub struct CreateRequestBody {
    pub client_operation_id: String,
    pub text: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    pub task_id: Option<String>,
}

fn default_scope() -> String {
    "continue".into()
}

#[derive(Debug, Serialize)]
pub struct AcceptResponse {
    pub request_id: String,
    pub message_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub dispatch_count: u32,
    pub replayed: bool,
}

#[derive(Debug, Deserialize)]
pub struct DesktopEventQuery {
    pub conversation_id: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Serialize)]
struct DesktopEventEnvelope {
    event_id: String,
    cursor: String,
    conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    kind: an_core::EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<an_core::TaskStatus>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_id: Option<String>,
    created_at: String,
}

pub fn routes(test_fixtures: bool) -> Router<AppState> {
    let router = Router::new()
        .route(
            "/api/desktop/v1/conversations/{id}/requests",
            post(create_request),
        )
        .route("/api/desktop/v1/requests/{id}", get(get_request))
        .route("/api/desktop/v1/events", get(desktop_events))
        .route(
            "/api/desktop/v1/tasks/{id}/links",
            get(list_links).post(create_link),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/links/{link_id}",
            delete(revoke_link),
        )
        // T-99：关联修正审计（服务端权威；用户事件不播种）
        .route(
            "/api/desktop/v1/tasks/{id}/link-audits",
            get(list_link_audits).post(create_link_audit),
        )
        .route(
            "/api/desktop/v1/preferences",
            get(get_preferences).patch(patch_preferences),
        )
        // T-86 草稿上云：工作区草稿服务端权威（乐观版本防丢失更新）
        .route(
            "/api/desktop/v1/drafts/{kind}/{scope_id}",
            get(get_draft).put(save_draft).delete(clear_draft),
        )
        // T-78：记忆列表与纠错/撤回（SQLite 权威；fixture/reset 仅测试种子复位）
        .route(
            "/api/desktop/v1/memories",
            get(list_memories).post(create_memory),
        )
        .route(
            "/api/desktop/v1/memories/{id}/confirm",
            post(confirm_memory),
        )
        .route(
            "/api/desktop/v1/memories/fixture/reset",
            post(reset_memories_fixture),
        )
        .route(
            "/api/desktop/v1/memories/{id}",
            get(get_memory).patch(patch_memory).delete(delete_memory),
        )
        // T-73：助理消息已读跨端（进程内夹具，无新表）
        .route(
            "/api/desktop/v1/assistant/messages",
            get(list_assistant_messages),
        )
        .route(
            "/api/desktop/v1/assistant/messages/read",
            get(get_assistant_reads).post(mark_assistant_reads),
        )
        .route(
            "/api/desktop/v1/assistant/messages/fixture/reset",
            post(reset_assistant_read_fixture),
        )
        .route("/api/desktop/v1/tasks/{id}/evidence", get(list_evidence))
        .route("/api/desktop/v1/evidence/search", get(search_evidence))
        .route("/api/desktop/v1/evidence/{id}", get(get_evidence))
        .route(
            "/api/desktop/v1/evidence/{id}/revoke",
            post(revoke_evidence),
        )
        .route("/api/desktop/v1/tasks/{id}/approvals", get(get_approval))
        .route(
            "/api/desktop/v1/tasks/{id}/approvals/reset",
            post(reset_approval),
        )
        .route(
            "/api/desktop/v1/approvals/{id}/decide",
            post(decide_approval),
        )
        .route("/api/desktop/v1/approvals/{id}/bump", post(bump_approval))
        .route(
            "/api/desktop/v1/approvals/{id}/execute",
            post(execute_approval),
        )
        .route(
            "/api/desktop/v1/approvals/{id}/ack-unknown",
            post(ack_approval_unknown),
        )
        .route(
            "/api/desktop/v1/approvals/{id}/expire",
            post(expire_approval),
        )
        .route(
            "/api/desktop/v1/attention",
            get(list_attention).post(post_attention),
        )
        .route("/api/desktop/v1/attention/mute", post(mute_attention))
        .route("/api/desktop/v1/attention/{id}", delete(dismiss_attention))
        .route("/api/desktop/v1/tasks/{id}/voice", get(get_voice))
        .route("/api/desktop/v1/tasks/{id}/voice/reset", post(reset_voice))
        .route(
            "/api/desktop/v1/tasks/{id}/voice/permission",
            post(voice_permission),
        )
        .route("/api/desktop/v1/tasks/{id}/voice/start", post(voice_start))
        .route(
            "/api/desktop/v1/tasks/{id}/voice/partial",
            post(voice_partial),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/voice/finalize",
            post(voice_finalize),
        )
        .route("/api/desktop/v1/tasks/{id}/voice/play", post(voice_play))
        .route("/api/desktop/v1/tasks/{id}/voice/barge", post(voice_barge))
        .route(
            "/api/desktop/v1/tasks/{id}/voice/offline",
            post(voice_offline),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/voice/as-text",
            post(voice_as_text),
        )
        .route("/api/desktop/v1/tasks/{id}/voice/share", post(voice_share))
        .route("/api/desktop/v1/tasks/{id}/handoff", get(get_handoff))
        .route(
            "/api/desktop/v1/tasks/{id}/handoff/reset",
            post(reset_handoff),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/handoff/draft",
            post(handoff_draft),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/handoff/resolve",
            post(handoff_resolve),
        )
        .route(
            "/api/desktop/v1/tasks/{id}/handoff/share-voice",
            post(handoff_share_voice),
        )
        // T-64 harness fixture（resume/fixture/*）改为按 test_fixtures 开关条件挂载，见函数尾部（T-95/FIX-C/H4）
        // T-82 首页快照：三区卡片读模型（只读；fixture 种子仅测试/演示模式，标注「模拟」）
        .route("/api/desktop/v1/snapshot", get(get_snapshot))
        .route(
            "/api/desktop/v1/snapshot/fixture/reset",
            post(snapshot_fixture_reset),
        );
    // T-95/FIX-C（H4）：Resume 测试夹具接口仅在 test_fixtures=true 时挂载；生产缺省不挂载（404）
    if test_fixtures {
        router
            .route(
                "/api/desktop/v1/resume/fixture/seed",
                post(resume_fixture_seed),
            )
            .route(
                "/api/desktop/v1/resume/fixture/replay-a",
                post(resume_fixture_replay_a),
            )
            .route(
                "/api/desktop/v1/resume/fixture/continue-b",
                post(resume_fixture_continue_b),
            )
    } else {
        router
    }
}

pub(crate) fn principal_from(headers: &HeaderMap) -> String {
    headers
        .get("x-desktop-principal")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("alice")
        .to_string()
}

/// conversation-memory 5.2a：记忆与回答注入的作用域（沿用 T-78 的 `local` 契约；
/// 夹具头 `x-desktop-principal` 提供多用户测试口径，无头回退 `LOCAL_USER`）。
pub(crate) fn principal_or_local(headers: &HeaderMap) -> String {
    headers
        .get("x-desktop-principal")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(an_runtime::session_materials::LOCAL_USER)
        .to_string()
}

async fn create_request(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(body): Json<CreateRequestBody>,
) -> Response {
    if rt
        .store
        .get_conversation(&conversation_id)
        .ok()
        .flatten()
        .is_none()
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "会话不存在",
                "retryable": false
            })),
        )
            .into_response();
    }
    let outcome = match rt.store.accept_desktop_request(
        &conversation_id,
        &body.client_operation_id,
        &body.text,
        &body.scope,
        body.task_id.as_deref(),
    ) {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("{e:#}");
            let status = if msg.contains("不能为空") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (
                status,
                Json(json!({
                    "code": "bad_request",
                    "message": msg,
                    "retryable": false
                })),
            )
                .into_response();
        }
    };

    match outcome {
        DesktopAcceptOutcome::Created(rec) => {
            // 后台继续分流；失败不影响已返回的 202 受理
            let text = rec.text.clone();
            let conv = rec.conversation_id.clone();
            let user = principal_or_local(&headers);
            let turn_rt = rt.clone();
            tokio::spawn(async move {
                if let Err(e) = turn_rt
                    .conversation
                    .continue_after_accepted_as(&user, &conv, &text)
                    .await
                {
                    tracing::warn!(
                        event = "desktop.accept.continue_failed",
                        error = %e,
                        conv_id = %conv,
                        "可靠受理后分流失败"
                    );
                }
            });
            (StatusCode::ACCEPTED, Json(to_response(&rec, false))).into_response()
        }
        DesktopAcceptOutcome::Replay(rec) => {
            (StatusCode::ACCEPTED, Json(to_response(&rec, true))).into_response()
        }
        DesktopAcceptOutcome::Conflict {
            request_id,
            message_id,
        } => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "operation_conflict",
                "message": "相同 client_operation_id 已绑定不同请求载荷",
                "retryable": false,
                "request_id": request_id,
                "message_id": message_id
            })),
        )
            .into_response(),
    }
}

async fn get_request(State(rt): State<AppState>, Path(request_id): Path<String>) -> Response {
    match rt.store.get_desktop_request(&request_id) {
        Ok(Some(rec)) => (StatusCode::OK, Json(rec)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "受理记录不存在",
                "retryable": false,
                "request_id": request_id
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "internal",
                "message": format!("{e:#}"),
                "retryable": true
            })),
        )
            .into_response(),
    }
}

/// B2：先按游标回放落库事件，再续接 live；`after` 失效返回 410。
async fn desktop_events(
    State(rt): State<AppState>,
    Query(q): Query<DesktopEventQuery>,
) -> Response {
    let Some(conversation_id) = q
        .conversation_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "conversation_id 必填",
                "retryable": false
            })),
        )
            .into_response();
    };

    if rt
        .store
        .get_conversation(&conversation_id)
        .ok()
        .flatten()
        .is_none()
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "会话不存在",
                "retryable": false
            })),
        )
            .into_response();
    }

    // 先订阅再冲刷+回放，缩小缺口；重复 event_id 在流内去重
    let rx = rt.bus.subscribe();
    rt.store.flush_events();

    let replay = match rt
        .store
        .list_conversation_events_after(&conversation_id, q.after.as_deref())
    {
        Ok(DesktopEventsAfter::CursorExpired) => {
            return (
                StatusCode::GONE,
                Json(json!({
                    "code": "cursor_expired",
                    "message": "游标无效或已淘汰，请重拉任务快照后再订阅",
                    "retryable": false,
                    "conversation_id": conversation_id,
                    "after": q.after
                })),
            )
                .into_response();
        }
        Ok(DesktopEventsAfter::Ok(evs)) => evs,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "internal",
                    "message": format!("{e:#}"),
                    "retryable": true
                })),
            )
                .into_response();
        }
    };

    let mut seen: HashSet<String> = replay.iter().map(|e| e.cursor.clone()).collect();
    let conv = conversation_id.clone();

    let replay_stream = stream::iter(replay.into_iter().map(|item| {
        Ok::<_, Infallible>(
            Event::default()
                .event("desktop_event")
                .json_data(envelope_from_cursor(&item))
                .unwrap_or_default(),
        )
    }));

    let live = TokioStreamExt::filter_map(BroadcastStream::new(rx), move |item| {
        let ev = item.ok()?;
        if ev.conversation_id != conv {
            return None;
        }
        if !seen.insert(ev.id.clone()) {
            return None;
        }
        Some(Ok::<_, Infallible>(
            Event::default()
                .event("desktop_event")
                .json_data(envelope_from_task_event(&ev))
                .unwrap_or_default(),
        ))
    });

    let stream = FuturesStreamExt::chain(replay_stream, live);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct CreateLinkBody {
    pub client_operation_id: String,
    pub conversation_id: String,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

fn default_created_by() -> String {
    "local".into()
}

#[derive(Debug, Deserialize)]
struct LinksQuery {
    include_revoked: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PatchPreferencesBody {
    pub expected_version: u32,
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub role: Option<String>,
    pub soul: Option<String>,
    pub taste: Option<String>,
    pub memory_principle: Option<String>,
    pub proactivity: Option<String>,
}

async fn create_link(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<CreateLinkBody>,
) -> Response {
    match rt.store.create_desktop_task_link(
        &task_id,
        &body.conversation_id,
        &body.client_operation_id,
        &body.source_message_ids,
        &body.created_by,
    ) {
        Ok(DesktopLinkOutcome::Created(link)) => (
            StatusCode::CREATED,
            Json(json!({ "link": link, "replayed": false })),
        )
            .into_response(),
        Ok(DesktopLinkOutcome::Replay(link)) => (
            StatusCode::CREATED,
            Json(json!({ "link": link, "replayed": true })),
        )
            .into_response(),
        Ok(DesktopLinkOutcome::Conflict {
            link_id,
            conversation_id,
        }) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "operation_conflict",
                "message": "相同 client_operation_id 已绑定不同关联",
                "retryable": false,
                "link_id": link_id,
                "conversation_id": conversation_id
            })),
        )
            .into_response(),
        Err(e) => {
            let msg = format!("{e:#}");
            let status = if msg.contains("不存在") {
                StatusCode::NOT_FOUND
            } else if msg.contains("不能为空") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(json!({
                    "code": if status == StatusCode::NOT_FOUND { "not_found" } else { "bad_request" },
                    "message": msg,
                    "retryable": false
                })),
            )
                .into_response()
        }
    }
}

async fn list_links(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Query(q): Query<LinksQuery>,
) -> Response {
    if rt.store.get_task(&task_id).ok().flatten().is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "任务不存在",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt
        .store
        .list_desktop_task_links(&task_id, q.include_revoked.unwrap_or(false))
    {
        Ok(links) => (StatusCode::OK, Json(json!({ "links": links }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "internal",
                "message": format!("{e:#}"),
                "retryable": true
            })),
        )
            .into_response(),
    }
}

async fn revoke_link(
    State(rt): State<AppState>,
    Path((task_id, link_id)): Path<(String, String)>,
) -> Response {
    if link_id.starts_with("legacy-") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "投影关联不可撤回，请先建立显式关联",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt.store.revoke_desktop_task_link(&task_id, &link_id) {
        Ok(Some(link)) => (StatusCode::OK, Json(json!({ "link": link }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "关联不存在",
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "internal",
                "message": format!("{e:#}"),
                "retryable": true
            })),
        )
            .into_response(),
    }
}

async fn get_preferences(State(rt): State<AppState>) -> Response {
    match rt.store.get_desktop_preferences("local") {
        Ok(p) => (StatusCode::OK, Json(p)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "internal",
                "message": format!("{e:#}"),
                "retryable": true
            })),
        )
            .into_response(),
    }
}

async fn patch_preferences(
    State(rt): State<AppState>,
    Json(body): Json<PatchPreferencesBody>,
) -> Response {
    let current = match rt.store.get_desktop_preferences("local") {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "internal",
                    "message": format!("{e:#}"),
                    "retryable": true
                })),
            )
                .into_response();
        }
    };
    let mut next = current.clone();
    if let Some(v) = body.name {
        next.name = v;
    }
    if let Some(v) = body.avatar {
        next.avatar = v;
    }
    if let Some(v) = body.role {
        next.role = v;
    }
    if let Some(v) = body.soul {
        next.soul = v;
    }
    if let Some(v) = body.taste {
        next.taste = v;
    }
    if let Some(v) = body.memory_principle {
        next.memory_principle = v;
    }
    if let Some(v) = body.proactivity {
        next.proactivity = v;
    }
    match rt
        .store
        .patch_desktop_preferences("local", body.expected_version, &next)
    {
        Ok(DesktopPrefPatchOutcome::Ok(p)) => (StatusCode::OK, Json(p)).into_response(),
        Ok(DesktopPrefPatchOutcome::Conflict(current)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "version_conflict",
                "message": "配置版本冲突，请基于当前值重新编辑",
                "retryable": false,
                "current": current
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "code": "internal",
                "message": format!("{e:#}"),
                "retryable": true
            })),
        )
            .into_response(),
    }
}

/// 受控单用户默认作用域已移至 `an_runtime::session_materials::LOCAL_USER`（5.2a 起按 principal 作用域）

#[derive(Debug, Deserialize)]
struct PatchMemoryBody {
    title: Option<String>,
    body: Option<String>,
    expires: Option<String>,
}

/// conversation-memory 5.2a：用户主动保存个人记忆（REQ-conversation-memory-02）。
#[derive(Debug, Deserialize)]
struct CreateMemoryBody {
    title: String,
    body: String,
    /// ISO 日期或自由文本（如「长期」）；空值按「长期」处理
    #[serde(default)]
    expires: Option<String>,
    /// fact（默认，可注入）/ preference / style（5.2b 生效闸门用）
    #[serde(default)]
    kind: Option<String>,
}

async fn create_memory(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateMemoryBody>,
) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.create_desktop_memory(
        &user,
        &body.title,
        &body.body,
        body.expires.as_deref().unwrap_or("长期"),
        body.kind.as_deref().unwrap_or("fact"),
    ) {
        Ok(m) => (StatusCode::CREATED, Json(memory_json(&m))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": "invalid", "message": format!("{e:#}"), "retryable": false })),
        )
            .into_response(),
    }
}

fn memory_json(m: &DesktopMemory) -> serde_json::Value {
    json!({
        "id": m.id,
        "title": m.title,
        "source": m.source,
        "body": m.body,
        "expires": m.expires,
        "kind": m.kind,
        "confirmed": m.confirmed_at.is_some(),
    })
}

/// conversation-memory 5.2b：确认（预览闸门）。幂等；未知 id 404。
async fn confirm_memory(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.confirm_desktop_memory(&user, &id) {
        Ok(Some(m)) => (StatusCode::OK, Json(memory_json(&m))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "code": "not_found", "message": "记忆不存在或已撤回", "retryable": false }),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

fn memory_store_err(e: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
    )
        .into_response()
}

async fn list_memories(State(rt): State<AppState>, headers: HeaderMap) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.list_desktop_memories(&user) {
        Ok(items) => {
            let items: Vec<_> = items.iter().map(memory_json).collect();
            (StatusCode::OK, Json(json!({ "items": items }))).into_response()
        }
        Err(e) => memory_store_err(e),
    }
}

async fn get_memory(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.get_desktop_memory(&user, &id) {
        Ok(Some(item)) if !item.revoked => {
            (StatusCode::OK, Json(memory_json(&item))).into_response()
        }
        Ok(Some(_)) => (
            StatusCode::GONE,
            Json(json!({
                "code": "revoked",
                "message": "记忆已撤回",
                "retryable": false
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "记忆条目不存在",
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => memory_store_err(e),
    }
}

async fn patch_memory(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PatchMemoryBody>,
) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.patch_desktop_memory(
        &user,
        &id,
        body.title.as_deref(),
        body.body.as_deref(),
        body.expires.as_deref(),
    ) {
        Ok(Some(item)) if item.revoked => (
            StatusCode::GONE,
            Json(json!({
                "code": "revoked",
                "message": "记忆已撤回，无法纠错",
                "retryable": false
            })),
        )
            .into_response(),
        Ok(Some(item)) => (StatusCode::OK, Json(memory_json(&item))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "记忆条目不存在",
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => memory_store_err(e),
    }
}

async fn delete_memory(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.revoke_desktop_memory(&user, &id) {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "id": id, "revoked": true })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "记忆条目不存在",
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => memory_store_err(e),
    }
}

async fn reset_memories_fixture(State(rt): State<AppState>, headers: HeaderMap) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.reset_desktop_memories(&user) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "reset": true }))).into_response(),
        Err(e) => memory_store_err(e),
    }
}

// ===== T-82 首页快照（AD-G7 / GET /api/desktop/v1/snapshot） =====

#[derive(Debug, Clone, Serialize)]
struct DesktopSnapshotCard {
    id: String,
    zone: &'static str,
    card_type: &'static str,
    entity_id: String,
    title: String,
    subtitle: String,
    badge: &'static str,
    priority: &'static str,
    simulated: bool,
    updated_at: String,
    source_refs: Vec<String>,
}

fn snapshot_status_subtitle(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "已创建",
        TaskStatus::Queued => "排队中 · Agent 工作",
        TaskStatus::Running => "Agent 执行中",
        TaskStatus::Verifying => "待核验证据 · 查看工作区",
        TaskStatus::NeedsHuman => "待人工处理",
        TaskStatus::Completed => "已完成",
        TaskStatus::Failed => "失败 · 可重试",
        TaskStatus::Cancelled => "已取消",
    }
}

fn snapshot_task_card(t: &TaskRecord, simulated: bool) -> DesktopSnapshotCard {
    // 分区口径（对齐 A 阶段 mockData）：排队/执行中归 Agent 工作；核验中等需要人的归我的工作
    let agent_zone = matches!(t.status, TaskStatus::Queued | TaskStatus::Running);
    let (zone, badge): (&'static str, &'static str) = if agent_zone {
        ("agent", "Agent 工作")
    } else {
        ("mine", "我的工作")
    };
    let priority = if matches!(t.status, TaskStatus::NeedsHuman | TaskStatus::Failed) {
        "high"
    } else {
        "normal"
    };
    // fixture 种子沿用 A 阶段 mockData 的稳定卡 id（E2E testid 契约：card-smt-03 等）
    let id = match (simulated, t.id.as_str()) {
        (true, "TASK-2026-018") => "card-smt-03".to_string(),
        (true, "TASK-2026-021") => "card-product".to_string(),
        _ => format!("card-task-{}", t.id),
    };
    DesktopSnapshotCard {
        id,
        zone,
        card_type: "task",
        entity_id: t.id.clone(),
        title: t.goal.clone(),
        subtitle: snapshot_status_subtitle(&t.status).to_string(),
        badge,
        priority,
        simulated,
        updated_at: t.updated_at.clone(),
        source_refs: vec![format!("task:{}", t.id)],
    }
}

fn snapshot_internal_err(e: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
    )
        .into_response()
}

async fn get_snapshot(State(rt): State<AppState>) -> Response {
    let tasks = match rt.store.list_tasks(None) {
        Ok(t) => t,
        Err(e) => return snapshot_internal_err(e),
    };
    let attention = match rt.store.list_desktop_attention() {
        Ok(a) => a,
        Err(e) => return snapshot_internal_err(e),
    };
    let mut cards: Vec<DesktopSnapshotCard> = Vec::new();
    let mut tasks_active = 0u64;
    for t in &tasks {
        if !matches!(t.status, TaskStatus::Completed | TaskStatus::Cancelled) {
            tasks_active += 1;
        }
        // fixture 种子任务标注「模拟」；真实任务一律 simulated=false（宪章原则 8）
        let simulated = matches!(t.id.as_str(), "TASK-2026-018" | "TASK-2026-021");
        cards.push(snapshot_task_card(t, simulated));
    }
    let attention_unread = attention.len() as u64;
    for a in &attention {
        let mut refs = vec![format!("attention:{}", a.id)];
        if let Some(tid) = &a.task_id {
            refs.push(format!("task:{tid}"));
        }
        cards.push(DesktopSnapshotCard {
            id: format!("card-attn-{}", a.id),
            zone: "watch",
            card_type: "attention",
            entity_id: a.task_id.clone().unwrap_or_else(|| a.id.clone()),
            title: a.title.clone(),
            subtitle: a.body.clone(),
            badge: "为你关注",
            priority: if a.kind == "urgent" { "high" } else { "normal" },
            simulated: false,
            updated_at: a.created_at.clone(),
            source_refs: refs,
        });
    }
    cards.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let cursor = now();
    let body = json!({
        "cards": cards,
        "counts": {
            "tasks_active": tasks_active,
            "attention_unread": attention_unread,
        },
        "cursor": cursor,
        "generated_at": cursor,
    });
    (StatusCode::OK, Json(body)).into_response()
}

/// fixture 种子任务：与 A 阶段 mockData 同内容；已存在则跳过（insert_task 为裸 INSERT）。
#[allow(clippy::too_many_arguments)]
fn snapshot_fixture_task(
    conversation_id: &str,
    id: &str,
    goal: &str,
    role: Role,
    status: TaskStatus,
) -> TaskRecord {
    let ts = now();
    TaskRecord {
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        goal: goal.to_string(),
        role,
        status,
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: role,
            required_tools: vec![],
            risk_level: Risk::Low,
            reason: "desktop snapshot fixture seed".into(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec!["snapshot fixture".into()],
            candidates: vec![],
        },
        result: None,
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: ts.clone(),
        updated_at: ts,
    }
}

async fn snapshot_fixture_reset(State(rt): State<AppState>) -> Response {
    let conv = match rt.store.create_conversation("桌面首页 fixture") {
        Ok(c) => c,
        Err(e) => return snapshot_internal_err(e),
    };
    // fixture 任务时间戳固定在过去：不得抢占「最新任务」类逻辑（S1 委派芯片、历史等）
    let ts = "2026-01-01T00:00:00+00:00".to_string();
    let seeds = [
        (
            "TASK-2026-018",
            "SMT-03 质量调查",
            Role::General,
            TaskStatus::Verifying,
        ),
        (
            "TASK-2026-021",
            "产品改进跟踪",
            Role::General,
            TaskStatus::Running,
        ),
    ];
    for (id, goal, role, status) in seeds {
        let exists = matches!(rt.store.get_task(id), Ok(Some(_)));
        if exists {
            continue;
        }
        let mut task = snapshot_fixture_task(&conv.id, id, goal, role, status);
        task.created_at = ts.clone();
        task.updated_at = ts.clone();
        if let Err(e) = rt.store.insert_task(&task, "desktop snapshot fixture") {
            return snapshot_internal_err(e);
        }
    }
    // 不种 attention：关注区是真实读模型，空区照常渲染；避免污染 ADATTN 等队列断言
    (StatusCode::OK, Json(json!({ "ok": true, "reset": true }))).into_response()
}

// ===== T-86 草稿上云 =====

const DESKTOP_DRAFT_USER: &str = "local";

fn draft_json(d: &DesktopDraftRow) -> serde_json::Value {
    json!({
        "text": d.text,
        "version": d.version,
        "scope": d.scope,
        "continue_task_id": d.continue_task_id,
        "updated_at": d.updated_at,
    })
}

fn empty_draft_json() -> serde_json::Value {
    json!({
        "text": "",
        "version": 0,
        "scope": "auto",
        "continue_task_id": null,
        "updated_at": ""
    })
}

fn drafts_internal_err(e: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
    )
        .into_response()
}

type DraftKindRejection = (StatusCode, Json<serde_json::Value>);

fn validate_draft_kind(kind: &str) -> Result<(), DraftKindRejection> {
    if kind != "task" && kind != "conv" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "invalid_request",
                "message": "kind 必须为 task 或 conv",
                "retryable": false
            })),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SaveDraftBody {
    #[serde(default)]
    text: String,
    #[serde(default = "default_draft_scope")]
    scope: String,
    #[serde(default)]
    continue_task_id: Option<String>,
    version: i64,
    #[serde(default)]
    expected_version: Option<i64>,
}

fn default_draft_scope() -> String {
    "auto".into()
}

async fn get_draft(
    State(rt): State<AppState>,
    Path((kind, scope_id)): Path<(String, String)>,
) -> Response {
    if let Err(rejection) = validate_draft_kind(&kind) {
        return rejection.into_response();
    }
    match rt
        .store
        .get_desktop_draft(DESKTOP_DRAFT_USER, &kind, &scope_id)
    {
        Ok(Some(row)) => {
            (StatusCode::OK, Json(json!({ "draft": draft_json(&row) }))).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!({ "draft": empty_draft_json() }))).into_response(),
        Err(e) => drafts_internal_err(e),
    }
}

async fn save_draft(
    State(rt): State<AppState>,
    Path((kind, scope_id)): Path<(String, String)>,
    Json(body): Json<SaveDraftBody>,
) -> Response {
    if let Err(rejection) = validate_draft_kind(&kind) {
        return rejection.into_response();
    }
    match rt.store.save_desktop_draft(DesktopDraftSave {
        user_id: DESKTOP_DRAFT_USER,
        kind: &kind,
        scope_id: &scope_id,
        text: &body.text,
        scope: &body.scope,
        continue_task_id: body.continue_task_id.as_deref(),
        version: body.version,
        expected_version: body.expected_version,
    }) {
        Ok(DesktopDraftSaveOutcome::Ok(row)) => {
            (StatusCode::OK, Json(json!({ "draft": draft_json(&row) }))).into_response()
        }
        Ok(DesktopDraftSaveOutcome::Conflict(row)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "version_conflict",
                "message": "草稿已被其他设备更新，以服务端为准",
                "draft": draft_json(&row),
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => drafts_internal_err(e),
    }
}

#[derive(Debug, Deserialize)]
struct ClearDraftQuery {
    sent_version: i64,
}

async fn clear_draft(
    State(rt): State<AppState>,
    Path((kind, scope_id)): Path<(String, String)>,
    Query(q): Query<ClearDraftQuery>,
) -> Response {
    if let Err(rejection) = validate_draft_kind(&kind) {
        return rejection.into_response();
    }
    match rt.store.clear_desktop_draft_after_send(
        DESKTOP_DRAFT_USER,
        &kind,
        &scope_id,
        q.sent_version,
    ) {
        Ok(Some(row)) => {
            (StatusCode::OK, Json(json!({ "draft": draft_json(&row) }))).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!({ "draft": empty_draft_json() }))).into_response(),
        Err(e) => drafts_internal_err(e),
    }
}

#[derive(Debug, Deserialize)]
struct MarkAssistantReadBody {
    message_ids: Vec<String>,
}

// ===== T-99 关联修正审计 =====

fn link_audit_json(r: &DesktopTaskLinkAuditRow) -> serde_json::Value {
    json!({
        "id": r.id,
        "task_id": r.task_id,
        "source": r.source,
        "action": r.action,
        "created_at": r.created_at,
    })
}

#[derive(Debug, Deserialize)]
struct CreateLinkAuditBody {
    source: String,
    action: String,
}

async fn list_link_audits(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.list_desktop_task_link_audits(&task_id) {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(link_audit_json).collect();
            (StatusCode::OK, Json(json!({ "items": items }))).into_response()
        }
        Err(e) => drafts_internal_err(e),
    }
}

async fn create_link_audit(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<CreateLinkAuditBody>,
) -> Response {
    if body.action != "link" && body.action != "revoke" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "invalid_request",
                "message": "action 必须为 link 或 revoke",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt
        .store
        .append_desktop_task_link_audit(&task_id, &body.source, &body.action)
    {
        Ok(row) => (
            StatusCode::OK,
            Json(json!({ "audit": link_audit_json(&row) })),
        )
            .into_response(),
        Err(e) => drafts_internal_err(e),
    }
}
async fn list_assistant_messages(State(rt): State<AppState>, headers: HeaderMap) -> Response {
    // M4（#361）：已读状态按调用者隔离；夹具头 x-desktop-principal 提供多用户测试口径，无头回退 local
    let user = principal_or_local(&headers);
    match rt.store.list_desktop_assistant_messages(&user) {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "kind": m.kind,
                        "title": m.title,
                        "body": m.body,
                        "read": m.read,
                    })
                })
                .collect();
            let unread_count = items.iter().filter(|i| i["read"] == false).count();
            (
                StatusCode::OK,
                Json(json!({ "items": items, "unread_count": unread_count })),
            )
                .into_response()
        }
        Err(e) => assistant_internal_err(e),
    }
}

fn assistant_internal_err(e: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
    )
        .into_response()
}

async fn get_assistant_reads(State(rt): State<AppState>, headers: HeaderMap) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.list_desktop_assistant_read_ids(&user) {
        Ok(mut read_ids) => {
            read_ids.sort();
            (StatusCode::OK, Json(json!({ "read_ids": read_ids }))).into_response()
        }
        Err(e) => assistant_internal_err(e),
    }
}

async fn mark_assistant_reads(
    State(rt): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MarkAssistantReadBody>,
) -> Response {
    let user = principal_or_local(&headers);
    let ids: Vec<String> = body
        .message_ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "invalid_request",
                "message": "message_ids 不能为空",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt.store.mark_desktop_assistant_reads(&user, &ids) {
        Ok((read_ids, newly_marked)) => (
            StatusCode::OK,
            Json(json!({
                "read_ids": read_ids,
                "newly_marked": newly_marked,
            })),
        )
            .into_response(),
        Err(e) => assistant_internal_err(e),
    }
}

async fn reset_assistant_read_fixture(State(rt): State<AppState>, headers: HeaderMap) -> Response {
    let user = principal_or_local(&headers);
    match rt.store.reset_desktop_assistant_reads(&user) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "reset": true }))).into_response(),
        Err(e) => assistant_internal_err(e),
    }
}

#[derive(Debug, Deserialize)]
struct EvidenceViewQuery {
    view: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EvidenceSearchQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RevokeEvidenceBody {
    principal_id: String,
}

async fn list_evidence(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = principal_from(&headers);
    match rt.store.list_desktop_evidence_for(&task_id, &principal) {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({ "principal": principal, "evidence": items })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn get_evidence(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<EvidenceViewQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = principal_from(&headers);
    let view = q.view.as_deref().unwrap_or("summary");
    match rt.store.get_desktop_evidence_for(&id, &principal, view) {
        Ok(DesktopEvidenceAccess::Allowed(ev)) => (
            StatusCode::OK,
            Json(json!({ "principal": principal, "evidence": ev })),
        )
            .into_response(),
        Ok(DesktopEvidenceAccess::Forbidden) => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "code": "forbidden",
                "message": "无权访问该证据",
                "retryable": false
            })),
        )
            .into_response(),
        Ok(DesktopEvidenceAccess::SourceUnavailable(meta)) => (
            StatusCode::GONE,
            Json(json!({
                "code": "source_unavailable",
                "message": meta.error_message,
                "retryable": false,
                "evidence_id": meta.id,
                "status": meta.status
            })),
        )
            .into_response(),
        Ok(DesktopEvidenceAccess::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "证据不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn search_evidence(
    State(rt): State<AppState>,
    Query(q): Query<EvidenceSearchQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = principal_from(&headers);
    match rt
        .store
        .search_desktop_evidence_for(&principal, q.q.as_deref().unwrap_or(""))
    {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({ "principal": principal, "evidence": items })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn revoke_evidence(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RevokeEvidenceBody>,
) -> Response {
    if body.principal_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "principal_id 不能为空",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt
        .store
        .revoke_desktop_evidence_grant(&id, body.principal_id.trim())
    {
        Ok(true) => (
            StatusCode::OK,
            Json(json!({
                "revoked": true,
                "evidence_id": id,
                "principal_id": body.principal_id.trim()
            })),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "授权记录不存在",
                "retryable": false
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct DecideApprovalBody {
    decision: String,
    expected_revision: i64,
}

#[derive(Debug, Deserialize)]
struct BumpApprovalBody {
    params: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteApprovalBody {
    expected_revision: i64,
    #[serde(default)]
    force_unknown: bool,
}

#[derive(Debug, Deserialize)]
struct MuteAttentionBody {
    mute_ordinary: bool,
}

#[derive(Debug, Deserialize)]
struct PostAttentionBody {
    kind: String,
    title: String,
    body: String,
    #[serde(default)]
    task_id: Option<String>,
}

async fn get_approval(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.get_or_create_desktop_approval(&task_id) {
        Ok(a) => (StatusCode::OK, Json(json!({ "approval": a }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn reset_approval(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.reset_desktop_approval(&task_id) {
        Ok(a) => (StatusCode::OK, Json(json!({ "approval": a }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn decide_approval(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DecideApprovalBody>,
) -> Response {
    match rt.store.decide_desktop_approval(
        &id,
        body.decision.trim(),
        body.expected_revision,
        "supervisor",
    ) {
        Ok(DesktopApprovalDecideOutcome::Ok(a)) => {
            (StatusCode::OK, Json(json!({ "approval": a }))).into_response()
        }
        Ok(DesktopApprovalDecideOutcome::RevisionConflict(a)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "revision_conflict",
                "message": "方案版本已变更，原审批失效，请重新确认",
                "approval": a,
                "retryable": false
            })),
        )
            .into_response(),
        Ok(DesktopApprovalDecideOutcome::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "审批不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn bump_approval(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<BumpApprovalBody>,
) -> Response {
    match rt.store.bump_desktop_approval(&id, body.params.trim()) {
        Ok(Some(a)) => (StatusCode::OK, Json(json!({ "approval": a }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "审批不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn execute_approval(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ExecuteApprovalBody>,
) -> Response {
    match rt
        .store
        .execute_desktop_approval(&id, body.expected_revision, body.force_unknown)
    {
        Ok(DesktopApprovalExecuteOutcome::Ok(a)) => {
            (StatusCode::OK, Json(json!({ "approval": a }))).into_response()
        }
        Ok(DesktopApprovalExecuteOutcome::RevisionConflict(a)) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "revision_conflict",
                "message": "方案版本已变更，原审批失效，请重新确认",
                "approval": a,
                "retryable": false
            })),
        )
            .into_response(),
        Ok(DesktopApprovalExecuteOutcome::Forbidden(a)) => {
            let msg = if a.status == "rejected" || a.status == "expired" {
                "审批已拒绝或过期，不能执行"
            } else {
                "尚未批准，不能执行"
            };
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "code": "forbidden",
                    "message": msg,
                    "approval": a,
                    "retryable": false
                })),
            )
                .into_response()
        }
        Ok(DesktopApprovalExecuteOutcome::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "审批不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn ack_approval_unknown(State(rt): State<AppState>, Path(id): Path<String>) -> Response {
    match rt.store.ack_desktop_approval_unknown(&id) {
        Ok(Some(a)) => (StatusCode::OK, Json(json!({ "approval": a }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "审批不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn expire_approval(State(rt): State<AppState>, Path(id): Path<String>) -> Response {
    match rt.store.expire_desktop_approval(&id) {
        Ok(Some(a)) => (StatusCode::OK, Json(json!({ "approval": a }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "审批不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn list_attention(State(rt): State<AppState>) -> Response {
    let mute = match rt.store.get_desktop_attention_mute("local") {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
            )
                .into_response();
        }
    };
    match rt.store.list_desktop_attention() {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({ "mute_ordinary": mute, "items": items })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn post_attention(
    State(rt): State<AppState>,
    Json(body): Json<PostAttentionBody>,
) -> Response {
    let kind = body.kind.trim();
    if !matches!(kind, "ordinary" | "digest" | "urgent") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "kind 须为 ordinary|digest|urgent",
                "retryable": false
            })),
        )
            .into_response();
    }
    match rt.store.enqueue_desktop_attention(
        "local",
        kind,
        body.title.trim(),
        body.body.trim(),
        body.task_id.as_deref(),
    ) {
        Ok((delivered, item)) => (
            StatusCode::OK,
            Json(json!({ "delivered": delivered, "item": item })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn mute_attention(
    State(rt): State<AppState>,
    Json(body): Json<MuteAttentionBody>,
) -> Response {
    match rt
        .store
        .set_desktop_attention_mute("local", body.mute_ordinary)
    {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "mute_ordinary": body.mute_ordinary })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn dismiss_attention(State(rt): State<AppState>, Path(id): Path<String>) -> Response {
    match rt.store.dismiss_desktop_attention(&id) {
        Ok(()) => (StatusCode::OK, Json(json!({ "dismissed": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct VoicePermissionBody {
    permission: String,
}

#[derive(Debug, Deserialize)]
struct VoicePartialBody {
    text: String,
}

#[derive(Debug, Deserialize)]
struct VoiceFinalizeBody {
    text: String,
    #[serde(default = "default_true")]
    link_task: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct VoiceTextBody {
    text: String,
}

#[derive(Debug, Deserialize)]
struct VoiceShareBody {
    share: bool,
}

#[derive(Debug, Deserialize)]
struct HandoffDraftBody {
    device: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct HandoffResolveBody {
    chosen: String,
}

#[derive(Debug, Deserialize)]
struct HandoffShareBody {
    share: bool,
}

fn voice_ok(r: anyhow::Result<Option<an_runtime::store::DesktopVoiceSession>>) -> Response {
    match r {
        Ok(Some(s)) => (StatusCode::OK, Json(json!({ "session": s }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "语音会话不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

fn handoff_ok(r: anyhow::Result<Option<an_runtime::store::DesktopHandoff>>) -> Response {
    match r {
        Ok(Some(h)) => (StatusCode::OK, Json(json!({ "handoff": h }))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "not_found", "message": "跨端状态不存在", "retryable": false })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn get_voice(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.get_or_create_desktop_voice(&task_id) {
        Ok(s) => (StatusCode::OK, Json(json!({ "session": s }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn reset_voice(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.reset_desktop_voice(&task_id) {
        Ok(s) => (StatusCode::OK, Json(json!({ "session": s }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn voice_permission(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<VoicePermissionBody>,
) -> Response {
    voice_ok(
        rt.store
            .set_desktop_voice_permission(&task_id, body.permission.trim()),
    )
}

async fn voice_start(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    voice_ok(rt.store.start_desktop_voice(&task_id))
}

async fn voice_partial(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<VoicePartialBody>,
) -> Response {
    voice_ok(rt.store.set_desktop_voice_partial(&task_id, &body.text))
}

async fn voice_finalize(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<VoiceFinalizeBody>,
) -> Response {
    voice_ok(
        rt.store
            .finalize_desktop_voice(&task_id, &body.text, body.link_task),
    )
}

async fn voice_play(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    voice_ok(rt.store.play_desktop_voice(&task_id))
}

async fn voice_barge(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    voice_ok(rt.store.barge_desktop_voice(&task_id))
}

async fn voice_offline(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    voice_ok(rt.store.offline_desktop_voice(&task_id))
}

async fn voice_as_text(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<VoiceTextBody>,
) -> Response {
    voice_ok(rt.store.voice_continue_as_text(&task_id, &body.text))
}

async fn voice_share(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<VoiceShareBody>,
) -> Response {
    voice_ok(rt.store.set_desktop_voice_share(&task_id, body.share))
}

async fn get_handoff(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.get_or_create_desktop_handoff(&task_id) {
        Ok(h) => (StatusCode::OK, Json(json!({ "handoff": h }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn reset_handoff(State(rt): State<AppState>, Path(task_id): Path<String>) -> Response {
    match rt.store.reset_desktop_handoff(&task_id) {
        Ok(h) => (StatusCode::OK, Json(json!({ "handoff": h }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
        )
            .into_response(),
    }
}

async fn handoff_draft(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<HandoffDraftBody>,
) -> Response {
    handoff_ok(
        rt.store
            .write_desktop_handoff_draft(&task_id, body.device.trim(), &body.text),
    )
}

async fn handoff_resolve(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<HandoffResolveBody>,
) -> Response {
    handoff_ok(
        rt.store
            .resolve_desktop_handoff(&task_id, body.chosen.trim()),
    )
}

async fn handoff_share_voice(
    State(rt): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<HandoffShareBody>,
) -> Response {
    handoff_ok(
        rt.store
            .set_desktop_handoff_voice_shared(&task_id, body.share),
    )
}

const RESUME_TASK_A: &str = "TASK-2026-018";
const RESUME_TASK_B: &str = "TASK-2026-021";

#[derive(Debug, Deserialize)]
struct ResumeFixtureBody {
    #[serde(default)]
    conversation_id: Option<String>,
}

fn publish_resume_event(
    rt: &Runtime,
    conversation_id: &str,
    task_id: &str,
    id: &str,
    kind: an_core::EventKind,
    status: Option<an_core::TaskStatus>,
    message: &str,
) {
    rt.bus.publish(an_core::TaskEvent {
        id: id.into(),
        conversation_id: conversation_id.into(),
        task_id: Some(task_id.into()),
        kind,
        status,
        message: message.into(),
        stream_id: None,
        created_at: an_core::now(),
    });
}

/// fixture：种子双任务（A running / B waiting→NeedsHuman），返回 conversation_id。
async fn resume_fixture_seed(State(rt): State<AppState>) -> Response {
    let conv = match rt.store.create_conversation("SMT-03 Resume fixture") {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "internal", "message": format!("{e:#}"), "retryable": true })),
            )
                .into_response();
        }
    };
    let cid = conv.id;
    // T-95/FIX-C（H4）：事件编号每次生成，不得写死——重复 seed 的主键冲突会整批回滚真实事件
    let ev_a = an_core::new_id("evt");
    let ev_b = an_core::new_id("evt");
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_A,
        &ev_a,
        an_core::EventKind::StatusChanged,
        Some(an_core::TaskStatus::Running),
        "采集中",
    );
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_B,
        &ev_b,
        an_core::EventKind::StatusChanged,
        Some(an_core::TaskStatus::NeedsHuman),
        "等待补充范围",
    );
    rt.store.flush_events();
    (
        StatusCode::OK,
        Json(json!({
            "conversation_id": cid,
            "after_cursor": ev_b,
            "event_ids": [ev_a, ev_b],
            "task_a": RESUME_TASK_A,
            "task_b": RESUME_TASK_B,
            "runs": [
                {
                    "task_id": RESUME_TASK_A,
                    "status": "running",
                    "progress": "采集中",
                    "version": 1
                },
                {
                    "task_id": RESUME_TASK_B,
                    "status": "waiting_input",
                    "progress": "等待补充范围",
                    "version": 1
                }
            ]
        })),
    )
        .into_response()
}

/// fixture：断线重放 A（含重复 event_id）；依赖已有 conversation_id。
async fn resume_fixture_replay_a(
    State(rt): State<AppState>,
    Json(body): Json<ResumeFixtureBody>,
) -> Response {
    let Some(cid) = body
        .conversation_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "conversation_id 必填",
                "retryable": false
            })),
        )
            .into_response();
    };
    if rt.store.get_conversation(&cid).ok().flatten().is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "会话不存在",
                "retryable": false
            })),
        )
            .into_response();
    }
    // 两个唯一 event_id；重复去重由 SSE/客户端保证（events.id 主键不可二次插入）
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_A,
        &an_core::new_id("evt"),
        an_core::EventKind::Progress,
        Some(an_core::TaskStatus::Running),
        "采集 40%",
    );
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_A,
        &an_core::new_id("evt"),
        an_core::EventKind::Progress,
        Some(an_core::TaskStatus::Running),
        "采集 80%",
    );
    rt.store.flush_events();
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "conversation_id": cid })),
    )
        .into_response()
}

/// fixture：B 补充输入后继续 running。
async fn resume_fixture_continue_b(
    State(rt): State<AppState>,
    Json(body): Json<ResumeFixtureBody>,
) -> Response {
    let Some(cid) = body
        .conversation_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": "bad_request",
                "message": "conversation_id 必填",
                "retryable": false
            })),
        )
            .into_response();
    };
    if rt.store.get_conversation(&cid).ok().flatten().is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "会话不存在",
                "retryable": false
            })),
        )
            .into_response();
    }
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_B,
        &an_core::new_id("evt"),
        an_core::EventKind::StatusChanged,
        Some(an_core::TaskStatus::Running),
        "已补充：限定 Line-B 批次",
    );
    rt.store.flush_events();
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "conversation_id": cid })),
    )
        .into_response()
}

fn envelope_from_cursor(item: &DesktopEventCursor) -> DesktopEventEnvelope {
    envelope_from_task_event(&item.event)
}

fn envelope_from_task_event(ev: &an_core::TaskEvent) -> DesktopEventEnvelope {
    DesktopEventEnvelope {
        event_id: ev.id.clone(),
        cursor: ev.id.clone(),
        conversation_id: ev.conversation_id.clone(),
        task_id: ev.task_id.clone(),
        kind: ev.kind,
        status: ev.status,
        message: ev.message.clone(),
        stream_id: ev.stream_id.clone(),
        created_at: ev.created_at.clone(),
    }
}

fn to_response(rec: &DesktopRequestRecord, replayed: bool) -> AcceptResponse {
    AcceptResponse {
        request_id: rec.request_id.clone(),
        message_id: rec.message_id.clone(),
        status: rec.status.clone(),
        task_id: rec.task_id.clone(),
        dispatch_count: rec.dispatch_count,
        replayed,
    }
}
