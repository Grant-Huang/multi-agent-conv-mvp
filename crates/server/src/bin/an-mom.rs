//! Independent local MOM simulator; never binds to a public interface.
use an_runtime::mom::{Engine, LlmMaster};
use an_server::mom::{self, AppState};
use anyhow::{bail, Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[tokio::main]
async fn main() -> Result<()> {
    let mut db = PathBuf::from("data/mom.db");
    let mut bind = "127.0.0.1:8790".to_string();
    let mut llm = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next().context("--db path")?.into(),
            "--bind" => bind = args.next().context("--bind address")?,
            "--llm-config" => llm = Some(args.next().context("--llm-config config.toml")?),
            "--help" => {
                println!("an-mom [--db data/mom.db] [--bind 127.0.0.1:8790] [--llm-config config.toml]\nSimulation only. Default Master and all domain tools are mocked.");
                return Ok(());
            }
            _ => bail!("unknown option {arg}"),
        }
    }
    let address: std::net::SocketAddr = bind.parse()?;
    anyhow::ensure!(
        address.ip().is_loopback(),
        "MOM simulator must bind loopback: fixture identities are not production authentication"
    );
    if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut state = AppState::scripted(Arc::new(Mutex::new(Engine::open(db)?)));
    if let Some(path) = llm {
        let config = an_runtime::config::AppConfig::load(std::path::Path::new(&path))?;
        let mut cfg = config.llm;
        cfg.max_retries = 1;
        cfg.max_tokens = Some(512);
        cfg.timeout_secs = 45;
        state.master = Arc::new(LlmMaster::new(an_llm::LlmClient::new(cfg)?));
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("MOM simulation listening on {address}. Domain data and approvals are fixtures.");
    let clock = mom::spawn_clock(state.clone());
    axum::serve(listener, mom::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    clock.abort();
    Ok(())
}
