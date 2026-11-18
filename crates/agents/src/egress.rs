//! 外发守卫（必经化方案第一、二阶段，见 docs/mcp/DLP_EGRESS_ENFORCEMENT_PLAN.md）。
//!
//! - 宿主（本模块）是强制执行点，dlp-egress 只做判定。三个 DLP 工具由宿主直接调用，**不注册给模型**。
//! - 工具调用：有外发绑定的工具，先把参数交给 DLP；enforce 模式下只把脱敏后的参数交给真实工具，
//!   REVIEW / BLOCK / DLP 异常时不调用真实工具（fail closed）。
//! - LLM 调用：云端后端的请求先脱敏；Token → 原值映射只留在本机，用于恢复模型输出（restore 用 local_ui）。
//! - task_id 来自任务上下文（`an_llm::egress::TASK_ID`），channel / destination_id / purpose 来自配置，都不来自模型。
//! - 模式：off（默认，不启动 DLP，行为不变）/ audit（检查并记录，从不拦截）/ enforce（拦截）。
//! - 日志不记录正文与参数值，只记录决策、原因码、request_id、耗时。
use crate::mcp::{compile_schema, open, McpServerConfig, McpTransport, Session};
use an_core::codes;
use an_llm::{
    egress::{current_task_id, BoxFut, LlmEgress, Outbound},
    ChatMessage, ErrorKind, LlmError,
};
use anyhow::{anyhow, bail, Context, Result};
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Mutex, time::timeout};

/// DLP 接口版本（D1 定稿）
const API_VERSION: &str = "1.0";
/// DLP 接受的通道取值（D1 定稿 9.3）
const CHANNELS: [&str; 6] = [
    "email",
    "slack",
    "external_api",
    "file_upload",
    "mcp",
    "agent",
];
/// LLM 消息合并成一次检查的上限；超过就逐条检查（DLP 单次载荷上限 256 KB）
const LLM_BATCH_BYTES: usize = 200 * 1024;
/// 启动 / 重连 DLP 的期限（含 Python 进程启动与工具发现，比单次判定慢得多）
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EgressMode {
    /// 不检查（默认），行为与以前完全相同
    #[default]
    Off,
    /// 检查并记录，从不拦截
    Audit,
    /// 拦截：只发送脱敏后的内容；REVIEW / BLOCK / DLP 异常时不发送
    Enforce,
}

impl EgressMode {
    fn as_str(self) -> &'static str {
        match self {
            EgressMode::Off => "off",
            EgressMode::Audit => "audit",
            EgressMode::Enforce => "enforce",
        }
    }
}

/// 一个工具的外发登记：调用它就意味着把参数发给 destination_id
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBinding {
    /// 工具名：内置工具如 `web_search`，MCP 工具为 `服务id.工具名`
    pub tool: String,
    pub destination_id: String,
    pub channel: String,
    pub purpose: String,
}

/// 一个 LLM 后端的外发登记（云端后端必须登记）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmBinding {
    /// `[backends.<名字>]` 中的名字；没有 `[backends]` 时为 `default`
    pub backend: String,
    pub destination_id: String,
    pub purpose: String,
}

/// `[egress]` 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressConfig {
    #[serde(default)]
    pub mode: EgressMode,
    /// 单次判定的期限；超时在 enforce 下视为不可发送
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// dlp-egress MCP 服务的启动命令（stdio）
    #[serde(default)]
    pub dlp_command: String,
    #[serde(default)]
    pub dlp_args: Vec<String>,
    #[serde(default)]
    pub dlp_cwd: Option<String>,
    #[serde(default)]
    pub tools: Vec<ToolBinding>,
    #[serde(default)]
    pub llm: Vec<LlmBinding>,
}

impl Default for EgressConfig {
    fn default() -> Self {
        Self {
            mode: EgressMode::Off,
            timeout_ms: default_timeout_ms(),
            dlp_command: String::new(),
            dlp_args: vec![],
            dlp_cwd: None,
            tools: vec![],
            llm: vec![],
        }
    }
}

fn default_timeout_ms() -> u64 {
    2000
}

/// 启动校验时，每个已注册工具属于哪一类外发路径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// 本机工具（时间、计算、读写工作目录），不外发
    Local,
    /// 内置联网工具（web_search / web_fetch）
    Tool,
    /// HTTP 远程 MCP 服务的工具：参数直接出网
    HttpMcpTool,
    /// 声明为 external_write 的工具
    ExternalWriteTool,
}

fn is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1") || host.starts_with("127.")
}

/// 启动校验。返回警告列表；enforce 下缺少必需的绑定时返回错误（拒绝启动）。
///
/// - `paths`：已注册的全部工具及其类别
/// - `backends`：(后端名, 实际地址)，按角色列出（旧写法中角色可以覆盖地址）
pub fn validate_bindings(
    cfg: &EgressConfig,
    paths: &[(String, PathKind)],
    backends: &[(String, String)],
) -> Result<Vec<String>> {
    if cfg.mode == EgressMode::Off {
        return Ok(vec![]);
    }
    if cfg.dlp_command.trim().is_empty() {
        bail!(
            "[egress] mode = {} 时必须配置 dlp_command",
            cfg.mode.as_str()
        );
    }
    if !(1..=60_000).contains(&cfg.timeout_ms) {
        bail!("[egress] timeout_ms 必须在 1..=60000 之间");
    }
    let known: BTreeMap<&str, PathKind> = paths.iter().map(|(n, k)| (n.as_str(), *k)).collect();
    for b in &cfg.tools {
        if !known.contains_key(b.tool.as_str()) {
            bail!(
                "[egress] 绑定的工具 {} 不存在（检查拼写；MCP 工具名为 服务id.工具名）",
                b.tool
            );
        }
        if !CHANNELS.contains(&b.channel.as_str()) {
            bail!(
                "[egress] 工具 {} 的 channel 必须是 {} 之一",
                b.tool,
                CHANNELS.join("/")
            );
        }
        if b.destination_id.trim().is_empty() || b.purpose.trim().is_empty() {
            bail!(
                "[egress] 工具 {} 必须填写 destination_id 和 purpose",
                b.tool
            );
        }
    }
    for b in &cfg.llm {
        if !backends.iter().any(|(n, _)| n == &b.backend) {
            bail!("[egress] 绑定的 LLM 后端 {} 不存在", b.backend);
        }
        if b.destination_id.trim().is_empty() || b.purpose.trim().is_empty() {
            bail!(
                "[egress] LLM 后端 {} 必须填写 destination_id 和 purpose",
                b.backend
            );
        }
    }
    let bound: std::collections::BTreeSet<&str> =
        cfg.tools.iter().map(|b| b.tool.as_str()).collect();
    let mut missing = vec![];
    for (name, kind) in paths {
        if *kind != PathKind::Local && !bound.contains(name.as_str()) {
            missing.push(format!("工具 {name}（{kind:?}）没有外发绑定"));
        }
    }
    let llm_bound: std::collections::BTreeSet<&str> =
        cfg.llm.iter().map(|b| b.backend.as_str()).collect();
    let mut seen = std::collections::BTreeSet::new();
    for (name, url) in backends {
        if !is_loopback(url) && !llm_bound.contains(name.as_str()) && seen.insert(name.clone()) {
            missing.push(format!("LLM 后端 {name} 不是本机地址，但没有外发绑定"));
        }
    }
    if cfg.mode == EgressMode::Enforce && !missing.is_empty() {
        bail!(
            "[egress] enforce 模式下以下外发路径必须登记：{}",
            missing.join("；")
        );
    }
    Ok(missing)
}

// ─────────────────────────────── DLP 客户端 ───────────────────────────────

struct Connected {
    session: Session,
    evaluate: jsonschema::Validator,
    restore: jsonschema::Validator,
}

struct DlpClient {
    server: McpServerConfig,
    state: Mutex<Option<Connected>>,
    call_timeout: Duration,
}

/// DLP 的一次判定结果（只取宿主需要的字段）
#[derive(Debug, Clone, Deserialize)]
struct Evaluation {
    decision: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    sanitized_text: Option<String>,
    #[serde(default)]
    sanitized_json: Option<Value>,
    #[serde(default)]
    reason_codes: Vec<String>,
    #[serde(default)]
    tokens: Vec<String>,
    ready_to_send: bool,
}

fn unavailable(reason: &str) -> anyhow::Error {
    anyhow!("{}: {reason}", codes::EGRESS_UNAVAILABLE)
}

impl DlpClient {
    fn new(cfg: &EgressConfig) -> Self {
        let server = McpServerConfig {
            id: "dlp".into(),
            transport: McpTransport::Stdio {
                command: cfg.dlp_command.clone(),
                args: cfg.dlp_args.clone(),
                cwd: cfg.dlp_cwd.clone(),
            },
            timeout_ms: cfg.timeout_ms,
            tools: vec![],
        };
        Self {
            server,
            state: Mutex::new(None),
            call_timeout: Duration::from_millis(cfg.timeout_ms),
        }
    }

    /// 启动 DLP 进程，完成握手并发现工具；要求 evaluate / restore 都提供 output schema
    async fn connect(&self) -> Result<Connected> {
        timeout(CONNECT_TIMEOUT, async {
            let session = open(&self.server)
                .await
                .map_err(|_| unavailable("connect"))?;
            let mut found = BTreeMap::new();
            let mut cursor = None;
            for _ in 0..32 {
                let page = session
                    .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map_err(|_| unavailable("discovery"))?;
                for t in page.tools {
                    found.insert(t.name.to_string(), t);
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            let schema = |name: &str| -> Result<jsonschema::Validator> {
                let tool = found.get(name).ok_or_else(|| unavailable("tool missing"))?;
                let s = tool
                    .output_schema
                    .as_ref()
                    .ok_or_else(|| unavailable("output schema missing"))?;
                compile_schema(&Value::Object((**s).clone())).map_err(|_| unavailable("bad schema"))
            };
            Ok(Connected {
                evaluate: schema("egress_evaluate")?,
                restore: schema("egress_restore")?,
                session,
            })
        })
        .await
        .map_err(|_| unavailable("connect timeout"))?
    }

    /// 调用一个 DLP 工具，返回 `data`。会话异常时丢弃，下次调用重新拉起。
    async fn call(&self, tool: &str, args: Map<String, Value>) -> Result<Value> {
        let mut slot = self.state.lock().await;
        let conn = match slot.take() {
            Some(c) if !c.session.is_closed() => c,
            _ => self.connect().await?,
        };
        // 超时或出错时 conn 随 future 一起被丢弃，子进程被终止（kill_on_drop），不复用脏会话
        let outcome = timeout(self.call_timeout, async {
            let result = conn
                .session
                .peer()
                .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args))
                .await
                .map_err(|_| unavailable("protocol"))?;
            Ok::<_, anyhow::Error>((result, conn))
        })
        .await
        .map_err(|_| unavailable("timeout"))??;
        let (result, conn) = outcome;
        let validator = if tool == "egress_restore" {
            &conn.restore
        } else {
            &conn.evaluate
        };
        let checked = (|| {
            if result.is_error == Some(true) {
                return Err(unavailable("tool error"));
            }
            let sc = result
                .structured_content
                .as_ref()
                .ok_or_else(|| unavailable("no structured content"))?;
            if tool != "egress_revoke" && !validator.is_valid(sc) {
                return Err(unavailable("output does not match schema"));
            }
            if sc["api_version"] != API_VERSION {
                return Err(unavailable("unsupported api_version"));
            }
            if sc["status"] != "success" {
                let code = sc["error"]["code"].as_str().unwrap_or("error");
                return Err(anyhow!("E_EGRESS_DLP_ERROR: {code}"));
            }
            Ok(sc["data"].clone())
        })();
        *slot = Some(conn);
        checked
    }

    async fn evaluate(
        &self,
        task_id: &str,
        channel: &str,
        destination_id: &str,
        purpose: &str,
        payload: (&str, Value),
    ) -> Result<Evaluation> {
        let mut args = Map::new();
        args.insert("task_id".into(), json!(task_id));
        args.insert("channel".into(), json!(channel));
        args.insert("destination_id".into(), json!(destination_id));
        args.insert("purpose".into(), json!(purpose));
        args.insert(payload.0.into(), payload.1);
        let data = self.call("egress_evaluate", args).await?;
        serde_json::from_value(data).map_err(|_| unavailable("evaluation shape"))
    }

    async fn restore(&self, task_id: &str, text: &str) -> Result<String> {
        let mut args = Map::new();
        args.insert("task_id".into(), json!(task_id));
        args.insert("text".into(), json!(text));
        args.insert("context".into(), json!("local_ui"));
        let data = self.call("egress_restore", args).await?;
        data["text"]
            .as_str()
            .map(str::to_string)
            .context("restore shape")
    }
}

// ─────────────────────────────── 守卫 ───────────────────────────────

pub struct EgressGuard {
    mode: EgressMode,
    client: DlpClient,
    tools: BTreeMap<String, ToolBinding>,
    llm: BTreeMap<String, LlmBinding>,
}

/// 判定结论（enforce 下的处理依据）
enum Verdict {
    Pass(Evaluation),
    Review(Evaluation),
    Block(Evaluation),
}

fn verdict(ev: Evaluation) -> Verdict {
    match (ev.decision.as_str(), ev.ready_to_send) {
        ("ALLOW" | "ANONYMIZE", true) => Verdict::Pass(ev),
        ("REVIEW", _) => Verdict::Review(ev),
        _ => Verdict::Block(ev),
    }
}

fn short(e: &anyhow::Error) -> String {
    an_core::truncate_chars(&format!("{e}"), 120)
}

impl EgressGuard {
    /// 按配置启动。off → `None`（不启动任何进程）。enforce 下 DLP 无法启动 → 返回错误（拒绝启动）；
    /// audit 下只记录错误，之后每次调用会尝试重连。
    pub async fn start(cfg: &EgressConfig) -> Result<Option<Arc<Self>>> {
        if cfg.mode == EgressMode::Off {
            return Ok(None);
        }
        if cfg.dlp_command.trim().is_empty() {
            bail!(
                "[egress] mode = {} 时必须配置 dlp_command",
                cfg.mode.as_str()
            );
        }
        let client = DlpClient::new(cfg);
        match client.connect().await {
            Ok(c) => *client.state.lock().await = Some(c),
            Err(e) if cfg.mode == EgressMode::Audit => {
                tracing::error!(event = "egress.guard_unavailable", code = codes::EGRESS_UNAVAILABLE, mode = "audit", reason = %short(&e), "外发检查服务启动失败（audit 模式：继续运行，只记录）");
            }
            Err(e) => return Err(e.context("[egress] enforce 模式下外发检查服务必须可用")),
        }
        tracing::info!(
            event = "egress.started",
            mode = cfg.mode.as_str(),
            tools = cfg.tools.len(),
            llm = cfg.llm.len(),
            "外发检查已启用"
        );
        Ok(Some(Arc::new(Self {
            mode: cfg.mode,
            client,
            tools: cfg
                .tools
                .iter()
                .map(|b| (b.tool.clone(), b.clone()))
                .collect(),
            llm: cfg
                .llm
                .iter()
                .map(|b| (b.backend.clone(), b.clone()))
                .collect(),
        })))
    }

    pub fn mode(&self) -> EgressMode {
        self.mode
    }

    fn task_id() -> String {
        current_task_id().unwrap_or_else(|| an_core::new_id("untracked"))
    }

    fn log(
        &self,
        path: &str,
        task_id: &str,
        destination_id: &str,
        ev: &Evaluation,
        elapsed: Instant,
    ) {
        let ms = elapsed.elapsed().as_millis() as u64;
        tracing::info!(event = "egress.evaluate", task_id, path, destination_id, decision = %ev.decision, reason_codes = ?ev.reason_codes, request_id = %ev.request_id, mode = self.mode.as_str(), elapsed_ms = ms, "外发检查");
    }

    fn log_blocked(&self, path: &str, task_id: &str, code: &str, ev: &Evaluation) {
        tracing::warn!(event = "egress.blocked", code, task_id, path, decision = %ev.decision, reason_codes = ?ev.reason_codes, request_id = %ev.request_id, mode = self.mode.as_str(), "外发被拦截，内容没有发出");
    }

    fn log_unavailable(&self, path: &str, task_id: &str, e: &anyhow::Error) {
        tracing::error!(event = "egress.guard_unavailable", code = codes::EGRESS_UNAVAILABLE, task_id, path, mode = self.mode.as_str(), reason = %short(e), "外发检查服务不可用");
    }

    /// 工具调用前检查。返回实际交给工具的参数；enforce 下不可发送时返回错误（不得调用工具）
    pub async fn check_tool(&self, tool: &str, args: &Value) -> Result<Value> {
        let Some(b) = self.tools.get(tool) else {
            return Ok(args.clone());
        };
        let task = Self::task_id();
        let t0 = Instant::now();
        let r = self
            .client
            .evaluate(
                &task,
                &b.channel,
                &b.destination_id,
                &b.purpose,
                ("json_body", args.clone()),
            )
            .await;
        let ev = match r {
            Ok(ev) => ev,
            Err(e) => {
                self.log_unavailable(tool, &task, &e);
                return match self.mode {
                    EgressMode::Enforce => Err(anyhow!(
                        "{}: 外发检查不可用，未调用 {tool}",
                        codes::EGRESS_UNAVAILABLE
                    )),
                    _ => Ok(args.clone()),
                };
            }
        };
        self.log(tool, &task, &b.destination_id, &ev, t0);
        if self.mode != EgressMode::Enforce {
            return Ok(args.clone());
        }
        match verdict(ev) {
            Verdict::Pass(ev) => ev.sanitized_json.clone().ok_or_else(|| {
                self.log_unavailable(tool, &task, &anyhow!("missing sanitized_json"));
                anyhow!(
                    "{}: 外发检查返回不完整，未调用 {tool}",
                    codes::EGRESS_UNAVAILABLE
                )
            }),
            Verdict::Review(ev) => {
                self.log_blocked(tool, &task, codes::EGRESS_REVIEW, &ev);
                Err(anyhow!(
                    "{}: 内容需要人工审批，未调用 {tool}（request_id={}，原因：{}）",
                    codes::EGRESS_REVIEW,
                    ev.request_id,
                    ev.reason_codes.join(",")
                ))
            }
            Verdict::Block(ev) => {
                self.log_blocked(tool, &task, codes::EGRESS_BLOCKED, &ev);
                Err(anyhow!(
                    "{}: 内容不允许外发，未调用 {tool}（request_id={}，原因：{}）",
                    codes::EGRESS_BLOCKED,
                    ev.request_id,
                    ev.reason_codes.join(",")
                ))
            }
        }
    }

    /// 给某个 LLM 后端的检查钩子；该后端没有绑定时返回 None
    pub fn llm_hook(self: &Arc<Self>, backend: &str) -> Option<Arc<dyn LlmEgress>> {
        let binding = self.llm.get(backend)?.clone();
        Some(Arc::new(LlmHook {
            guard: self.clone(),
            binding,
        }))
    }

    async fn check_llm(
        &self,
        b: &LlmBinding,
        messages: &[ChatMessage],
    ) -> Result<Outbound, LlmError> {
        let path = format!("llm:{}", b.backend);
        let task = Self::task_id();
        let passthrough = || Outbound {
            messages: messages.to_vec(),
            restore: HashMap::new(),
        };
        let t0 = Instant::now();
        let evaluated = self.evaluate_messages(&task, b, messages).await;
        let (evs, sanitized) = match evaluated {
            Ok(x) => x,
            Err(e) => {
                self.log_unavailable(&path, &task, &e);
                return match self.mode {
                    EgressMode::Enforce => Err(LlmError::new(
                        ErrorKind::EgressUnavailable,
                        format!("{}: 外发检查不可用，未调用模型", codes::EGRESS_UNAVAILABLE),
                    )),
                    _ => Ok(passthrough()),
                };
            }
        };
        for ev in &evs {
            self.log(&path, &task, &b.destination_id, ev, t0);
        }
        if self.mode != EgressMode::Enforce {
            return Ok(passthrough());
        }
        let mut tokens = vec![];
        for ev in evs {
            match verdict(ev) {
                Verdict::Pass(ev) => tokens.extend(ev.tokens),
                Verdict::Review(ev) => {
                    self.log_blocked(&path, &task, codes::EGRESS_REVIEW, &ev);
                    return Err(LlmError::new(
                        ErrorKind::EgressReview,
                        format!(
                            "{}: 内容需要人工审批，未调用模型（request_id={}）",
                            codes::EGRESS_REVIEW,
                            ev.request_id
                        ),
                    ));
                }
                Verdict::Block(ev) => {
                    self.log_blocked(&path, &task, codes::EGRESS_BLOCKED, &ev);
                    return Err(LlmError::new(
                        ErrorKind::EgressBlocked,
                        format!(
                            "{}: 内容不允许发给模型（request_id={}）",
                            codes::EGRESS_BLOCKED,
                            ev.request_id
                        ),
                    ));
                }
            }
        }
        let Some(sanitized) = sanitized else {
            return Err(LlmError::new(
                ErrorKind::EgressUnavailable,
                format!("{}: 外发检查返回不完整", codes::EGRESS_UNAVAILABLE),
            ));
        };
        tokens.sort();
        tokens.dedup();
        let restore = if tokens.is_empty() {
            HashMap::new()
        } else {
            match self.client.restore(&task, &tokens.join("\n")).await {
                Ok(text) => {
                    let values: Vec<&str> = text.split('\n').collect();
                    if values.len() == tokens.len() {
                        tokens
                            .into_iter()
                            .zip(values)
                            .map(|(t, v)| (t, v.to_string()))
                            .collect()
                    } else {
                        HashMap::new()
                    }
                }
                Err(e) => {
                    // 恢复失败只影响显示（用户看到 Token），不影响安全：照常发送脱敏内容
                    tracing::warn!(event = "egress.restore_unavailable", code = codes::EGRESS_UNAVAILABLE, task_id = %task, path = %path, reason = %short(&e), "模型输出中的 Token 无法在本机恢复（检查 DLP 的 allow_restore）");
                    HashMap::new()
                }
            }
        };
        Ok(Outbound {
            messages: sanitized,
            restore,
        })
    }

    /// 检查一组消息：不超过上限时合并为一次 json_body 检查，否则逐条检查（同一 task 作用域，Token 一致）
    async fn evaluate_messages(
        &self,
        task: &str,
        b: &LlmBinding,
        messages: &[ChatMessage],
    ) -> Result<(Vec<Evaluation>, Option<Vec<ChatMessage>>)> {
        let body: Vec<Value> = messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect();
        let size = serde_json::to_string(&body).map_or(usize::MAX, |s| s.len());
        if size <= LLM_BATCH_BYTES {
            let ev = self
                .client
                .evaluate(
                    task,
                    "external_api",
                    &b.destination_id,
                    &b.purpose,
                    ("json_body", Value::Array(body)),
                )
                .await?;
            let sanitized = ev.sanitized_json.as_ref().and_then(|v| {
                let arr = v.as_array()?;
                if arr.len() != messages.len() {
                    return None;
                }
                arr.iter()
                    .zip(messages)
                    .map(|(x, m)| {
                        Some(ChatMessage {
                            role: m.role.clone(),
                            content: x["content"].as_str()?.to_string(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()
            });
            return Ok((vec![ev], sanitized));
        }
        let mut evs = vec![];
        let mut out = Some(vec![]);
        for m in messages {
            let ev = self
                .client
                .evaluate(
                    task,
                    "external_api",
                    &b.destination_id,
                    &b.purpose,
                    ("text", json!(m.content)),
                )
                .await?;
            match (&mut out, &ev.sanitized_text) {
                (Some(v), Some(t)) => v.push(ChatMessage {
                    role: m.role.clone(),
                    content: t.clone(),
                }),
                _ => out = None,
            }
            evs.push(ev);
        }
        Ok((evs, out))
    }
}

struct LlmHook {
    guard: Arc<EgressGuard>,
    binding: LlmBinding,
}

impl LlmEgress for LlmHook {
    fn outbound<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> BoxFut<'a, Result<Outbound, LlmError>> {
        Box::pin(self.guard.check_llm(&self.binding, messages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        for u in [
            "http://127.0.0.1:8098/v1",
            "http://localhost/v1",
            "http://[::1]:80/v1",
            "http://127.0.0.2/v1",
        ] {
            assert!(is_loopback(u), "{u}");
        }
        for u in [
            "https://api.example.com/v1",
            "http://10.0.0.5/v1",
            "https://localhost.evil.com/v1",
            "http://user@example.com/",
        ] {
            assert!(!is_loopback(u), "{u}");
        }
    }

    #[test]
    fn config_defaults_off_and_rejects_unknown_fields() {
        let cfg: EgressConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(cfg.mode, EgressMode::Off);
        assert_eq!(cfg.timeout_ms, 2000);
        assert!(
            serde_json::from_value::<EgressConfig>(json!({"mode":"enforce","tenat":"x"})).is_err()
        );
    }
}
