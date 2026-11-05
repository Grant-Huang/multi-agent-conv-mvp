//! OpenAI 兼容的 Chat Completions 客户端。
//!
//! 针对本地小模型做了几件事：
//! 1. 结构化输出：通过 `response_format` 把 JSON Schema 交给推理服务做语法约束
//!    （llama.cpp 会把 schema 转成 GBNF 语法），同时在 prompt 里也写明格式；
//! 2. 容错解析：去掉 `<think>…</think>`、```json 代码块，提取第一个完整 JSON 对象；
//! 3. 校验 + 重试：解析失败或业务校验不通过时，把错误信息反馈给模型再试一次
//!    （借鉴 Pydantic AI 的"结构化输出 + 闭环重试"）。

mod backend;
mod budget;
mod error;
mod policy;
mod scheduler;
mod status;
mod tokens;

pub use backend::{parse_props, Backend, BackendConfig, BackendKind, Priority, ServerProps};
pub use budget::{clip_middle, fit, ContextBudget, TrimReport};
pub use error::{is_context_error, parse_retry_after, ErrorKind, LlmError};
pub use policy::{backoff_delay, jitter01, Breaker, CallRecord, Stats, StatsSummary, Timeouts};
pub use scheduler::{Lease, SchedSnapshot, Scheduler, SlotInfo, Who};
pub use status::{parse_metrics, parse_slots, ServerMetrics, ServerSlot, ServerView};
pub use tokens::{estimate_messages, estimate_tokens};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use an_core::codes;
use serde_json::{json, Value};
use tokio::time::{timeout_at, Instant as TokioInstant};

/// 输出被截断时，`max_tokens` 最多放大到这个值
const MAX_TOKENS_CAP: u32 = 16384;
/// 没配 `max_tokens` 时，估算预算用的输出长度
const DEFAULT_OUTPUT_TOKENS: u32 = 2048;
/// 提示词不到这么多 token 时，"超上下文"不是裁剪能解决的，不再缩减重试
const MIN_SHRINK_TOKENS: u32 = 600;

fn next_req_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// 结构化输出的实现方式。不同推理服务支持程度不同，所以做成可配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StructuredMode {
    /// `{"type":"json_schema","json_schema":{...}}`：OpenAI 标准格式，新版 llama.cpp 支持
    #[default]
    JsonSchema,
    /// `{"type":"json_object","schema":{...}}`：llama.cpp 早期的扩展写法
    JsonObject,
    /// 不发 response_format，只靠 prompt 约束（兼容性最好，小模型出错率最高）
    PromptOnly,
}

/// LLM 连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// 例如 `http://127.0.0.1:8098/v1`
    pub base_url: String,
    /// llama.cpp 默认不需要，留空即可
    #[serde(default)]
    pub api_key: String,
    /// 模型名。llama.cpp 单模型部署时基本会被忽略
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub structured_mode: StructuredMode,
    /// 结构化输出失败后最多重试几次
    #[serde(default = "default_retries")]
    pub max_retries: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            temperature: default_temperature(),
            max_tokens: None,
            timeout_secs: default_timeout(),
            structured_mode: StructuredMode::default(),
            max_retries: default_retries(),
        }
    }
}

fn default_temperature() -> f32 {
    0.2
}
fn default_timeout() -> u64 {
    180
}
fn default_retries() -> u32 {
    2
}

/// 一条对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(c: impl Into<String>) -> Self {
        Self { role: "system".into(), content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self { role: "user".into(), content: c.into() }
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: c.into() }
    }
}

/// 结构化调用的结果，附带调试信息
#[derive(Debug, Clone)]
pub struct Structured<T> {
    pub value: T,
    /// 每次尝试时模型的原始输出
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
}

/// 结构化调用失败时也带上原始输出，便于写日志
#[derive(Debug)]
pub struct StructuredError {
    pub message: String,
    /// 错误码（`an_core::codes`）：格式问题是 `E_LLM_FORMAT`，其余按模型调用错误分类
    pub code: &'static str,
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
}

impl std::fmt::Display for StructuredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}（共尝试 {} 次）", self.message, self.attempts)
    }
}
impl std::error::Error for StructuredError {}

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    cfg: LlmConfig,
    /// 所属后端（按配置路由时才有；测试里直接 `new` 的客户端没有）
    backend: Option<Arc<Backend>>,
    /// 角色名（conversation / reply / router / general / reviewer），埋点用
    role: String,
    priority: Priority,
    timeouts: Timeouts,
}

/// 一次调用的请求内容（`messages` 之外的参数）
struct CallReq<'a> {
    messages: &'a [ChatMessage],
    response_format: Option<Value>,
    max_tokens: Option<u32>,
    /// 结构化输出的 schema 名（普通对话为空），日志用
    schema: &'a str,
    req_id: u64,
    /// 第几次格式重试（普通对话恒为 1）
    format_attempt: u32,
    /// 调用方是否在消费增量文本；是的话，已经推送过文本就不能再重试（否则用户会看到重复内容）
    streaming: bool,
}

/// 日志 / 统计用的调用标识
struct CallInfo<'a> {
    req_id: u64,
    schema: &'a str,
    format_attempt: u32,
    /// 第几次网络重试（0 = 首次）
    retry: u32,
    /// 本次占用的槽位（只有 llama_cpp 后端有）
    slot: Option<u32>,
    queue_wait_ms: u64,
}

/// 一次成功收到完整流的结果
#[derive(Debug, Default)]
struct Attempt {
    /// 模型输出的原文（内联的 `<think>` 段落未去掉）
    text: String,
    finish_reason: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    ttft_ms: Option<u64>,
    total_ms: u64,
}

/// 整个调用（含重试）的期限
struct Deadline {
    start: Instant,
    budget: Option<Duration>,
}

impl Deadline {
    fn new(budget: Option<Duration>) -> Self {
        Self { start: Instant::now(), budget }
    }

    fn remaining(&self) -> Option<Duration> {
        self.budget.map(|b| b.saturating_sub(self.start.elapsed()))
    }
}

/// 调用方中途放弃（future 被丢弃）时，记一条 `cancelled`；连接随之关闭，推理服务会停止生成
struct CancelGuard<'a> {
    client: &'a LlmClient,
    info: &'a CallInfo<'a>,
    started: Instant,
    done: bool,
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.client.emit(self.info, "cancelled", self.started.elapsed().as_millis() as u64, None, None);
        }
    }
}

fn build_http(connect: Duration) -> Result<reqwest::Client> {
    // 不设整体超时：流式响应的各段超时由 `stream_once` 自己控制
    reqwest::Client::builder().connect_timeout(connect).build().context("创建 HTTP 客户端失败")
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        // 没绑定后端的客户端不设调用期限（和以前一致）
        let timeouts = Timeouts::defaults(cfg.timeout_secs, Priority::Background);
        let http = build_http(timeouts.connect)?;
        Ok(Self { http, cfg, backend: None, role: String::new(), priority: Priority::Interactive, timeouts })
    }

    /// 复制一个使用其他分段超时的客户端
    pub fn with_timeouts(&self, timeouts: Timeouts) -> Self {
        let mut c = self.clone();
        if timeouts.connect != self.timeouts.connect {
            if let Ok(h) = build_http(timeouts.connect) {
                c.http = h;
            }
        }
        c.timeouts = timeouts;
        c
    }

    pub fn timeouts(&self) -> &Timeouts {
        &self.timeouts
    }

    /// 复制一个绑定到某个后端、角色和优先级的客户端；超时按优先级取默认值，再用后端配置覆盖
    pub fn with_backend(&self, backend: Arc<Backend>, role: &str, priority: Priority) -> Self {
        let b = &backend.cfg;
        let mut t = Timeouts::defaults(self.cfg.timeout_secs, priority);
        let secs = Duration::from_secs;
        t.connect = b.connect_secs.map_or(t.connect, secs);
        t.first_token = b.first_token_secs.map_or(t.first_token, secs);
        t.idle = b.idle_secs.map_or(t.idle, secs);
        t.total = b.total_secs.map_or(t.total, secs);
        if let Some(d) = b.deadline_secs {
            t.deadline = (d > 0).then(|| secs(d));
        }
        let mut c = self.with_timeouts(t);
        c.backend = Some(backend);
        c.role = role.to_string();
        c.priority = priority;
        c
    }

    pub fn backend(&self) -> Option<&Arc<Backend>> {
        self.backend.as_ref()
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// 复制一个使用其他模型名的客户端（按角色配置不同模型时使用）
    pub fn with_model(&self, model: &str) -> Self {
        let mut c = self.clone();
        c.cfg.model = model.to_string();
        c
    }

    /// 复制一个连接到其他服务地址的客户端（不同角色的模型跑在不同 llama.cpp 实例时使用）
    pub fn with_base_url(&self, base_url: &str) -> Self {
        let mut c = self.clone();
        c.cfg.base_url = base_url.to_string();
        c
    }

    pub fn model(&self) -> &str {
        &self.cfg.model
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.cfg.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    /// 请求体：所有调用统一走流式（断开即取消生成），并要求服务端在最后附带 token 用量
    fn request_body(&self, req: &CallReq<'_>, slot: Option<u32>) -> Value {
        let mut body = json!({
            "model": self.cfg.model,
            "messages": req.messages,
            "temperature": self.cfg.temperature,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = json!(mt);
        }
        if let Some(rf) = &req.response_format {
            body["response_format"] = rf.clone();
        }
        // 固定槽位：指定的槽位被占时 llama.cpp 会死等，所以槽位必须是调度器挑好的空闲槽位
        if let Some(s) = slot {
            body["id_slot"] = json!(s);
        }
        if let Some(b) = &self.backend {
            for (k, v) in &b.cfg.extra_body {
                if !matches!(k.as_str(), "model" | "messages" | "stream") {
                    body[k] = v.clone();
                }
            }
        }
        body
    }

    /// 发一次流式请求并累积结果。超时分三段：等响应 / 首 token 之前受 `first_token` 限制，
    /// 开始生成后每个 token 间隔受 `idle` 限制，整次请求受 `total` 限制。
    async fn stream_once<F: FnMut(&str)>(&self, req: &CallReq<'_>, slot: Option<u32>, remaining: Option<Duration>, on_delta: &mut F) -> std::result::Result<Attempt, LlmError> {
        use futures::StreamExt;
        let t = &self.timeouts;
        let start = TokioInstant::now();
        let first_budget = remaining.map_or(t.first_token, |r| t.first_token.min(r));
        let first_deadline = start + first_budget;
        let total_deadline = start + t.total;
        let limit_for = |first_pending: bool| -> (TokioInstant, ErrorKind) {
            let (mut limit, mut kind) = if first_pending { (first_deadline, ErrorKind::TimeoutFirstToken) } else { (TokioInstant::now() + t.idle, ErrorKind::TimeoutIdle) };
            if total_deadline <= limit {
                (limit, kind) = (total_deadline, ErrorKind::TimeoutTotal);
            }
            (limit, kind)
        };
        let timeout_error = |kind: ErrorKind| {
            let msg = match kind {
                ErrorKind::TimeoutFirstToken => format!("等待 {} 秒仍未收到第一个 token（排队或提示词预填充过久）", first_budget.as_secs()),
                ErrorKind::TimeoutIdle => format!("生成过程中 {} 秒没有新的 token", t.idle.as_secs()),
                _ => format!("单次请求超过总时长上限 {} 秒", t.total.as_secs()),
            };
            LlmError::new(kind, msg)
        };

        let mut http_req = self.http.post(self.endpoint("chat/completions")).json(&self.request_body(req, slot));
        if !self.cfg.api_key.is_empty() {
            http_req = http_req.bearer_auth(&self.cfg.api_key);
        }
        let (limit, kind) = limit_for(true);
        let resp = match timeout_at(limit, http_req.send()).await {
            Err(_) => return Err(timeout_error(kind)),
            Ok(Err(e)) => return Err(LlmError::new(ErrorKind::Unreachable, format!("无法连接 LLM 服务 {}：{e}", self.cfg.base_url))),
            Ok(Ok(r)) => r,
        };
        let status = resp.status();
        if !status.is_success() {
            let retry_after = resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(parse_retry_after);
            let body = tokio::time::timeout(Duration::from_secs(10), resp.text()).await.ok().and_then(|r| r.ok()).unwrap_or_default();
            return Err(LlmError::from_http(status.as_u16(), &body, retry_after));
        }

        // SSE：每行 `data: {json}`，最后一行 `data: [DONE]`。按字节切行，避免多字节字符被分包截断
        let mut stream = resp.bytes_stream();
        let (mut full, mut sent, mut buf) = (String::new(), String::new(), Vec::<u8>::new());
        let (mut first_seen, mut done) = (false, false);
        let mut a = Attempt::default();
        'outer: loop {
            let (limit, kind) = limit_for(!first_seen);
            let Some(chunk) = timeout_at(limit, stream.next()).await.map_err(|_| timeout_error(kind))? else { break };
            buf.extend_from_slice(&chunk.map_err(|e| LlmError::new(ErrorKind::StreamCut, format!("读取流式响应失败：{e}")))?);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else { continue };
                if data == "[DONE]" {
                    done = true;
                    break 'outer;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                if let Some(err) = v.get("error") {
                    let code = err["code"].as_u64().unwrap_or(500) as u16;
                    return Err(LlmError::from_http(code, &err.to_string(), None));
                }
                let choice = &v["choices"][0];
                let delta = &choice["delta"];
                let piece = delta["content"].as_str().unwrap_or("");
                // 推理内容（reasoning_content）不展示，但说明模型还在生成，用于"卡住"判断和首 token 计时
                let thinking = delta["reasoning_content"].as_str().or_else(|| delta["reasoning"].as_str()).unwrap_or("");
                if !first_seen && !(piece.is_empty() && thinking.is_empty()) {
                    first_seen = true;
                    a.ttft_ms = Some(start.elapsed().as_millis() as u64);
                }
                if !piece.is_empty() {
                    full.push_str(piece);
                    // 只推送新增的可见部分
                    let visible = visible_text(&full);
                    if visible.len() > sent.len() && visible.starts_with(&sent) {
                        on_delta(&visible[sent.len()..]);
                        sent = visible;
                    }
                }
                if let Some(fr) = choice["finish_reason"].as_str() {
                    a.finish_reason = Some(fr.to_string());
                }
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    a.prompt_tokens = u["prompt_tokens"].as_u64();
                    a.completion_tokens = u["completion_tokens"].as_u64();
                }
            }
        }
        if !done && a.finish_reason.is_none() {
            return Err(LlmError::new(ErrorKind::StreamCut, format!("流式响应在结束标记之前中断（已收到 {} 个字符）", full.chars().count())));
        }
        a.text = full;
        a.total_ms = start.elapsed().as_millis() as u64;
        Ok(a)
    }

    /// 记录一次尝试：调用统计、`llm.call` 日志
    fn emit(&self, info: &CallInfo<'_>, outcome: &'static str, total_ms: u64, a: Option<&Attempt>, err: Option<&LlmError>) {
        let backend = self.backend.as_ref().map(|b| b.name.as_str()).unwrap_or("-");
        let (ttft, pt, ct) = a.map(|a| (a.ttft_ms, a.prompt_tokens, a.completion_tokens)).unwrap_or((None, None, None));
        let finish = a.and_then(|a| a.finish_reason.as_deref());
        if let Some(b) = &self.backend {
            b.stats.record(CallRecord { role: self.role.clone(), priority: self.priority, outcome, attempt: info.format_attempt, slot: info.slot, queue_wait_ms: info.queue_wait_ms, ttft_ms: ttft, total_ms, prompt_tokens: pt, completion_tokens: ct });
        }
        if err.is_some() || outcome == "truncated" {
            let code = err.map(|e| e.kind.code()).unwrap_or(codes::LLM_TRUNCATED);
            let msg = err.map(|e| e.message.clone()).unwrap_or_default();
            tracing::warn!(event = "llm.call", code, req_id = info.req_id, role = %self.role, priority = self.priority.as_str(), schema = info.schema, backend, model = %self.cfg.model, base_url = %self.cfg.base_url, slot_id = info.slot, attempt = info.format_attempt, retry = info.retry, queue_wait_ms = info.queue_wait_ms, ttft_ms = ttft, total_ms, prompt_tokens = pt, completion_tokens = ct, finish_reason = finish, outcome, error = %msg, "模型调用未成功");
        } else {
            tracing::info!(event = "llm.call", req_id = info.req_id, role = %self.role, priority = self.priority.as_str(), schema = info.schema, backend, model = %self.cfg.model, base_url = %self.cfg.base_url, slot_id = info.slot, attempt = info.format_attempt, retry = info.retry, queue_wait_ms = info.queue_wait_ms, ttft_ms = ttft, total_ms, prompt_tokens = pt, completion_tokens = ct, finish_reason = finish, outcome);
        }
    }

    /// 一次尝试：熔断检查 → 流式请求 → 统计 / 日志 / 熔断计数。调用方中途丢弃（取消）时记为 `cancelled`
    async fn call_once<F: FnMut(&str)>(&self, req: &CallReq<'_>, retry: u32, deadline: &Deadline, on_delta: &mut F) -> std::result::Result<Attempt, LlmError> {
        let mut info = CallInfo { req_id: req.req_id, schema: req.schema, format_attempt: req.format_attempt, retry, slot: None, queue_wait_ms: 0 };
        if let Some(b) = &self.backend {
            if let Err(left) = b.breaker.check() {
                let e = LlmError::new(ErrorKind::BreakerOpen, format!("后端 {} 连续失败，已熔断，约 {} 秒后自动试探恢复", b.name, left.as_secs() + 1));
                self.emit(&info, e.kind.outcome(), 0, None, Some(&e));
                return Err(e);
            }
        }
        // 申请槽位 / 并发许可（排队时间也受调用期限约束）；租约在本函数结束或被取消时自动归还
        let mut lease = None;
        if let Some(b) = &self.backend {
            let who = Who { role: &self.role, priority: self.priority, req_id: req.req_id };
            let default_wait = match self.priority {
                Priority::Interactive => Duration::from_secs(30),
                Priority::Background => Duration::from_secs(600),
            };
            let base = b.cfg.queue_secs.map_or(default_wait, Duration::from_secs);
            let max_wait = deadline.remaining().map_or(base, |r| base.min(r));
            let est = estimate_messages(req.messages).saturating_add(req.max_tokens.unwrap_or(1024));
            let queued = Instant::now();
            match b.sched.acquire(&who, est, max_wait).await {
                Ok(l) => {
                    info.slot = l.slot();
                    info.queue_wait_ms = l.queue_wait_ms();
                    lease = Some(l);
                }
                Err(e) => {
                    info.queue_wait_ms = queued.elapsed().as_millis() as u64;
                    self.emit(&info, e.kind.outcome(), 0, None, Some(&e));
                    return Err(e);
                }
            }
        }
        let started = Instant::now();
        let mut guard = CancelGuard { client: self, info: &info, started, done: false };
        let result = self.stream_once(req, info.slot, deadline.remaining(), on_delta).await;
        guard.done = true;
        let total_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(a) => {
                let outcome = if a.finish_reason.as_deref() == Some("length") { "truncated" } else { "ok" };
                self.emit(&info, outcome, total_ms, Some(a), None);
                if let (Some(l), Some(p), Some(c)) = (&lease, a.prompt_tokens, a.completion_tokens) {
                    l.report_usage(p + c);
                }
                if let Some(b) = &self.backend {
                    if b.breaker.record_success() {
                        tracing::info!(event = "llm.breaker", state = "closed", backend = %b.name, "后端恢复正常");
                    }
                }
            }
            Err(e) => {
                self.emit(&info, e.kind.outcome(), total_ms, None, Some(e));
                if let (Some(b), ErrorKind::RateLimited) = (&self.backend, e.kind) {
                    // 429：整个后端暂停，别的请求也不要再打过去
                    b.sched.pause_for(e.retry_after.unwrap_or(Duration::from_secs(1)), "429");
                }
                if let (Some(b), true) = (&self.backend, e.kind.trips_breaker()) {
                    if b.breaker.record_failure() {
                        tracing::error!(event = "llm.breaker", code = codes::LLM_BREAKER, state = "open", backend = %b.name, last_outcome = e.kind.outcome(), "后端连续失败，熔断 10 秒");
                    }
                }
            }
        }
        result
    }

    /// 带退避的调用：连接失败 / 超时 / 429 / 5xx / 流中断才重试，最多 `max_transient` 次
    /// （前台最多 1 次、后台用 `max_retries`），每次重试前按指数退避 + 抖动等待，且受调用期限约束；
    /// 已经向用户推送了部分文本时不再重试（否则会重复输出）
    async fn run<F: FnMut(&str)>(&self, req: &CallReq<'_>, deadline: &Deadline, on_delta: &mut F) -> std::result::Result<Attempt, LlmError> {
        let max_transient = if self.priority == Priority::Interactive { self.cfg.max_retries.min(1) } else { self.cfg.max_retries };
        let emitted = AtomicBool::new(false);
        let mut n = 0u32;
        loop {
            let mut forward = |d: &str| {
                if req.streaming {
                    emitted.store(true, Ordering::Relaxed);
                }
                on_delta(d);
            };
            let e = match self.call_once(req, n, deadline, &mut forward).await {
                Ok(a) => return Ok(a),
                Err(e) => e,
            };
            if !e.is_retryable() || n >= max_transient || emitted.load(Ordering::Relaxed) {
                return Err(e);
            }
            n += 1;
            let delay = backoff_delay(n, jitter01(), e.retry_after);
            if deadline.remaining().is_some_and(|r| r < delay + Duration::from_secs(1)) {
                return Err(e);
            }
            tracing::warn!(event = "llm.retry", code = codes::LLM_RETRY, req_id = req.req_id, role = %self.role, attempt = n, reason = e.kind.outcome(), backoff_ms = delay.as_millis() as u64, "模型调用失败，退避后重试");
            tokio::time::sleep(delay).await;
        }
    }

    fn new_req<'a>(&self, messages: &'a [ChatMessage], schema: &'a str) -> CallReq<'a> {
        CallReq { messages, response_format: None, max_tokens: self.cfg.max_tokens, schema, req_id: next_req_id(), format_attempt: 1, streaming: false }
    }

    fn log_trim(&self, req_id: u64, r: &TrimReport, budget: u32, reason: &str) {
        tracing::warn!(event = "llm.context_trim", code = codes::LLM_CONTEXT_TRIM, req_id, role = %self.role, reason, budget, tokens_before = r.tokens_before, tokens_after = r.tokens_after, dropped_messages = r.dropped_messages, truncated_messages = r.truncated_messages, "提示词超出上下文预算，已裁剪（数据库里的原文不受影响）");
    }

    /// 按所属后端的上下文预算裁剪提示词；后端未知或上下文总量未知时原样返回。
    /// `reserve_output` 是额外预留的输出空间（结构化输出的格式重试会把上一轮输出追加回提示词）
    fn fit_for(&self, messages: &[ChatMessage], req_id: u64, reserve_output: u32) -> Vec<ChatMessage> {
        let Some(b) = &self.backend else { return messages.to_vec() };
        let ctx = b.context_tokens();
        if ctx == 0 {
            return messages.to_vec();
        }
        let max_output = self.cfg.max_tokens.unwrap_or(DEFAULT_OUTPUT_TOKENS).saturating_add(reserve_output);
        let budget = ContextBudget { context_tokens: ctx, concurrency: b.context_divisor(), max_output }.prompt_tokens();
        let (out, report) = fit(messages, budget);
        if report.changed() {
            self.log_trim(req_id, &report, budget, "budget");
        }
        out
    }

    /// 同 `run`；服务端仍报"超上下文"（估算偏小、或上下文总量未知）时，把提示词砍到约一半再试一次
    async fn run_fitted<F: FnMut(&str)>(&self, req: &CallReq<'_>, deadline: &Deadline, on_delta: &mut F) -> std::result::Result<Attempt, LlmError> {
        let err = match self.run(req, deadline, on_delta).await {
            Err(e) if e.kind == ErrorKind::ContextExceeded => e,
            other => return other,
        };
        let before = estimate_messages(req.messages);
        if before < MIN_SHRINK_TOKENS {
            return Err(err);
        }
        let (msgs, report) = fit(req.messages, before / 2);
        if !report.changed() {
            return Err(err);
        }
        self.log_trim(req.req_id, &report, before / 2, "ctx_exceeded_retry");
        let req2 = CallReq { messages: &msgs, response_format: req.response_format.clone(), max_tokens: req.max_tokens, schema: req.schema, req_id: req.req_id, format_attempt: req.format_attempt, streaming: req.streaming };
        self.run(&req2, deadline, on_delta).await
    }

    /// 普通文本对话
    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let deadline = Deadline::new(self.timeouts.deadline);
        // 输出被 max_tokens 截断时仍返回已有文本（回复被截短好过没有回复），只在日志里记 truncated
        let req = self.new_req(messages, "");
        let fitted = self.fit_for(messages, req.req_id, 0);
        let req = CallReq { messages: &fitted, ..req };
        let a = self.run_fitted(&req, &deadline, &mut |_: &str| {}).await?;
        Ok(strip_think(&a.text))
    }

    /// 流式对话：每收到一段**可见**文本（已过滤思考段落）就调用 `on_delta`，最后返回完整回答。
    /// 调用失败时返回错误，调用方可以退回到 [`chat`](Self::chat)。
    pub async fn chat_stream(&self, messages: &[ChatMessage], mut on_delta: impl FnMut(&str)) -> Result<String> {
        let deadline = Deadline::new(self.timeouts.deadline);
        let req = CallReq { streaming: true, ..self.new_req(messages, "") };
        let fitted = self.fit_for(messages, req.req_id, 0);
        let req = CallReq { messages: &fitted, ..req };
        let a = self.run_fitted(&req, &deadline, &mut on_delta).await?;
        if a.text.trim().is_empty() {
            return Err(LlmError::new(ErrorKind::Empty, "LLM 流式响应为空").into());
        }
        Ok(strip_think(&a.text))
    }

    /// 结构化输出：返回反序列化后的 `T`。
    ///
    /// - `schema_name`：schema 名字（也会出现在请求里，方便在 llama.cpp 日志里辨认）
    /// - `schema`：JSON Schema（建议手写成扁平结构，小模型更容易遵守）
    /// - `validate`：业务校验，返回 Err(说明) 时会把说明反馈给模型重试
    ///
    /// 重试规则：网络 / 超时 / 429 / 5xx 由 `run` 按退避重试（不占格式重试次数）；格式不合格才把原因反馈给模型再试；
    /// 超上下文、4xx 等重试没用的错误直接返回；输出被截断时不把半截内容反馈给模型，而是放大一次 `max_tokens` 重试。
    pub async fn chat_json<T, F>(
        &self,
        messages: &[ChatMessage],
        schema_name: &str,
        schema: &Value,
        validate: F,
    ) -> std::result::Result<Structured<T>, StructuredError>
    where
        T: DeserializeOwned,
        F: Fn(&T) -> std::result::Result<(), String>,
    {
        let started = Instant::now();
        let response_format = match self.cfg.structured_mode {
            StructuredMode::JsonSchema => Some(json!({
                "type": "json_schema",
                "json_schema": { "name": schema_name, "strict": true, "schema": schema }
            })),
            StructuredMode::JsonObject => Some(json!({ "type": "json_object", "schema": schema })),
            StructuredMode::PromptOnly => None,
        };

        // 在 system prompt 末尾追加格式说明（即使有语法约束，也让模型"知道"要输出什么）
        let mut msgs = messages.to_vec();
        let format_hint = format!(
            "\n\n## 输出格式\n只输出一个 JSON 对象，不要输出任何解释或 Markdown。JSON Schema：\n{}",
            serde_json::to_string(schema).unwrap_or_default()
        );
        match msgs.first_mut() {
            Some(first) if first.role == "system" => first.content.push_str(&format_hint),
            _ => msgs.insert(0, ChatMessage::system(format_hint)),
        }

        let mut raw_outputs = Vec::new();
        let mut last_err = String::new();
        let max_attempts = self.cfg.max_retries + 1;
        let deadline = Deadline::new(self.timeouts.deadline);
        let req_id = next_req_id();
        // 格式重试会把上一轮输出追加回提示词，所以多预留一份输出空间；之后追加的反馈不再重新裁剪
        let mut msgs = self.fit_for(&msgs, req_id, self.cfg.max_tokens.unwrap_or(DEFAULT_OUTPUT_TOKENS));
        let mut max_tokens = self.cfg.max_tokens;
        let mut escalated = false;
        let mut attempt = 0;
        let fail = |e: &LlmError, raw_outputs: Vec<String>, attempts: u32| {
            tracing::error!(event = "llm.structured_failed", code = e.kind.code(), req_id, schema = schema_name, model = %self.cfg.model, attempts, outcome = e.kind.outcome(), error = %e.message, "模型调用最终失败");
            StructuredError { message: e.to_string(), code: e.kind.code(), raw_outputs, attempts, latency_ms: started.elapsed().as_millis() as u64 }
        };

        while attempt < max_attempts {
            attempt += 1;
            let req = CallReq { messages: &msgs, response_format: response_format.clone(), max_tokens, schema: schema_name, req_id, format_attempt: attempt, streaming: false };
            let a = match self.run_fitted(&req, &deadline, &mut |_: &str| {}).await {
                Ok(a) => a,
                Err(e) => return Err(fail(&e, raw_outputs, attempt)),
            };
            if a.finish_reason.as_deref() == Some("length") {
                // 截断：半截 JSON 不当格式错误反馈给模型；放大一次 max_tokens 再试，仍不行就报"输出过长"
                if let (false, Some(mt)) = (escalated, max_tokens.filter(|m| *m < MAX_TOKENS_CAP)) {
                    escalated = true;
                    max_tokens = Some((mt * 2).min(MAX_TOKENS_CAP));
                    attempt -= 1;
                    tracing::warn!(event = "llm.truncated_retry", code = codes::LLM_TRUNCATED, req_id, schema = schema_name, from = mt, to = max_tokens, "输出被截断，放大 max_tokens 重试一次");
                    continue;
                }
                let e = LlmError::new(ErrorKind::Truncated, format!("输出被 max_tokens（{}）截断，放大后仍不够；请调大 [llm].max_tokens 或缩短要求输出的内容", max_tokens.map_or("未限制".to_string(), |m| m.to_string())));
                return Err(fail(&e, raw_outputs, attempt));
            }
            let raw = strip_think(&a.text);
            raw_outputs.push(raw.clone());

            // 解析 + 校验
            let parsed = extract_json(&raw)
                .ok_or_else(|| "没有找到 JSON 对象".to_string())
                .and_then(|j| serde_json::from_str::<T>(j).map_err(|e| format!("JSON 不符合格式要求: {e}")))
                .and_then(|v| validate(&v).map(|_| v));

            match parsed {
                Ok(value) => {
                    tracing::debug!(event = "llm.structured_ok", schema = schema_name, model = %self.cfg.model, attempts = attempt, elapsed_ms = started.elapsed().as_millis() as u64);
                    return Ok(Structured {
                        value,
                        raw_outputs,
                        attempts: attempt,
                        latency_ms: started.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    tracing::warn!(event = "llm.format_retry", code = codes::LLM_FORMAT_RETRY, schema = schema_name, model = %self.cfg.model, attempt, error = %e, raw = %truncate(&raw, 300), "结构化输出不合格，准备重试");
                    last_err = e.clone();
                    // 把模型的错误输出和错误原因反馈回去，让它修正
                    msgs.push(ChatMessage::assistant(raw));
                    msgs.push(ChatMessage::user(format!(
                        "上面的输出不合格：{e}。请严格按照要求的 JSON 格式重新输出，只输出 JSON。"
                    )));
                }
            }
        }

        tracing::error!(event = "llm.structured_failed", code = codes::LLM_FORMAT, schema = schema_name, model = %self.cfg.model, attempts = max_attempts, error = %last_err, "结构化输出最终失败");
        Err(StructuredError {
            message: format!("[{}] 结构化输出失败: {last_err}", codes::LLM_FORMAT),
            code: codes::LLM_FORMAT,
            raw_outputs,
            attempts: max_attempts,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// 健康检查：调用 `GET /models`。绑定了后端时结果缓存 8 秒，多个浏览器共用
    pub async fn list_models(&self) -> Result<Vec<String>> {
        match &self.backend {
            Some(b) => b.models_cached(self.fetch_models()).await.map_err(anyhow::Error::msg),
            None => self.fetch_models().await,
        }
    }

    async fn fetch_models(&self) -> Result<Vec<String>> {
        let mut req = self.http.get(self.endpoint("models")).timeout(Duration::from_secs(5));
        if !self.cfg.api_key.is_empty() {
            req = req.bearer_auth(&self.cfg.api_key);
        }
        let v: Value = req.send().await?.error_for_status()?.json().await?;
        Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
    }

    /// llama-server 的 `/slots` 和 `/metrics`（缓存 8 秒）。不是 `llama_cpp` 后端时返回 None
    pub async fn server_view(&self) -> Option<std::result::Result<ServerView, String>> {
        let b = self.backend.clone()?;
        if b.cfg.kind != BackendKind::LlamaCpp {
            return None;
        }
        let http = self.http.clone();
        let base = self.cfg.base_url.clone();
        Some(b.server_cached(async move { fetch_server(&http, &base).await }).await)
    }
}

/// 去掉 `/v1` 后的服务根地址（`/slots`、`/metrics`、`/props` 都在根上，不在 `/v1` 下）
fn service_root(base: &str) -> String {
    let b = base.trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b).to_string()
}

async fn fetch_server(http: &reqwest::Client, base: &str) -> std::result::Result<ServerView, String> {
    let root = service_root(base);
    let get = |path: String| http.get(path).timeout(Duration::from_secs(5));
    let slots_v: Value = get(format!("{root}/slots")).send().await.map_err(|e| format!("读取 /slots 失败：{e}"))?.error_for_status().map_err(|e| format!("/slots 返回错误：{e}"))?.json().await.map_err(|e| format!("/slots 不是 JSON：{e}"))?;
    let slots = parse_slots(&slots_v).ok_or_else(|| "/slots 的返回不是槽位数组".to_string())?;
    let text = get(format!("{root}/metrics")).send().await.map_err(|e| format!("读取 /metrics 失败：{e}"))?.error_for_status().map_err(|e| format!("/metrics 返回错误：{e}"))?.text().await.map_err(|e| format!("读取 /metrics 正文失败：{e}"))?;
    Ok(ServerView { slots, metrics: parse_metrics(&text) })
}

/// 去掉推理模型输出里的 `<think>…</think>` 段落（Qwen3、DeepSeek-R1 等会输出）
pub fn strip_think(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(start) = out.find("<think>") {
        match out[start..].find("</think>") {
            Some(end) => out.replace_range(start..start + end + "</think>".len(), ""),
            // 只有开头没有结尾：说明输出被截断，丢掉后面全部
            None => out.truncate(start),
        }
    }
    out.trim().to_string()
}

/// 流式输出过程中"当前可以展示给用户的文本"：
/// 去掉完整的思考段落；遇到未闭合的 `<think>` 时截断；末尾若是 `<think>` 的一部分（如 `<thi`）先不展示，等后续内容。
pub fn visible_text(full: &str) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut out = String::new();
    let mut rest = full;
    loop {
        match rest.find(OPEN) {
            Some(i) => {
                out.push_str(&rest[..i]);
                match rest[i..].find(CLOSE) {
                    Some(j) => rest = &rest[i + j + CLOSE.len()..],
                    None => return out.trim_start().to_string(),
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    for k in (1..OPEN.len()).rev() {
        if out.ends_with(&OPEN[..k]) {
            out.truncate(out.len() - k);
            break;
        }
    }
    out.trim_start().to_string()
}

/// 从模型输出中提取第一个完整的 JSON 对象（兼容 ```json 代码块和前后多余文字）。
/// 通过括号配对查找，并正确跳过字符串里的括号。
pub fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_blocks() {
        assert_eq!(strip_think("<think>嗯…</think>\n{\"a\":1}"), "{\"a\":1}");
        assert_eq!(strip_think("答案<think>没写完"), "答案");
    }

    #[test]
    fn visible_text_hides_thinking_while_streaming() {
        // 模拟流式到达的各个阶段
        assert_eq!(visible_text("<thi"), "");
        assert_eq!(visible_text("<think>想一想"), "");
        assert_eq!(visible_text("<think>想一想</think>\n答案"), "答案");
        assert_eq!(visible_text("答案是 1 <"), "答案是 1 ");
        assert_eq!(visible_text("a</think>b"), "a</think>b"); // 没有开头标签时原样输出
        assert_eq!(visible_text("前<think>x</think>后<think>未完"), "前后");
    }

    #[test]
    fn with_base_url_changes_endpoint_and_keeps_other_settings() {
        let cfg: LlmConfig = serde_json::from_str(r#"{"base_url":"http://127.0.0.1:8200/v1","model":"big"}"#).unwrap();
        let big = LlmClient::new(cfg).unwrap();
        let small = big.with_base_url("http://127.0.0.1:8201/v1/").with_model("small");
        assert_eq!(small.endpoint("/chat/completions"), "http://127.0.0.1:8201/v1/chat/completions");
        assert_eq!(small.model(), "small");
        assert_eq!(small.config().timeout_secs, big.config().timeout_secs);
        // 原客户端不受影响
        assert_eq!(big.endpoint("chat/completions"), "http://127.0.0.1:8200/v1/chat/completions");
    }

    #[test]
    fn extracts_json_from_noisy_output() {
        let s = "好的，结果如下：\n```json\n{\"a\": \"含 } 括号\", \"b\": {\"c\": 1}}\n```\n以上。";
        assert_eq!(extract_json(s).unwrap(), "{\"a\": \"含 } 括号\", \"b\": {\"c\": 1}}");
        assert!(extract_json("没有 json").is_none());
        assert!(extract_json("{\"未闭合\": 1").is_none());
    }
}
