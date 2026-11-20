//! 运营频道的服务端接线（docs/mom/OPS_DESK.md，DECISIONS 第 56–58 条）。
//!
//! 只在 `[mom] chat_bridge = true` 时由 `main` 启用：每秒让发布器消费一次 MOM 发件箱，并挂桌面与未读接口。
//! 关闭时这里的代码一行都不执行，也不会注册任何路由。
//!
//! 超时、取消与清理：后台循环每次只做一次短的同步 tick；锁失败（引擎或发布器中毒）时循环退出并记录日志；
//! 进程退出时任务随运行时一起结束，没有需要清理的外部资源。

use std::sync::{Arc, Mutex};

use an_runtime::mom::ops_desk::{OpsPublisher, OPS_CHANNEL_ID};
use an_runtime::mom::ops_view;
use an_runtime::mom::Engine;
use axum::{
    extract::{Path, State},
    routing::get,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub type SharedPublisher = Arc<Mutex<OpsPublisher>>;
pub type SharedEngine = Arc<Mutex<Engine>>;

/// 桌面与未读接口共用的状态：引擎 + 发布器。
#[derive(Clone)]
pub struct OpsState {
    pub engine: SharedEngine,
    pub publisher: SharedPublisher,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 每秒 tick 一次。出错只记日志，不影响会话和 MOM 的其他功能。
pub fn spawn_publisher(
    engine: SharedEngine,
    publisher: SharedPublisher,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let step = (|| -> anyhow::Result<()> {
                let mut engine = engine
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime unavailable"))?;
                let mut publisher = publisher
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime unavailable"))?;
                publisher.tick(&mut engine, now_secs())?;
                Ok(())
            })();
            if let Err(error) = step {
                if error.to_string().contains("runtime unavailable") {
                    tracing::error!(event = "task.status", task_id = "ops-channel", error = %error, "运营频道发布器停止");
                    break;
                }
                tracing::warn!(event = "task.status", task_id = "ops-channel", error = %error, "运营频道发布失败，下一秒重试");
            }
        }
    })
}

pub fn router(state: OpsState) -> Router {
    Router::new()
        .route("/api/mom/desk/unread", get(unread))
        .route("/api/mom/desk/read", post(read))
        .route("/api/mom/desk", get(desk))
        .route("/api/mom/desk/cases/{id}", get(desk_case))
        .route(
            "/api/mom/desk/cases/{id}/open-in-channel",
            post(open_in_channel),
        )
        .route("/api/mom/desk/cases/{id}/approve", post(approve))
        .with_state(state)
}

type ApiResult = Result<Json<Value>, (axum::http::StatusCode, String)>;
fn fail(e: impl std::fmt::Display) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
fn not_found(id: &str) -> (axum::http::StatusCode, String) {
    (
        axum::http::StatusCode::NOT_FOUND,
        format!("case {id} not found"),
    )
}
fn conflict(e: impl std::fmt::Display) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::CONFLICT, e.to_string())
}

#[derive(Debug, Deserialize)]
struct ApproveBody {
    scope: String,
    plan_revision: u64,
}

async fn unread(State(s): State<OpsState>) -> ApiResult {
    let n = s.publisher.lock().map_err(fail)?.unread().map_err(fail)?;
    Ok(Json(json!({"unread": n})))
}
async fn read(State(s): State<OpsState>) -> ApiResult {
    s.publisher
        .lock()
        .map_err(fail)?
        .mark_read()
        .map_err(fail)?;
    Ok(Json(json!({"unread": 0})))
}

async fn desk(State(s): State<OpsState>) -> ApiResult {
    let engine = s.engine.lock().map_err(fail)?;
    let view = ops_view::desk_view(&engine).map_err(fail)?;
    Ok(Json(serde_json::to_value(view).map_err(fail)?))
}

async fn desk_case(State(s): State<OpsState>, Path(id): Path<String>) -> ApiResult {
    let engine = s.engine.lock().map_err(fail)?;
    match engine.human_view(&id) {
        Ok(v) => Ok(Json(v)),
        Err(e) if e.to_string().contains("unknown case") => Err(not_found(&id)),
        Err(e) => Err(fail(e)),
    }
}

async fn open_in_channel(State(s): State<OpsState>, Path(id): Path<String>) -> ApiResult {
    // 固定先锁引擎再锁发布器，与 spawn_publisher 一致，避免死锁
    let engine = s.engine.lock().map_err(fail)?;
    let publisher = s.publisher.lock().map_err(fail)?;
    match publisher.open_case(&engine, &id, now_secs()) {
        Ok(message_id) => Ok(Json(json!({
            "message_id": message_id,
            "conversation_id": OPS_CHANNEL_ID,
        }))),
        Err(e) if e.to_string().starts_with("E_CASE_NOT_FOUND") => Err(not_found(&id)),
        Err(e) => Err(fail(e)),
    }
}

/// 运营频道卡片上的批准（决定 60）。操作人固定 supervisor；不从请求体读 actor。
async fn approve(
    State(s): State<OpsState>,
    Path(id): Path<String>,
    Json(body): Json<ApproveBody>,
) -> ApiResult {
    let mut engine = s.engine.lock().map_err(fail)?;
    let publisher = s.publisher.lock().map_err(fail)?;
    let already = match engine.case(&id) {
        Ok(c) => c
            .approvals
            .get(&body.scope)
            .filter(|a| a.plan_revision == body.plan_revision)
            .map(|a| a.approver.clone()),
        Err(_) => return Err(not_found(&id)),
    };
    if let Some(approver) = already {
        publisher.refresh_case(&engine, &id).map_err(fail)?;
        return Ok(Json(json!({"already": true, "approver": approver})));
    }
    match engine.approve(&id, body.plan_revision, &body.scope, "supervisor") {
        Ok(()) => {
            publisher.refresh_case(&engine, &id).map_err(fail)?;
            Ok(Json(json!({"already": false, "approver": "supervisor"})))
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unknown case") {
                Err(not_found(&id))
            } else if msg.contains("stale") || msg.contains("expired") {
                Err(conflict(msg))
            } else {
                Err(fail(e))
            }
        }
    }
}
