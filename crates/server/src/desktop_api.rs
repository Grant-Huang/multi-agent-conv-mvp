//! AI Desktop 扩展协议（B1 可靠受理；B2 游标 SSE；B3 关联与配置）。路径挂在 `/api/desktop/v1`。

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use an_core::EventPublisher;
use an_runtime::store::{
    DesktopAcceptOutcome, DesktopApprovalDecideOutcome, DesktopApprovalExecuteOutcome,
    DesktopEventCursor, DesktopEventsAfter, DesktopEvidenceAccess, DesktopLinkOutcome,
    DesktopPrefPatchOutcome, DesktopRequestRecord,
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

pub fn routes() -> Router<AppState> {
    Router::new()
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
        .route(
            "/api/desktop/v1/preferences",
            get(get_preferences).patch(patch_preferences),
        )
        // T-71：记忆列表服务权威（夹具种子，无新表；完整记忆抽取另卡）
        .route("/api/desktop/v1/memories", get(list_memories))
        .route("/api/desktop/v1/memories/{id}", get(get_memory))
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
        // T-64 harness fixture：往 bus 灌事件供 B2 SSE 消费（无新表）
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
}

fn principal_from(headers: &HeaderMap) -> String {
    headers
        .get("x-desktop-principal")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("alice")
        .to_string()
}

async fn create_request(
    State(rt): State<AppState>,
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
            let turn_rt = rt.clone();
            tokio::spawn(async move {
                if let Err(e) = turn_rt
                    .conversation
                    .continue_after_accepted(&conv, &text)
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
                "message": "相同 client_operation_id 已绑定不同正文",
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

/// 桌面记忆条目（夹具；字段对齐 DESIGN Memory：来源、正文、有效期）
#[derive(Debug, Clone, Serialize)]
struct DesktopMemoryItem {
    id: String,
    title: String,
    source: String,
    body: String,
    expires: String,
}

fn fixture_memories() -> Vec<DesktopMemoryItem> {
    vec![
        DesktopMemoryItem {
            id: "mem-1".into(),
            title: "质量报告偏好摘要格式".into(),
            source: "用户确认 · 2026-11-18".into(),
            body: "先给结论，再列证据条目与待核实项。".into(),
            expires: "2027-11-18".into(),
        },
        DesktopMemoryItem {
            id: "mem-2".into(),
            title: "免打扰时段".into(),
            source: "设置 · 通知与主动性".into(),
            body: "工作日 22:00–08:00 不推普通建议。".into(),
            expires: "长期".into(),
        },
    ]
}

async fn list_memories() -> Response {
    (
        StatusCode::OK,
        Json(json!({ "items": fixture_memories() })),
    )
        .into_response()
}

async fn get_memory(Path(id): Path<String>) -> Response {
    match fixture_memories().into_iter().find(|m| m.id == id) {
        Some(item) => (StatusCode::OK, Json(item)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "code": "not_found",
                "message": "记忆条目不存在",
                "retryable": false
            })),
        )
            .into_response(),
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
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_A,
        "seed-a-status",
        an_core::EventKind::StatusChanged,
        Some(an_core::TaskStatus::Running),
        "采集中",
    );
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_B,
        "seed-b-status",
        an_core::EventKind::StatusChanged,
        Some(an_core::TaskStatus::NeedsHuman),
        "等待补充范围",
    );
    rt.store.flush_events();
    (
        StatusCode::OK,
        Json(json!({
            "conversation_id": cid,
            "after_cursor": "seed-b-status",
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
        "ev-a-1",
        an_core::EventKind::Progress,
        Some(an_core::TaskStatus::Running),
        "采集 40%",
    );
    publish_resume_event(
        &rt,
        &cid,
        RESUME_TASK_A,
        "ev-a-2",
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
        "ev-b-continue",
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
