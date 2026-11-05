//! OpenAI 兼容的 Chat Completions 客户端。
//!
//! 针对本地小模型做了几件事：
//! 1. 结构化输出：通过 `response_format` 把 JSON Schema 交给推理服务做语法约束
//!    （llama.cpp 会把 schema 转成 GBNF 语法），同时在 prompt 里也写明格式；
//! 2. 容错解析：去掉 `<think>…</think>`、```json 代码块，提取第一个完整 JSON 对象；
//! 3. 校验 + 重试：解析失败或业务校验不通过时，把错误信息反馈给模型再试一次
//!    （借鉴 Pydantic AI 的"结构化输出 + 闭环重试"）。

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use an_core::codes;
use serde_json::{json, Value};

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
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .context("创建 HTTP 客户端失败")?;
        Ok(Self { http, cfg })
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

    /// 发送一次 chat completion 请求，返回 assistant 文本（已去掉 think 段落）
    async fn complete(&self, messages: &[ChatMessage], response_format: Option<Value>) -> Result<String> {
        let mut body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": self.cfg.temperature,
            "stream": false,
        });
        if let Some(mt) = self.cfg.max_tokens {
            body["max_tokens"] = json!(mt);
        }
        if let Some(rf) = response_format {
            body["response_format"] = rf;
        }

        let mut req = self.http.post(self.endpoint("chat/completions")).json(&body);
        if !self.cfg.api_key.is_empty() {
            req = req.bearer_auth(&self.cfg.api_key);
        }
        let resp = req.send().await.with_context(|| format!("无法连接 LLM 服务 {}", self.cfg.base_url))?;
        let status = resp.status();
        let text = resp.text().await.context("读取 LLM 响应失败")?;
        if !status.is_success() {
            bail!("LLM 返回 HTTP {status}: {}", truncate(&text, 500));
        }
        let v: Value = serde_json::from_str(&text).with_context(|| format!("LLM 响应不是 JSON: {}", truncate(&text, 300)))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("LLM 响应缺少 choices[0].message.content: {}", truncate(&text, 300)))?;
        Ok(strip_think(content))
    }

    /// 普通文本对话
    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        self.complete(messages, None).await
    }

    /// 流式对话：每收到一段**可见**文本（已过滤思考段落）就调用 `on_delta`，最后返回完整回答。
    /// 推理服务不支持流式时返回错误，调用方可以退回到 [`chat`](Self::chat)。
    pub async fn chat_stream(&self, messages: &[ChatMessage], mut on_delta: impl FnMut(&str)) -> Result<String> {
        use futures::StreamExt;
        let mut body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": self.cfg.temperature,
            "stream": true,
        });
        if let Some(mt) = self.cfg.max_tokens {
            body["max_tokens"] = json!(mt);
        }
        let mut req = self.http.post(self.endpoint("chat/completions")).json(&body);
        if !self.cfg.api_key.is_empty() {
            req = req.bearer_auth(&self.cfg.api_key);
        }
        let resp = req.send().await.with_context(|| format!("无法连接 LLM 服务 {}", self.cfg.base_url))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("LLM 返回 HTTP {status}: {}", truncate(&text, 500));
        }

        // SSE：每行 `data: {json}`，最后一行 `data: [DONE]`
        let (mut full, mut sent, mut buf) = (String::new(), String::new(), String::new());
        let mut stream = resp.bytes_stream();
        'outer: while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk.context("读取流式响应失败")?));
            while let Some(pos) = buf.find('\n') {
                let line: String = buf.drain(..=pos).collect();
                let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else { continue };
                if data == "[DONE]" {
                    break 'outer;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                    full.push_str(piece);
                    // 只推送新增的可见部分
                    let visible = visible_text(&full);
                    if visible.len() > sent.len() && visible.starts_with(&sent) {
                        on_delta(&visible[sent.len()..]);
                        sent = visible;
                    }
                }
            }
        }
        if full.trim().is_empty() {
            bail!("LLM 流式响应为空");
        }
        Ok(strip_think(&full))
    }

    /// 结构化输出：返回反序列化后的 `T`。
    ///
    /// - `schema_name`：schema 名字（也会出现在请求里，方便在 llama.cpp 日志里辨认）
    /// - `schema`：JSON Schema（建议手写成扁平结构，小模型更容易遵守）
    /// - `validate`：业务校验，返回 Err(说明) 时会把说明反馈给模型重试
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

        for attempt in 1..=max_attempts {
            let raw = match self.complete(&msgs, response_format.clone()).await {
                Ok(r) => r,
                Err(e) => {
                    // 网络/HTTP 错误：不反馈给模型，直接重试
                    last_err = format!("{e:#}");
                    let code = if last_err.contains("无法连接") { codes::LLM_UNREACHABLE } else { codes::LLM_HTTP };
                    tracing::warn!(event = "llm.request_failed", code, schema = schema_name, model = %self.cfg.model, attempt, error = %last_err, "LLM 请求失败");
                    continue;
                }
            };
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
                    })
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
            raw_outputs,
            attempts: max_attempts,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// 健康检查：调用 `GET /models`，返回服务端报告的模型列表
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let mut req = self.http.get(self.endpoint("models")).timeout(Duration::from_secs(5));
        if !self.cfg.api_key.is_empty() {
            req = req.bearer_auth(&self.cfg.api_key);
        }
        let v: Value = req.send().await?.error_for_status()?.json().await?;
        Ok(v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default())
    }
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
