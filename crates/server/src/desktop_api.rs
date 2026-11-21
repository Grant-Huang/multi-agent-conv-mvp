//! AI Desktop 扩展协议（B1 起）：可靠受理等。路径挂在 `/api/desktop/v1`。

use std::sync::Arc;

use an_runtime::store::{DesktopAcceptOutcome, DesktopRequestRecord};
use an_runtime::Runtime;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/desktop/v1/conversations/{id}/requests",
            post(create_request),
        )
        .route("/api/desktop/v1/requests/{id}", get(get_request))
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
