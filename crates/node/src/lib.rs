//! AgentNexus 节点：在一台机器上运行，把这台机器上的 agent 提供给局域网内的 AgentNexus 使用。
//!
//! 节点做的事（都在节点所在的机器上完成）：
//! - 自动发现本机安装的 ACP agent（opencode、hermes、pi-acp），也可以在 node.toml 中自定义；
//! - 在局域网广播自己（mDNS，跨平台），方便 AgentNexus 自动找到；
//! - 显示 6 位配对码，AgentNexus 输入正确的配对码后获得长期令牌；
//! - 收到任务后在本机启动 agent 执行，把进度和结果实时传回；工作目录、权限规则、git diff 都在本机处理；
//! - AgentNexus 断开连接（取消任务）时立即终止本机的 agent 进程。

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_agents::util::expand_tilde;
use an_agents::{acp, AcpAgent, AcpConfig};
use an_core::node::{NodeError, NodeInfo, PairRequest, PairResponse, Progress, RunRequest, PROTOCOL_VERSION, SERVICE_TYPE};
use an_core::{codes, AgentAdapter, AgentCard, AgentError, EventKind, EventPublisher, EventSink, TaskEvent};
use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// 配对码有效期
const CODE_TTL: Duration = Duration::from_secs(600);
/// 配对码最多输错几次，超过后更换配对码（防止暴力尝试）
const MAX_PAIR_FAILURES: u32 = 5;
/// 默认端口
pub const DEFAULT_PORT: u16 = 7788;

// ───────────────────────── 配置 ─────────────────────────

/// 节点配置（node.toml，全部可选）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// 显示名称，默认是本机名
    #[serde(default)]
    pub name: Option<String>,
    /// 监听地址。局域网使用需要 0.0.0.0；只给本机用可以改成 127.0.0.1
    #[serde(default = "d_bind")]
    pub bind: String,
    #[serde(default = "d_port")]
    pub port: u16,
    /// agent 的默认工作目录（建议是一个 git 仓库，便于收集改动证据）
    #[serde(default = "d_workdir")]
    pub workdir: PathBuf,
    /// 保存节点身份和令牌的目录
    #[serde(default = "d_state")]
    pub state_dir: PathBuf,
    /// 是否在局域网广播（mDNS）
    #[serde(default = "d_true")]
    pub advertise: bool,
    /// JSON 日志目录；为空表示不写文件
    #[serde(default = "d_logs")]
    pub log_dir: String,
    #[serde(default)]
    pub acp: NodeAcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAcp {
    /// 自动发现本机安装的常见 ACP agent
    #[serde(default = "d_true")]
    pub auto_detect: bool,
    /// 自定义 agent（写法与 AgentNexus 的 [[acp.agents]] 相同）
    #[serde(default)]
    pub agents: Vec<AcpConfig>,
}

impl Default for NodeAcp {
    fn default() -> Self {
        Self { auto_detect: true, agents: vec![] }
    }
}

fn d_bind() -> String {
    "0.0.0.0".into()
}
fn d_port() -> u16 {
    DEFAULT_PORT
}
fn d_workdir() -> PathBuf {
    "~/agentnexus-node/workspace".into()
}
fn d_state() -> PathBuf {
    "~/.agentnexus-node".into()
}
fn d_true() -> bool {
    true
}
fn d_logs() -> String {
    "~/.agentnexus-node/logs".into()
}

impl Default for NodeConfig {
    fn default() -> Self {
        toml::from_str("").expect("默认配置")
    }
}

impl NodeConfig {
    /// 读取配置文件；文件不存在时使用默认配置
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(p).with_context(|| format!("无法读取节点配置 {}", p.display()))?;
                toml::from_str(&text).with_context(|| format!("节点配置格式错误 {}", p.display()))
            }
            None => Ok(Self::default()),
        }
    }

    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(an_core::hostname)
    }
}

fn expand(p: &Path) -> PathBuf {
    PathBuf::from(expand_tilde(&p.to_string_lossy()))
}

// ───────────────────────── 持久化状态 ─────────────────────────

/// 保存在 state_dir/node.json：节点 id 和已发放的令牌
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Persisted {
    node_id: String,
    tokens: Vec<TokenRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenRecord {
    token: String,
    client_name: String,
    created_at: String,
}

fn load_or_create_state(dir: &Path) -> Result<Persisted> {
    std::fs::create_dir_all(dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;
    let path = dir.join("node.json");
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(p) = serde_json::from_str::<Persisted>(&text) {
            if !p.node_id.is_empty() {
                return Ok(p);
            }
        }
    }
    let p = Persisted { node_id: an_core::new_id("node"), tokens: vec![] };
    save_state(dir, &p)?;
    Ok(p)
}

fn save_state(dir: &Path, p: &Persisted) -> Result<()> {
    let path = dir.join("node.json");
    std::fs::write(&path, serde_json::to_string_pretty(p)?)?;
    // 文件里有令牌，只允许当前用户读写
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ───────────────────────── 节点 ─────────────────────────

struct Pairing {
    code: String,
    expires: Instant,
    failures: u32,
}

/// 生成 6 位数字配对码
fn new_code() -> String {
    let n = u32::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 1_000_000;
    format!("{n:06}")
}

pub struct Node {
    pub info: NodeInfo,
    agents: Vec<Arc<dyn AgentAdapter>>,
    /// agent id → 正在执行的任务数
    busy: Mutex<HashMap<String, u32>>,
    state: Mutex<Persisted>,
    state_dir: PathBuf,
    pairing: Mutex<Pairing>,
    diagnostics: Arc<an_observe::Diagnostics>,
    /// 配对码变化时的回调（命令行程序用它把新配对码打印出来）
    on_new_code: Box<dyn Fn(&str) + Send + Sync>,
}

impl Node {
    /// 创建节点：读取身份、发现本机 agent、生成配对码
    pub fn new(cfg: &NodeConfig, diagnostics: Arc<an_observe::Diagnostics>, on_new_code: Box<dyn Fn(&str) + Send + Sync>) -> Result<Arc<Self>> {
        let state_dir = expand(&cfg.state_dir);
        let state = load_or_create_state(&state_dir)?;
        let workdir = expand(&cfg.workdir);
        std::fs::create_dir_all(&workdir).with_context(|| format!("无法创建工作目录 {}", workdir.display()))?;
        let name = cfg.display_name();

        // 先注册自定义 agent，再注册本机已安装的预设（id 相同时自定义优先）
        let mut agents: Vec<Arc<dyn AgentAdapter>> = vec![];
        let mut add = |c: AcpConfig, only_installed: bool| {
            if agents.iter().any(|a| a.card().id == c.id) {
                return;
            }
            let agent = AcpAgent::new(c, workdir.clone());
            if only_installed && !agent.is_installed() {
                return;
            }
            agents.push(Arc::new(agent));
        };
        for c in &cfg.acp.agents {
            add(c.clone(), false);
        }
        if cfg.acp.auto_detect {
            for c in acp::presets() {
                add(c, true);
            }
        }

        let info = NodeInfo {
            node_id: state.node_id.clone(),
            name,
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL_VERSION,
            os: std::env::consts::OS.into(),
        };
        let node = Arc::new(Self {
            info,
            agents,
            busy: Mutex::default(),
            state: Mutex::new(state),
            state_dir,
            pairing: Mutex::new(Pairing { code: new_code(), expires: Instant::now() + CODE_TTL, failures: 0 }),
            diagnostics,
            on_new_code,
        });
        Ok(node)
    }

    pub fn agents(&self) -> Vec<AgentCard> {
        self.agents.iter().map(|a| a.card().clone()).collect()
    }

    /// 当前有效的配对码（过期则自动更换）
    pub fn pairing_code(&self) -> String {
        let mut p = self.pairing.lock().unwrap();
        if Instant::now() >= p.expires {
            self.rotate_code(&mut p, "配对码已过期，已更换");
        }
        p.code.clone()
    }

    fn rotate_code(&self, p: &mut Pairing, reason: &str) {
        p.code = new_code();
        p.expires = Instant::now() + CODE_TTL;
        p.failures = 0;
        tracing::info!(event = "node.code_rotated", reason, "配对码已更换");
        (self.on_new_code)(&p.code);
    }

    /// 定期检查配对码是否过期（过期后更换并通知）
    pub fn start_code_rotation(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let _ = this.pairing_code();
            }
        });
    }

    fn client_of(&self, headers: &HeaderMap) -> Option<String> {
        let token = headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")?.trim();
        let st = self.state.lock().unwrap();
        st.tokens.iter().find(|t| t.token == token).map(|t| t.client_name.clone())
    }

    /// 校验令牌，返回调用方名称；失败时记录日志并返回 None（调用方用 [`unauthorized`] 响应）
    fn authorize(&self, headers: &HeaderMap) -> Option<String> {
        let client = self.client_of(headers);
        if client.is_none() {
            tracing::warn!(event = "node.auth_failed", code = codes::NODE_AUTH, "收到未授权的请求");
        }
        client
    }

    /// 在局域网广播本节点（mDNS）。返回的守护进程需要保持存活
    pub fn advertise(&self, port: u16) -> Result<mdns_sd::ServiceDaemon> {
        let daemon = mdns_sd::ServiceDaemon::new().context("无法启动 mDNS")?;
        let host = format!("{}.local.", an_agents::remote::node_slug(&an_core::hostname()));
        let props = [
            ("id", self.info.node_id.as_str()),
            ("name", self.info.name.as_str()),
            ("ver", self.info.version.as_str()),
            ("proto", "1"),
            ("os", self.info.os.as_str()),
        ];
        // 实例名带上 id 前缀，避免同名机器冲突
        let instance = format!("{} {}", self.info.name, &self.info.node_id[..13.min(self.info.node_id.len())]);
        let info = mdns_sd::ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, &props[..])
            .context("mDNS 服务信息无效")?
            .enable_addr_auto();
        daemon.register(info).context("mDNS 注册失败")?;
        tracing::info!(event = "node.advertised", service = SERVICE_TYPE, port, "已在局域网广播");
        Ok(daemon)
    }

    /// HTTP 接口
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/node/info", get(info))
            .route("/node/pair", post(pair))
            .route("/node/agents", get(agents))
            .route("/node/run", post(run))
            .route("/node/diagnostics", get(diagnostics))
            .with_state(self.clone())
    }
}

/// 401：未配对或令牌无效
fn unauthorized() -> Response {
    err(StatusCode::UNAUTHORIZED, codes::NODE_AUTH, "未配对或令牌无效，请在 AgentNexus 中重新配对")
}

/// 统一的错误响应
fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(NodeError { code: code.into(), message: message.into() })).into_response()
}

type St = State<Arc<Node>>;

async fn info(State(node): St) -> Json<NodeInfo> {
    Json(node.info.clone())
}

async fn pair(State(node): St, Json(req): Json<PairRequest>) -> Response {
    let mut p = node.pairing.lock().unwrap();
    if Instant::now() >= p.expires {
        node.rotate_code(&mut p, "配对码已过期，已更换");
        return err(StatusCode::UNAUTHORIZED, codes::NODE_PAIR, "配对码已过期，请使用节点上显示的新配对码");
    }
    if req.code.trim() != p.code {
        p.failures += 1;
        tracing::warn!(event = "node.pair_failed", code = codes::NODE_PAIR, client = %req.client_name, failures = p.failures, "配对码错误");
        if p.failures >= MAX_PAIR_FAILURES {
            node.rotate_code(&mut p, "配对码输错次数过多，已更换");
            return err(StatusCode::UNAUTHORIZED, codes::NODE_PAIR, "配对码输错次数过多，节点已更换配对码");
        }
        return err(StatusCode::UNAUTHORIZED, codes::NODE_PAIR, "配对码错误");
    }
    // 配对成功：发放令牌；配对码一次性使用，立即更换
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    {
        let mut st = node.state.lock().unwrap();
        st.tokens.push(TokenRecord { token: token.clone(), client_name: req.client_name.clone(), created_at: an_core::now() });
        if let Err(e) = save_state(&node.state_dir, &st) {
            tracing::error!(event = "node.state_save_failed", error = %e);
            return err(StatusCode::INTERNAL_SERVER_ERROR, codes::NODE_PAIR, format!("保存令牌失败：{e:#}"));
        }
    }
    tracing::info!(event = "node.paired", client = %req.client_name, "配对成功");
    node.rotate_code(&mut p, "配对码已使用，已更换");
    Json(PairResponse { token, node_id: node.info.node_id.clone(), name: node.info.name.clone() }).into_response()
}

async fn agents(State(node): St, headers: HeaderMap) -> Response {
    if node.authorize(&headers).is_none() {
        return unauthorized();
    }
    Json(node.agents()).into_response()
}

async fn diagnostics(State(node): St, headers: HeaderMap) -> Response {
    if node.authorize(&headers).is_none() {
        return unauthorized();
    }
    Json(serde_json::json!({
        "node": node.info,
        "agents": node.agents(),
        "busy": *node.busy.lock().unwrap(),
        "problem_counts": node.diagnostics.counts_by_code(),
        "recent_problems": node.diagnostics.recent(100),
    }))
    .into_response()
}

/// 把 agent 的进度事件转发到 SSE
struct ChannelPublisher(mpsc::UnboundedSender<Event>);

impl EventPublisher for ChannelPublisher {
    fn publish(&self, event: TaskEvent) {
        if event.kind == EventKind::Progress {
            if let Ok(e) = Event::default().event("progress").json_data(Progress { message: event.message }) {
                let _ = self.0.send(e);
            }
        }
    }
}

/// 归还执行名额
struct BusyGuard {
    node: Arc<Node>,
    agent_id: String,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(n) = self.node.busy.lock().unwrap().get_mut(&self.agent_id) {
            *n = n.saturating_sub(1);
        }
    }
}

/// 连接断开（AgentNexus 取消任务）时终止执行
struct AbortOnDrop {
    handle: tokio::task::JoinHandle<()>,
    task_id: String,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if !self.handle.is_finished() {
            tracing::info!(event = "node.run_cancelled", task_id = %self.task_id, "AgentNexus 断开连接，终止任务");
            self.handle.abort();
        }
    }
}

async fn run(State(node): St, headers: HeaderMap, Json(req): Json<RunRequest>) -> Response {
    let Some(client) = node.authorize(&headers) else {
        return unauthorized();
    };
    let Some(agent) = node.agents.iter().find(|a| a.card().id == req.agent_id).cloned() else {
        return err(StatusCode::NOT_FOUND, codes::NODE_AGENT_NOT_FOUND, format!("节点上没有 agent `{}`", req.agent_id));
    };
    let card = agent.card().clone();
    if !card.enabled {
        return err(StatusCode::CONFLICT, codes::AGENT_NOT_INSTALLED, card.disabled_reason.unwrap_or_default());
    }
    // 并发上限：AgentNexus 侧已经按 max_concurrent 分配，这里再保护一次（例如多个 AgentNexus 共用一个节点）
    {
        let mut busy = node.busy.lock().unwrap();
        let n = busy.entry(card.id.clone()).or_insert(0);
        if *n >= card.max_concurrent {
            tracing::warn!(event = "node.busy", code = codes::NODE_BUSY, agent_id = %card.id, "agent 已达到并发上限");
            return err(StatusCode::CONFLICT, codes::NODE_BUSY, format!("{} 正在执行 {} 个任务，已达上限", card.name, n));
        }
        *n += 1;
    }
    let guard = BusyGuard { node: node.clone(), agent_id: card.id.clone() };

    let task = req.task;
    let task_id = task.task_id.clone();
    tracing::info!(event = "node.run_start", task_id = %task_id, agent_id = %card.id, client = %client);
    let (tx, rx) = mpsc::unbounded_channel::<Event>();
    let sink = EventSink::new(Arc::new(ChannelPublisher(tx.clone())), &task.conversation_id, &task.task_id);
    let started = Instant::now();
    let tid = task_id.clone();
    let handle = tokio::spawn(async move {
        let event = match agent.run(task, sink).await {
            Ok(r) => {
                tracing::info!(event = "node.run_end", task_id = %tid, agent_id = %card.id, status = ?r.status, elapsed_ms = started.elapsed().as_millis() as u64);
                Event::default().event("result").json_data(&r)
            }
            Err(e) => {
                let (code, message) = match e.downcast_ref::<AgentError>() {
                    Some(ae) => (ae.code.to_string(), ae.message.clone()),
                    None => (codes::AGENT_RUNTIME.to_string(), format!("{e:#}")),
                };
                tracing::error!(event = "node.run_failed", code = %code, task_id = %tid, agent_id = %card.id, error = %message);
                Event::default().event("error").json_data(NodeError { code, message })
            }
        };
        if let Ok(ev) = event {
            let _ = tx.send(ev);
        }
    });
    let abort = AbortOnDrop { handle, task_id };
    // 流被丢弃（连接断开）时，guard 和 abort 一起被丢弃：归还名额并终止任务
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(move |e| {
        let _keep = (&guard, &abort);
        Ok::<_, Infallible>(e)
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response()
}

/// 本机在局域网中的 IP（用于显示地址）。通过"连接"一个外部地址让系统选出网卡，不会真的发送数据
pub fn lan_ip() -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // TEST-NET 地址，不会产生流量
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_six_digits() {
        for _ in 0..100 {
            let c = new_code();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|ch| ch.is_ascii_digit()));
        }
    }

    #[test]
    fn default_config() {
        let c = NodeConfig::default();
        assert_eq!(c.port, DEFAULT_PORT);
        assert_eq!(c.bind, "0.0.0.0");
        assert!(c.acp.auto_detect && c.advertise);
    }
}
