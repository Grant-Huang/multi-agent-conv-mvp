//! AgentNexus 节点命令行程序。
//!
//! 用法：
//! ```text
//! an-node                       # 使用默认配置
//! an-node node.toml             # 使用配置文件
//! an-node --port 7799 --name 办公室电脑 --workdir ~/code/demo --no-advertise
//! ```

use std::path::PathBuf;

use an_node::{lan_ip, Node, NodeConfig};
use anyhow::{bail, Result};

/// 解析命令行参数（参数不多，不引入额外的解析库）
fn parse_args() -> Result<NodeConfig> {
    let mut args = std::env::args().skip(1).peekable();
    let mut config_path: Option<PathBuf> = None;
    let mut overrides: Vec<(String, String)> = vec![];
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("用法：an-node [node.toml] [--port 端口] [--name 名称] [--workdir 目录] [--bind 地址] [--no-advertise]");
                std::process::exit(0);
            }
            "--no-advertise" => overrides.push(("advertise".into(), String::new())),
            "--port" | "--name" | "--workdir" | "--bind" => match args.next() {
                Some(v) => overrides.push((a.trim_start_matches("--").into(), v)),
                None => bail!("{a} 后面需要一个值"),
            },
            other if !other.starts_with('-') && config_path.is_none() => config_path = Some(other.into()),
            other => bail!("无法识别的参数 {other}，用 --help 查看用法"),
        }
    }
    let mut cfg = NodeConfig::load(config_path.as_deref())?;
    for (k, v) in overrides {
        match k.as_str() {
            "advertise" => cfg.advertise = false,
            "port" => cfg.port = v.parse().map_err(|_| anyhow::anyhow!("端口必须是数字"))?,
            "name" => cfg.name = Some(v),
            "workdir" => cfg.workdir = v.into(),
            "bind" => cfg.bind = v,
            _ => unreachable!(),
        }
    }
    Ok(cfg)
}

/// 把配对码按 3+3 位显示，便于读出
fn pretty_code(code: &str) -> String {
    format!("{} {}", &code[..3], &code[3..])
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = parse_args()?;
    let log_dir = (!cfg.log_dir.is_empty()).then(|| PathBuf::from(an_agents::util::expand_tilde(&cfg.log_dir)));
    let (diagnostics, _guard) = an_observe::init("an-node", &an_observe::LogConfig { dir: log_dir.clone(), level: "info".into() });

    let node = Node::new(&cfg, diagnostics, Box::new(|code| println!("\n新的配对码：{}（10 分钟内有效）\n", pretty_code(code))))?;
    let listener = tokio::net::TcpListener::bind((cfg.bind.as_str(), cfg.port)).await.map_err(|e| anyhow::anyhow!("无法监听 {}:{}：{e}", cfg.bind, cfg.port))?;
    let port = listener.local_addr()?.port();

    // 局域网广播（失败不退出：用户仍可以按地址手动添加）
    let _mdns = if cfg.advertise {
        match node.advertise(port) {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::warn!(event = "node.advertise_failed", code = an_core::codes::DISCOVERY, error = %format!("{e:#}"), "局域网广播失败，AgentNexus 需要按地址手动添加本节点");
                None
            }
        }
    } else {
        None
    };

    // ───── 启动信息：用户需要看到的都在这里 ─────
    let line = "═".repeat(44);
    println!("\n{line}\n AgentNexus 节点已启动");
    println!(" 名称：{}（{}）", node.info.name, node.info.node_id);
    if cfg.bind == "127.0.0.1" {
        println!(" 地址：http://127.0.0.1:{port}（只允许本机访问）");
    } else {
        match lan_ip() {
            Some(ip) => println!(" 地址：http://{ip}:{port}"),
            None => println!(" 地址：http://<本机 IP>:{port}"),
        }
    }
    println!(" 局域网自动发现：{}", if _mdns.is_some() { "已开启" } else { "未开启（需在 AgentNexus 中按地址添加）" });
    let cards = node.agents();
    if cards.is_empty() {
        println!(" agent：没有发现。安装 opencode / hermes / pi-acp，或在 node.toml 中添加 [[acp.agents]]");
    } else {
        println!(" agent：");
        for c in &cards {
            let status = if c.enabled { "✓".to_string() } else { format!("✗ {}", c.disabled_reason.clone().unwrap_or_default()) };
            println!("   {status} {}（{}，角色 {}，并发 {}）", c.id, c.name, c.role.as_str(), c.max_concurrent);
        }
    }
    println!(" 配对码：{}（10 分钟内有效，在 AgentNexus 的「节点」页输入）", pretty_code(&node.pairing_code()));
    if let Some(d) = &log_dir {
        println!(" 日志：{}", d.display());
    }
    println!("{line}\n");

    node.start_code_rotation();
    axum::serve(listener, node.router()).await?;
    Ok(())
}
