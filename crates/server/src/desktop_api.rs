//! AI Desktop 扩展协议（B1 可靠受理；B2 游标 SSE）。路径挂在 `/api/desktop/v1`。

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use an_runtime::store::{
    DesktopAcceptOutcome, DesktopEventCursor, DesktopEventsAfter, DesktopRequestRecord,
};
use an_runtime::Runtime;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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
