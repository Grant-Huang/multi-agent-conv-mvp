//! 模型调用错误：分类决定"要不要重试、怎么退避、算不算后端故障"。

use std::time::Duration;

use an_core::codes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// 连接失败 / 连接超时
    Unreachable,
    /// 等第一个 token 超时（排队或提示词预填充太久）
    TimeoutFirstToken,
    /// 生成过程中 token 间隔超时（卡住）
    TimeoutIdle,
    /// 单次请求总时长超限
    TimeoutTotal,
    /// 400 且原因是超出上下文
    ContextExceeded,
    /// 429
    RateLimited,
    /// 其他 4xx（请求本身有问题，重试没用）
    Http4xx,
    Http5xx,
    /// 流在 `[DONE]` / finish_reason 之前就断了
    StreamCut,
    /// 输出被 max_tokens 截断
    Truncated,
    /// 模型没有返回内容
    Empty,
    /// 后端熔断中，没有发请求
    BreakerOpen,
    /// 排队等不到空闲槽位 / 并发许可（调度器已经等过了，不再重试）
    Busy,
}

impl ErrorKind {
    /// 日志里 `outcome` 字段的取值
    pub fn outcome(self) -> &'static str {
        match self {
            ErrorKind::Unreachable => "unreachable",
            ErrorKind::TimeoutFirstToken => "timeout_first_token",
            ErrorKind::TimeoutIdle => "timeout_idle",
            ErrorKind::TimeoutTotal => "timeout_total",
            ErrorKind::ContextExceeded => "ctx_exceeded",
            ErrorKind::RateLimited => "rate_limited",
            ErrorKind::Http4xx => "http_4xx",
            ErrorKind::Http5xx => "http_5xx",
            ErrorKind::StreamCut => "stream_cut",
            ErrorKind::Truncated => "truncated",
            ErrorKind::Empty => "empty",
            ErrorKind::BreakerOpen => "breaker_open",
            ErrorKind::Busy => "busy",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Unreachable => codes::LLM_UNREACHABLE,
            ErrorKind::TimeoutFirstToken | ErrorKind::TimeoutIdle | ErrorKind::TimeoutTotal => codes::LLM_TIMEOUT,
            ErrorKind::ContextExceeded => codes::LLM_CONTEXT,
            ErrorKind::RateLimited => codes::LLM_RATE_LIMITED,
            ErrorKind::Http4xx | ErrorKind::Http5xx => codes::LLM_HTTP,
            ErrorKind::StreamCut => codes::LLM_STREAM,
            ErrorKind::Truncated => codes::LLM_TRUNCATED,
            ErrorKind::Empty => codes::LLM_FORMAT,
            ErrorKind::BreakerOpen => codes::LLM_BREAKER,
            ErrorKind::Busy => codes::LLM_BUSY,
        }
    }

    /// 能不能原样再发一次（要经过退避，而且受总期限限制）
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::Unreachable
                | ErrorKind::TimeoutFirstToken
                | ErrorKind::TimeoutIdle
                | ErrorKind::TimeoutTotal
                | ErrorKind::RateLimited
                | ErrorKind::Http5xx
                | ErrorKind::StreamCut
        )
    }

    /// 算不算"后端出了故障"（连续出现会触发熔断）；429、4xx、截断都不算
    pub fn trips_breaker(self) -> bool {
        matches!(
            self,
            ErrorKind::Unreachable | ErrorKind::TimeoutFirstToken | ErrorKind::TimeoutIdle | ErrorKind::TimeoutTotal | ErrorKind::Http5xx | ErrorKind::StreamCut
        )
    }
}

#[derive(Debug, Clone)]
pub struct LlmError {
    pub kind: ErrorKind,
    pub message: String,
    pub status: Option<u16>,
    /// 服务端要求的等待时间（429 的 `Retry-After`）
    pub retry_after: Option<Duration>,
}

impl LlmError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), status: None, retry_after: None }
    }

    /// 根据 HTTP 状态码和响应内容分类
    pub fn from_http(status: u16, body: &str, retry_after: Option<Duration>) -> Self {
        let kind = match status {
            429 => ErrorKind::RateLimited,
            400 | 413 if is_context_error(body) => ErrorKind::ContextExceeded,
            500..=599 => ErrorKind::Http5xx,
            _ => ErrorKind::Http4xx,
        };
        let shown: String = body.chars().take(500).collect();
        Self { kind, message: format!("LLM 返回 HTTP {status}: {shown}"), status: Some(status), retry_after }
    }

    pub fn is_retryable(&self) -> bool {
        self.kind.retryable()
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind.code(), self.message)
    }
}

impl std::error::Error for LlmError {}

/// 响应内容是不是"超出上下文"（llama.cpp 的 `exceed_context_size_error`，OpenAI 的 `context_length_exceeded` 等）
pub fn is_context_error(body: &str) -> bool {
    let b = body.to_lowercase();
    ["exceed_context_size", "context_length_exceeded", "context size", "context length", "maximum context", "too long", "n_ctx"].iter().any(|k| b.contains(k))
}

/// 解析 `Retry-After`（只支持秒数写法；最长按 60 秒算）
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(|s| Duration::from_secs(s.min(60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_classification() {
        assert_eq!(LlmError::from_http(429, "slow down", None).kind, ErrorKind::RateLimited);
        assert_eq!(LlmError::from_http(503, "busy", None).kind, ErrorKind::Http5xx);
        assert_eq!(LlmError::from_http(401, "bad key", None).kind, ErrorKind::Http4xx);
        // 400 但不是上下文问题：不重试
        let e = LlmError::from_http(400, "{\"error\":\"unknown field response_format\"}", None);
        assert_eq!(e.kind, ErrorKind::Http4xx);
        assert!(!e.is_retryable());
    }

    #[test]
    fn context_errors_from_llama_cpp_and_openai_are_detected() {
        let llama = r#"{"error":{"code":400,"message":"the request exceeds the available context size, try increasing it","type":"exceed_context_size_error","n_prompt_tokens":300000,"n_ctx":262144}}"#;
        let e = LlmError::from_http(400, llama, None);
        assert_eq!(e.kind, ErrorKind::ContextExceeded);
        assert_eq!(e.kind.code(), codes::LLM_CONTEXT);
        assert!(!e.is_retryable());
        let openai = r#"{"error":{"code":"context_length_exceeded","message":"This model's maximum context length is 8192 tokens"}}"#;
        assert_eq!(LlmError::from_http(400, openai, None).kind, ErrorKind::ContextExceeded);
    }

    #[test]
    fn retry_and_breaker_rules() {
        for k in [ErrorKind::Unreachable, ErrorKind::TimeoutFirstToken, ErrorKind::TimeoutIdle, ErrorKind::Http5xx, ErrorKind::StreamCut] {
            assert!(k.retryable() && k.trips_breaker(), "{k:?}");
        }
        // 429 要重试，但不是后端故障
        assert!(ErrorKind::RateLimited.retryable() && !ErrorKind::RateLimited.trips_breaker());
        for k in [ErrorKind::ContextExceeded, ErrorKind::Http4xx, ErrorKind::Truncated, ErrorKind::Empty, ErrorKind::BreakerOpen, ErrorKind::Busy] {
            assert!(!k.retryable() && !k.trips_breaker(), "{k:?}");
        }
    }

    #[test]
    fn display_carries_code_and_message_without_losing_downcast() {
        let e = LlmError::new(ErrorKind::TimeoutFirstToken, "等了 120 秒没有收到第一个 token");
        assert_eq!(e.to_string(), "[E_LLM_TIMEOUT] 等了 120 秒没有收到第一个 token");
        let any: anyhow::Error = e.into();
        assert_eq!(any.downcast_ref::<LlmError>().unwrap().kind, ErrorKind::TimeoutFirstToken);
    }

    #[test]
    fn retry_after_is_parsed_and_capped() {
        assert_eq!(parse_retry_after("3"), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after(" 500 "), Some(Duration::from_secs(60)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }
}
