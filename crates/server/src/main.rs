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
    // 日志级别通过 RUST_LOG 控制，例如 RUST_LOG=debug
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()))
        .init();

    let config_path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "config.toml".into()));
    let config = AppConfig::load(&config_path)?;
    let store = Store::open(&config.server.db_path)?;
    let rt = Arc::new(Runtime::new(config.clone(), store)?);

    // 启动时检查一次 LLM 连通性（失败不退出，方便先启动服务再开模型）
    match rt.llm.list_models().await {
        Ok(models) => tracing::info!("LLM 已连接 {}，模型：{:?}", config.llm.base_url, models),
        Err(e) => tracing::warn!("暂时无法连接 LLM {}：{e:#}", config.llm.base_url),
    }
    for card in rt.registry.cards() {
        tracing::info!(
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
    tracing::info!("AgentNexus 已启动：http://{}", config.server.bind);
    axum::serve(listener, app.layer(TraceLayer::new_for_http())).await?;
    Ok(())
}
