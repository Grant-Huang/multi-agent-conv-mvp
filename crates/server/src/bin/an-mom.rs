//! Independent local MOM simulator; never binds to a public interface.
use an_runtime::mom::{Engine, LlmMaster};
use an_server::mom::{self, AppState};
use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[tokio::main]
async fn main() -> Result<()> {
    let mut db = PathBuf::from("data/mom.db");
    let mut bind = "127.0.0.1:8790".to_string();
    let mut llm = None;
    let mut relos_url: Option<String> = std::env::var("AN_MOM_RELOS_URL").ok();
    // 阶段⑤a-3：模拟工厂（an-des-mcp）命令；设置后全程经连接器使用模拟工厂，不回退到进程内仿真。
    let mut des_mcp: Option<String> = std::env::var("AN_MOM_DES_MCP").ok();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next().context("--db path")?.into(),
            "--bind" => bind = args.next().context("--bind address")?,
            "--llm-config" => llm = Some(args.next().context("--llm-config config.toml")?),
            "--relos-url" => relos_url = Some(args.next().context("--relos-url http://host:port")?),
            "--des-mcp" => des_mcp = Some(args.next().context("--des-mcp <command>")?),
            "--help" => {
                println!("an-mom [--db data/mom.db] [--bind 127.0.0.1:8790] [--llm-config config.toml] [--relos-url http://127.0.0.1:8000] [--des-mcp an-des-mcp]\nSimulation only. Default Master and all domain tools are mocked.\n--relos-url (or AN_MOM_RELOS_URL): S1 diagnosis reads RelOS /v1/context/compile (5s timeout; simulated knowledge).\n--des-mcp (or AN_MOM_DES_MCP): run shop-floor DES simulations in the simulated factory started over MCP stdio; startup fails with E_DES_UNAVAILABLE if it cannot start (no fallback to the in-process simulation).");
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
    // #487：连接器注册表优先（connectors.toml，路径可用 CONNECTORS_TOML 覆盖）——
    // id="relos" 的描述符生效（endpoint + K2 凭据引用）。未注册按拍板②走
    // 「告警 + 旧配置回退」（先告警后拒绝，迁移期满翻转）。
    let connectors_path =
        std::env::var("CONNECTORS_TOML").unwrap_or_else(|_| "connectors.toml".into());
    let registry = an_runtime::connectors::load(Path::new(&connectors_path))?;
    if let Some(desc) = registry.lookup("relos") {
        let credential = an_runtime::connectors::resolve_credential(desc);
        if desc.credentials.is_some() && credential.is_none() {
            tracing::warn!(
                event = "connector.credential_missing",
                connector = "relos",
                "K2 凭据引用的 env 未设置：将以无凭据调用"
            );
        }
        state = state.with_relos_with_bearer(&desc.endpoint, credential.as_deref())?;
        println!(
            "RelOS via connector registry: {} (owner: {})",
            desc.endpoint,
            desc.owner.as_deref().unwrap_or("-")
        );
    } else if let Some(url) = relos_url.filter(|u| !u.is_empty()) {
        tracing::warn!(
            event = "connector.unregistered",
            connector = "relos",
            path = %connectors_path,
            "relos 未注册进连接器注册表：迁移期回退旧配置（先告警后拒绝）"
        );
        state = state.with_relos(&url)?;
        println!("RelOS context enabled (legacy): {url} (simulated knowledge; failures degrade to human decision)");
    }
    if let Some(path) = llm {
        let config = an_runtime::config::AppConfig::load(std::path::Path::new(&path))?;
        let mut cfg = config.llm;
        cfg.max_retries = 1;
        cfg.max_tokens = Some(512);
        cfg.timeout_secs = 45;
        state.master = Arc::new(LlmMaster::new(an_llm::LlmClient::new(cfg)?));
    }
    if let Some(command) = des_mcp.filter(|c| !c.trim().is_empty()) {
        // 管理口令每次启动随机生成，只交给子进程与连接器，不打印、不落盘。
        let admin_token = an_core::new_id("des-admin");
        let connector = an_server::mom_des_connector::DesConnector::spawn(
            std::path::Path::new(&command),
            &[],
            &admin_token,
        )
        .await
        .context("simulated factory (--des-mcp) failed to start")?;
        state = state.with_des(Arc::new(connector));
        println!("Simulated factory connected over MCP stdio: {command} (assumed parameters; not a real plant)");
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    // 打印实际监听地址（--bind 端口为 0 时是系统分配的端口）。
    let address = listener.local_addr()?;
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
