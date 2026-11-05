//! 上下文预算：按后端计算单个请求最多能放多少 token 的提示词，超出时按固定顺序裁剪。
//!
//! 裁剪顺序（保证"系统提示 + 最新一条消息"不丢）：
//! 1. 单条消息超过上限 → 保留头尾，中间换成"已截断 N 字"（数据库里的全文不受影响）；
//! 2. 仍然超出 → 从最旧的历史消息开始整条丢弃；
//! 3. 只剩系统提示和最新消息还超出 → 进一步缩短最新消息。

use crate::tokens::{estimate_messages, estimate_tokens};
use crate::ChatMessage;

/// 给估算误差留的余量
pub const MARGIN_TOKENS: u32 = 1024;
/// 预算再紧也至少给提示词这么多，免得算出负数或极小值
pub const MIN_PROMPT_TOKENS: u32 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// 上下文总量（llama.cpp 开了 `--kv-unified` 时是所有槽位共享的池子）
    pub context_tokens: u32,
    /// 预计同时占用这个上下文的请求数（共享池时 = 槽位数；每个请求独占上下文时 = 1）
    pub concurrency: u32,
    /// 本次请求预留给输出的 token 数
    pub max_output: u32,
}

impl ContextBudget {
    /// 单个请求的提示词 token 上限
    pub fn prompt_tokens(&self) -> u32 {
        let share = self.context_tokens / self.concurrency.max(1);
        share.saturating_sub(self.max_output.saturating_add(MARGIN_TOKENS)).max(MIN_PROMPT_TOKENS)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrimReport {
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub dropped_messages: usize,
    pub truncated_messages: usize,
}

impl TrimReport {
    pub fn changed(&self) -> bool {
        self.dropped_messages > 0 || self.truncated_messages > 0
    }
}

/// 把文本压到 `max_tokens` 以内：保留开头约 60% 和结尾约 40%，中间换成截断说明。返回（新文本，去掉的字数）
pub fn clip_middle(text: &str, max_tokens: u32) -> (String, usize) {
    let est = estimate_tokens(text);
    if est <= max_tokens {
        return (text.to_string(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // 按比例估计要保留多少字，再留 10% 余量；估算不准时逐步缩小直到放得下
    let mut keep = ((n as f64) * (max_tokens as f64 / est as f64) * 0.9) as usize;
    loop {
        let head_n = keep * 6 / 10;
        let tail_n = keep - head_n;
        let removed = n.saturating_sub(keep);
        let head: String = chars[..head_n.min(n)].iter().collect();
        let tail: String = chars[n.saturating_sub(tail_n)..].iter().collect();
        let out = format!("{head}\n…[中间已截断 {removed} 字]…\n{tail}");
        if estimate_tokens(&out) <= max_tokens || keep < 32 {
            return (out, removed);
        }
        keep = keep * 9 / 10;
    }
}

/// 把消息裁剪到 `budget` token 以内（估算值）。没超出时原样返回。
pub fn fit(messages: &[ChatMessage], budget: u32) -> (Vec<ChatMessage>, TrimReport) {
    let before = estimate_messages(messages);
    let mut report = TrimReport { tokens_before: before, tokens_after: before, ..Default::default() };
    if before <= budget || messages.is_empty() {
        return (messages.to_vec(), report);
    }
    let mut msgs = messages.to_vec();
    let first_history = usize::from(msgs[0].role == "system");
    let last = msgs.len() - 1;

    // 1. 单条过长的消息保留头尾（系统提示是代码生成的指令，不动）
    let (cap_history, cap_last) = ((budget / 4).max(512), (budget / 2).max(1024));
    let mut last_clipped = false;
    for (i, m) in msgs.iter_mut().enumerate().skip(first_history) {
        let (clipped, removed) = clip_middle(&m.content, if i == last { cap_last } else { cap_history });
        if removed > 0 {
            m.content = clipped;
            report.truncated_messages += 1;
            last_clipped |= i == last;
        }
    }

    // 2. 还超：从最旧的历史开始整条丢（不丢系统提示和最新消息）
    while estimate_messages(&msgs) > budget && msgs.len() > first_history + 1 {
        msgs.remove(first_history);
        report.dropped_messages += 1;
    }

    // 3. 只剩系统提示 + 最新消息还超：继续缩短最新消息
    if estimate_messages(&msgs) > budget && msgs.len() > first_history {
        let system_tokens = if first_history == 1 { estimate_tokens(&msgs[0].content) + 4 } else { 0 };
        let allowed = budget.saturating_sub(system_tokens + 8).max(256);
        let m = msgs.last_mut().expect("至少有一条消息");
        let (clipped, removed) = clip_middle(&m.content, allowed);
        if removed > 0 {
            // 第 1 步已经算过一次截断的不重复计数
            if !last_clipped {
                report.truncated_messages += 1;
            }
            m.content = clipped;
        }
    }
    report.tokens_after = estimate_messages(&msgs);
    (msgs, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(n: usize) -> String {
        "字".repeat(n)
    }

    #[test]
    fn prompt_budget_divides_shared_pool_and_reserves_output() {
        // 262144 的池子被 6 个槽位共享，输出预留 2048，余量 1024
        let b = ContextBudget { context_tokens: 262_144, concurrency: 6, max_output: 2048 };
        assert_eq!(b.prompt_tokens(), 262_144 / 6 - 2048 - 1024);
        // 每个请求独占上下文（云端）
        let c = ContextBudget { context_tokens: 128_000, concurrency: 1, max_output: 4096 };
        assert_eq!(c.prompt_tokens(), 128_000 - 4096 - 1024);
        // 预算极小也不会算出 0 或负数
        let tiny = ContextBudget { context_tokens: 4096, concurrency: 8, max_output: 4096 };
        assert_eq!(tiny.prompt_tokens(), MIN_PROMPT_TOKENS);
    }

    #[test]
    fn under_budget_is_untouched() {
        let msgs = vec![ChatMessage::system("系统"), ChatMessage::user("你好")];
        let (out, r) = fit(&msgs, 1000);
        assert_eq!(out.len(), 2);
        assert!(!r.changed());
        assert_eq!(r.tokens_before, r.tokens_after);
    }

    #[test]
    fn clip_middle_keeps_head_and_tail_and_reports_removed_chars() {
        let text = format!("开头{}结尾", long(5000));
        let (out, removed) = clip_middle(&text, 500);
        assert!(out.starts_with("开头") && out.ends_with("结尾"), "{out}");
        assert!(out.contains(&format!("中间已截断 {removed} 字")), "{out}");
        assert!(estimate_tokens(&out) <= 500, "{}", estimate_tokens(&out));
        assert!(removed > 4000);
        // 没超限：原样
        assert_eq!(clip_middle("短文本", 500), ("短文本".to_string(), 0));
    }

    #[test]
    fn oldest_history_is_dropped_first_and_system_and_latest_survive() {
        let mut msgs = vec![ChatMessage::system("系统提示")];
        for i in 0..10 {
            msgs.push(ChatMessage::user(format!("历史{i}{}", long(300))));
        }
        msgs.push(ChatMessage::user("最新问题"));
        let (out, r) = fit(&msgs, 1200);
        assert_eq!(out.first().unwrap().content, "系统提示");
        assert_eq!(out.last().unwrap().content, "最新问题");
        assert!(r.dropped_messages > 0 && r.tokens_after <= 1200, "{r:?}");
        // 留下来的是较新的历史，较旧的先被丢
        let kept: Vec<_> = out.iter().filter_map(|m| m.content.strip_prefix("历史").and_then(|s| s.chars().next())).collect();
        assert!(kept.contains(&'9') && !kept.contains(&'0'), "{kept:?}");
    }

    #[test]
    fn a_huge_paste_in_the_latest_message_keeps_head_and_tail() {
        let paste = format!("请审阅：{}完毕", long(100_000));
        let msgs = vec![ChatMessage::system("系统提示"), ChatMessage::user("之前聊的内容"), ChatMessage::user(paste)];
        let (out, r) = fit(&msgs, 4000);
        let last = &out.last().unwrap().content;
        assert!(last.starts_with("请审阅：") && last.ends_with("完毕") && last.contains("已截断"), "{last}");
        assert!(r.tokens_after <= 4000 && r.truncated_messages >= 1, "{r:?}");
        assert_eq!(out[0].content, "系统提示");
    }

    #[test]
    fn single_oversized_history_message_is_clipped_not_dropped_when_that_is_enough() {
        let msgs = vec![
            ChatMessage::system("系统"),
            ChatMessage::user(format!("A{}Z", long(20_000))),
            ChatMessage::assistant("好的"),
            ChatMessage::user("继续"),
        ];
        let (out, r) = fit(&msgs, 6000);
        assert_eq!(r.dropped_messages, 0, "{r:?}");
        assert_eq!(r.truncated_messages, 1);
        assert!(out[1].content.starts_with('A') && out[1].content.ends_with('Z'));
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn system_prompt_is_never_truncated_even_if_huge() {
        let msgs = vec![ChatMessage::system(long(3000)), ChatMessage::user(long(3000))];
        let (out, _) = fit(&msgs, 2000);
        assert_eq!(out[0].content.chars().count(), 3000);
        assert!(out[1].content.contains("已截断"));
    }
}
