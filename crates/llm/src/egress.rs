//! 外发检查的 LLM 侧接口（必经化方案第二阶段）。
//!
//! - [`LlmEgress`]：请求发往模型服务之前调用，返回脱敏后的消息和 Token → 原值映射；返回错误时本次调用不发出。
//!   具体实现（调用 dlp-egress）在 `an-agents::egress`，这里只定义接口，避免 an-llm 依赖 MCP。
//! - [`Restorer`]：把模型输出中的 Token 换回原值。流式输出中 Token 可能被切在两个数据块之间，
//!   所以遇到疑似 Token 开头的 `[` 先暂存，直到确认完整或确认不是 Token。
//! - [`TASK_ID`]：当前任务 ID（task-local）。General 在任务开始时设置，工具守卫和 LLM 检查都从这里读，
//!   不来自模型输出。
use std::{collections::HashMap, future::Future, pin::Pin};

use crate::{ChatMessage, LlmError};

tokio::task_local! {
    /// 当前任务 ID；不在任务内（如前台对话）时没有值
    pub static TASK_ID: String;
}

/// 读取当前任务 ID
pub fn current_task_id() -> Option<String> {
    TASK_ID.try_with(|t| t.clone()).ok()
}

/// 在指定任务 ID 下执行一段异步代码（`FuturesUnordered` 等同一任务内的并发也能读到）
pub async fn scope_task<F: Future>(task_id: String, fut: F) -> F::Output {
    TASK_ID.scope(task_id, fut).await
}

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 检查通过后实际要发出的内容
#[derive(Debug, Clone, Default)]
pub struct Outbound {
    /// 发给模型服务的消息（已脱敏）
    pub messages: Vec<ChatMessage>,
    /// Token → 原值，用于在本机恢复模型输出；为空表示不需要恢复
    pub restore: HashMap<String, String>,
}

pub trait LlmEgress: Send + Sync {
    /// 发出前检查。返回 `Err` 时调用方不得发送任何请求
    fn outbound<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> BoxFut<'a, Result<Outbound, LlmError>>;
}

/// Token 最长字节数：`[` + 类型名 + `_` + 编号 + `]`，超过这个长度的 `[...` 不可能是 Token
const MAX_TOKEN_BYTES: usize = 48;

/// 流式 Token 恢复器
pub struct Restorer {
    map: HashMap<String, String>,
    pending: String,
}

impl Restorer {
    pub fn new(map: HashMap<String, String>) -> Self {
        Self {
            map,
            pending: String::new(),
        }
    }

    /// 输入一段增量文本，返回可以立即输出的（已恢复）文本
    pub fn push(&mut self, delta: &str) -> String {
        self.pending.push_str(delta);
        // 最后一个 `[` 之后还没有 `]`，且长度仍可能是 Token：从这个 `[` 起暂存
        let hold_from = self.pending.rfind('[').filter(|&i| {
            !self.pending[i..].contains(']') && self.pending.len() - i <= MAX_TOKEN_BYTES
        });
        let ready = match hold_from {
            Some(i) => {
                let tail = self.pending.split_off(i);
                std::mem::replace(&mut self.pending, tail)
            }
            None => std::mem::take(&mut self.pending),
        };
        restore_all(&self.map, &ready)
    }

    /// 流结束：输出剩余暂存内容
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        restore_all(&self.map, &rest)
    }
}

/// 一次性恢复整段文本。只替换映射中存在的 Token，模型编造的 Token 原样保留
pub fn restore_all(map: &HashMap<String, String>, text: &str) -> String {
    if map.is_empty() || !text.contains('[') {
        return text.to_string();
    }
    let mut out = text.to_string();
    // 长的先换，避免 [X_0010] 被 [X_001] 的前缀误伤（Token 带右括号，实际不会，但保持稳妥）
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for k in keys {
        out = out.replace(k.as_str(), &map[k]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> HashMap<String, String> {
        [
            ("[CUSTOMER_001]".to_string(), "松岩液压".to_string()),
            ("[ORDER_ID_001]".to_string(), "PO-771203".to_string()),
        ]
        .into()
    }

    #[test]
    fn restores_tokens_split_across_deltas() {
        let mut r = Restorer::new(map());
        let mut out = String::new();
        for d in [
            "客户[CUS",
            "TOMER_00",
            "1]的单号[ORDER_ID_001",
            "]，[注意]",
            "结束[CUSTOMER_009]",
        ] {
            out.push_str(&r.push(d));
        }
        out.push_str(&r.finish());
        assert_eq!(out, "客户松岩液压的单号PO-771203，[注意]结束[CUSTOMER_009]");
    }

    #[test]
    fn long_bracket_text_is_not_held_forever() {
        let mut r = Restorer::new(map());
        let long = format!("[{}", "x".repeat(60));
        assert_eq!(r.push(&long), long, "超过 Token 最大长度的内容应立即输出");
        assert_eq!(r.finish(), "");
    }

    #[test]
    fn empty_map_is_passthrough() {
        let mut r = Restorer::new(HashMap::new());
        assert_eq!(r.push("[CUSTOMER_001"), "");
        assert_eq!(r.finish(), "[CUSTOMER_001");
        assert_eq!(restore_all(&HashMap::new(), "[A_001]"), "[A_001]");
    }

    #[tokio::test]
    async fn task_id_is_scoped() {
        assert_eq!(current_task_id(), None);
        let inner = scope_task("t-1".into(), async { current_task_id() }).await;
        assert_eq!(inner.as_deref(), Some("t-1"));
    }
}
