//! Response Composer：Conversation 的"交付"环节。
//! 把 Worker 结果、审查意见整理成给用户看的回复。
//! 要明确区分：Worker 完成 ≠ 验收通过 ≠ 用户目标达成。

use an_core::{truncate_chars, ReviewVerdict, TaskRecord, TaskResult, TaskStatus};
use an_llm::{ChatMessage, LlmClient};

pub struct Composer {
    llm: LlmClient,
}

impl Composer {
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// 生成交付消息；LLM 调用失败时退回到模板，保证用户一定能收到结果
    pub async fn compose(&self, task: &TaskRecord, status: TaskStatus, result: Option<&TaskResult>, review: Option<&ReviewVerdict>) -> String {
        let facts = Self::facts(task, status, result, review);
        let messages = [
            ChatMessage::system(
                "你是 AgentNexus 的会话助手，负责把后台任务的结果交付给用户。\n\
                 要求：\n\
                 1. 先用一句话说明结果（完成 / 未通过审查 / 失败 / 需要人工确认）；\n\
                 2. 再给出关键内容；有文件产物时列出文件名；\n\
                 3. 如实说明审查意见和遗留问题，不要夸大，不要编造事实材料里没有的内容；\n\
                 4. 简洁，使用中文。",
            ),
            ChatMessage::user(facts.clone()),
        ];
        match self.llm.chat(&messages).await {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) | Err(_) => Self::fallback(status, result, review),
        }
    }

    /// 交给模型的"事实材料"
    fn facts(task: &TaskRecord, status: TaskStatus, result: Option<&TaskResult>, review: Option<&ReviewVerdict>) -> String {
        let mut s = format!(
            "任务目标：{}\n执行角色：{}\n最终状态：{}\n",
            task.goal,
            task.role.as_str(),
            status.as_str()
        );
        if let Some(r) = result {
            s += &format!("执行结果摘要：\n{}\n", truncate_chars(&r.summary, 4000));
            if !r.artifacts.is_empty() {
                s += &format!(
                    "产物：{}\n",
                    r.artifacts.iter().map(|a| format!("[{}] {}", a.kind, a.name)).collect::<Vec<_>>().join("，")
                );
            }
            if !r.errors.is_empty() {
                s += &format!("错误：{}\n", r.errors.join("；"));
            }
        }
        if let Some(v) = review {
            s += &format!("审查结论：{:?}\n审查评价：{}\n", v.verdict, v.comment);
            if !v.issues.is_empty() {
                s += &format!("审查发现的问题：\n{}\n", v.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"));
            }
        }
        s
    }

    /// 不依赖模型的模板回复
    fn fallback(status: TaskStatus, result: Option<&TaskResult>, review: Option<&ReviewVerdict>) -> String {
        let head = match status {
            TaskStatus::Completed => "任务已完成。",
            TaskStatus::NeedsHuman => "任务需要人工确认。",
            TaskStatus::Cancelled => "任务已取消。",
            _ => "任务未能完成。",
        };
        let mut s = head.to_string();
        if let Some(r) = result {
            s += &format!("\n\n{}", r.summary);
        }
        if let Some(v) = review.filter(|v| !v.issues.is_empty()) {
            s += &format!("\n\n审查意见：\n{}", v.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"));
        }
        s
    }
}
