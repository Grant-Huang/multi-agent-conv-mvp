//! AgentNexus MVP 服务入口。
//!
//! 用法：`cargo run --bin agentnexus -- [配置文件路径]`（默认 `config.toml`）

use std::path::PathBuf;
use std::sync::Arc;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use anyhow::{Context, Result};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "config.toml".into()));
    let config = AppConfig::load(&config_path)?;

    // 日志：控制台 + JSON 文件（埋点，给自动排查用）+ 最近问题缓冲（/api/diagnostics）
    let log_dir = (!config.logging.dir.is_empty()).then(|| PathBuf::from(&config.logging.dir));
    let (diagnostics, _log_guard) = an_observe::init("agentnexus", &an_observe::LogConfig { dir: log_dir.clone(), level: config.logging.level.clone() });

    let store = Store::open(&config.server.db_path)?;
    let rt = Arc::new(Runtime::with_diagnostics(config.clone(), store, diagnostics)?);
    rt.start_background();

    // 启动时检查一次 LLM 连通性（失败不退出，方便先启动服务再开模型）
    match rt.llm.list_models().await {
        Ok(models) => tracing::info!(event = "llm.connected", base_url = %config.llm.base_url, ?models, "LLM 已连接"),
        Err(e) => tracing::warn!(event = "llm.unreachable", code = an_core::codes::LLM_UNREACHABLE, base_url = %config.llm.base_url, error = %format!("{e:#}"), "暂时无法连接 LLM"),
    }
    for card in rt.registry.cards() {
        tracing::info!(
            event = "agent.registered",
            "Agent [{:<8}] {}（{}，{}）{}",
            card.role.as_str(),
            card.name,
            card.id,
            card.kind,
            if card.enabled { "可用".to_string() } else { format!("不可用：{}", card.disabled_reason.unwrap_or_default()) }
        );
    }

    let mut app = an_server::api(rt.clone());
    // 如果前端已经构建（web/dist），一并提供静态页面
    let static_dir = &config.server.static_dir;
    if static_dir.join("index.html").exists() {
        let index = ServeFile::new(static_dir.join("index.html"));
        app = app.fallback_service(ServeDir::new(static_dir).not_found_service(index));
        tracing::info!("前端页面：http://{}", config.server.bind);
    } else {
        tracing::info!("未找到 {}/index.html，只提供 API（开发时请用 `npm run dev`）", static_dir.display());
    }

    let listener = tokio::net::TcpListener::bind(&config.server.bind).await.with_context(|| format!("无法监听 {}", config.server.bind))?;
    for role in an_core::Role::ALL {
        if let Ok(ids) = rt.registry.group_ids(role) {
            tracing::info!(event = "pool.resolved", role = role.as_str(), members = ?ids, "执行组");
        }
    }
    if let Some(d) = &log_dir {
        tracing::info!(event = "log.file", dir = %d.display(), "JSON 日志目录（用于自动排查）");
    }
    tracing::info!(event = "server.started", bind = %config.server.bind, "AgentNexus 已启动：http://{}", config.server.bind);
    axum::serve(listener, app.layer(TraceLayer::new_for_http())).await?;
    Ok(())
}
