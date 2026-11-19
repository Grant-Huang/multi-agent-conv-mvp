//! 运营频道的服务端接线（docs/mom/OPS_DESK.md，DECISIONS 第 56–58 条）。
//!
//! 只在 `[mom] chat_bridge = true` 时由 `main` 启用：每秒让发布器消费一次 MOM 发件箱，并挂两个未读接口。
//! 关闭时这里的代码一行都不执行，也不会注册任何路由。
//!
//! 超时、取消与清理：后台循环每次只做一次短的同步 tick；锁失败（引擎或发布器中毒）时循环退出并记录日志；
//! 进程退出时任务随运行时一起结束，没有需要清理的外部资源。

use std::sync::{Arc, Mutex};

use an_runtime::mom::ops_desk::OpsPublisher;
use an_runtime::mom::Engine;
use axum::{extract::State, routing::get, routing::post, Json, Router};
use serde_json::{json, Value};

pub type SharedPublisher = Arc<Mutex<OpsPublisher>>;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 每秒 tick 一次。出错只记日志，不影响会话和 MOM 的其他功能。
pub fn spawn_publisher(
    engine: Arc<Mutex<Engine>>,
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

pub fn router(publisher: SharedPublisher) -> Router {
    Router::new()
        .route("/api/mom/desk/unread", get(unread))
        .route("/api/mom/desk/read", post(read))
        .with_state(publisher)
}

type ApiResult = Result<Json<Value>, (axum::http::StatusCode, String)>;
fn fail(e: impl std::fmt::Display) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn unread(State(p): State<SharedPublisher>) -> ApiResult {
    let n = p.lock().map_err(fail)?.unread().map_err(fail)?;
    Ok(Json(json!({"unread": n})))
}
async fn read(State(p): State<SharedPublisher>) -> ApiResult {
    p.lock().map_err(fail)?.mark_read().map_err(fail)?;
    Ok(Json(json!({"unread": 0})))
}
