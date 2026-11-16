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
    let mut relos_url: Option<String> = std::env::var("AN_MOM_RELOS_URL").ok();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next().context("--db path")?.into(),
            "--bind" => bind = args.next().context("--bind address")?,
            "--llm-config" => llm = Some(args.next().context("--llm-config config.toml")?),
            "--relos-url" => relos_url = Some(args.next().context("--relos-url http://host:port")?),
            "--help" => {
                println!("an-mom [--db data/mom.db] [--bind 127.0.0.1:8790] [--llm-config config.toml] [--relos-url http://127.0.0.1:8000]\nSimulation only. Default Master and all domain tools are mocked.\n--relos-url (or AN_MOM_RELOS_URL): S1 diagnosis reads RelOS /v1/context/compile (5s timeout; simulated knowledge).");
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
    // 与主服务共用日志/诊断层，拒绝请求也必须能被定位。
    let (_diagnostics, _log_guard) = an_observe::init(
        "an-mom",
        &an_observe::LogConfig {
            dir: Some(PathBuf::from("logs")),
            level: "info".into(),
        },
    );
    if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut state = AppState::scripted(Arc::new(Mutex::new(Engine::open(db)?)));
    if let Some(url) = relos_url.filter(|u| !u.is_empty()) {
        state = state.with_relos(&url)?;
        println!("RelOS context enabled: {url} (simulated knowledge; failures degrade to human decision)");
    }
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
    let app = an_server::host_guard::protect(mom::router(state), listener.local_addr()?.port());
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    clock.abort();
    Ok(())
}
