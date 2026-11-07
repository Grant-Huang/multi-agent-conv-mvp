//! Response Composer：Conversation 的"交付"环节。
//! 把 Worker 结果、审查意见整理成给用户看的回复。
//! 要明确区分：Worker 完成 ≠ 验收通过 ≠ 用户目标达成。

use an_core::{truncate_chars, ReviewVerdict, TaskRecord, TaskResult, TaskStatus, WorkerStatus};
use an_llm::{ChatMessage, LlmClient};

/// 失败时附带的进程输出长度（取末尾：外部 agent 常把提示词回显在前面，真正的错误在最后）
const FAILURE_OUTPUT_CHARS: usize = 1500;

/// 失败任务的进程输出末尾：优先 stderr，没有时用 stdout
fn failure_output(r: &TaskResult) -> Option<String> {
    if r.status != WorkerStatus::Failed {
        return None;
    }
    let pick = |kind: &str| {
        r.evidence
            .iter()
            .find(|e| e.kind == kind && !e.content.trim().is_empty())
            .map(|e| e.content.trim())
    };
    let text = pick("stderr_tail").or_else(|| pick("stdout_tail"))?;
    let n = text.chars().count();
    Some(
        text.chars()
            .skip(n.saturating_sub(FAILURE_OUTPUT_CHARS))
            .collect(),
    )
}

pub struct Composer {
    llm: LlmClient,
}

impl Composer {
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// 生成交付消息；LLM 调用失败时退回到模板，保证用户一定能收到结果
    pub async fn compose(
        &self,
        task: &TaskRecord,
        status: TaskStatus,
        result: Option<&TaskResult>,
        review: Option<&ReviewVerdict>,
    ) -> String {
        let facts = Self::facts(task, status, result, review);
        let messages = [
            ChatMessage::system(
                "你是 AgentNexus 的会话助手，负责把后台任务的结果交付给用户。\n\
                 要求：\n\
                 1. 先用一句话说明结果（完成 / 未通过审查 / 失败 / 需要人工确认）；\n\
                 2. 再给出关键内容；有文件产物时列出文件名；\n\
                 3. 如实说明审查意见和遗留问题，不要夸大，不要编造事实材料里没有的内容；\n\
                 4. 失败时根据错误信息和进程输出说明直接原因；材料里看不出原因就说明原因未知，不要猜测；\n\
                 5. 简洁，使用中文。",
            ),
            ChatMessage::user(facts.clone()),
        ];
        match self.llm.chat(&messages).await {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) | Err(_) => Self::fallback(status, result, review),
        }
    }

    /// 交给模型的"事实材料"
    fn facts(
        task: &TaskRecord,
        status: TaskStatus,
        result: Option<&TaskResult>,
        review: Option<&ReviewVerdict>,
    ) -> String {
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
                    r.artifacts
                        .iter()
                        .map(|a| format!("[{}] {}", a.kind, a.name))
                        .collect::<Vec<_>>()
                        .join("，")
                );
            }
            if !r.errors.is_empty() {
                s += &format!("错误：{}\n", r.errors.join("；"));
            }
            if let Some(out) = failure_output(r) {
                s += &format!("进程输出末尾（判断失败原因用）：\n{out}\n");
            }
        }
        if let Some(v) = review {
            s += &format!("审查结论：{:?}\n审查评价：{}\n", v.verdict, v.comment);
            if !v.issues.is_empty() {
                s += &format!(
                    "审查发现的问题：\n{}\n",
                    v.issues
                        .iter()
                        .map(|i| format!("- {i}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }
        s
    }

    /// 不依赖模型的模板回复
    fn fallback(
        status: TaskStatus,
        result: Option<&TaskResult>,
        review: Option<&ReviewVerdict>,
    ) -> String {
        let head = match status {
            TaskStatus::Completed => "任务已完成。",
            TaskStatus::NeedsHuman => "任务需要人工确认。",
            TaskStatus::Cancelled => "任务已取消。",
            _ => "任务未能完成。",
        };
        let mut s = head.to_string();
        if let Some(r) = result {
            if !r.summary.trim().is_empty() {
                s += &format!("\n\n{}", r.summary);
            }
            if !r.errors.is_empty() && r.errors.iter().all(|e| *e != r.summary) {
                s += &format!("\n\n错误：{}", r.errors.join("；"));
            }
            if let Some(out) = failure_output(r) {
                s += &format!("\n\n进程输出末尾：\n{out}");
            }
        }
        if let Some(v) = review.filter(|v| !v.issues.is_empty()) {
            s += &format!(
                "\n\n审查意见：\n{}",
                v.issues
                    .iter()
                    .map(|i| format!("- {i}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_core::{Evidence, PolicyVerdict, Risk, Role, RouteDecision, TaskClass, WorkerStatus};

    fn task() -> TaskRecord {
        TaskRecord {
            id: "task-1".into(),
            conversation_id: "conv-1".into(),
            goal: "新建 fizzbuzz.py".into(),
            role: Role::Coding,
            status: TaskStatus::Failed,
            route: RouteDecision {
                task_class: TaskClass::L3,
                execution_role: Role::Coding,
                required_tools: vec![],
                risk_level: Risk::Medium,
                reason: "".into(),
            },
            policy: PolicyVerdict {
                auto_execute: true,
                needs_review: true,
                notes: vec![],
                candidates: vec![],
            },
            result: None,
            review: None,
            assignment: None,
            created_at: "".into(),
            updated_at: "".into(),
        }
    }

    /// 模拟 Codex 额度用完：stderr 前面是回显的长提示词，真正的错误在末尾
    fn failed_result() -> TaskResult {
        let echoed_prompt = "用户：很长的历史对话。".repeat(500);
        TaskResult {
            status: WorkerStatus::Failed,
            summary: "".into(),
            artifacts: vec![],
            evidence: vec![
                Evidence { kind: "exit_code".into(), content: "exit status: 1".into() },
                Evidence { kind: "stdout_tail".into(), content: "".into() },
                Evidence {
                    kind: "stderr_tail".into(),
                    content: format!("{echoed_prompt}\nERROR: stream disconnected before completion: upstream HTTP 429: 已达到使用上限"),
                },
            ],
            errors: vec!["[E_AGENT_RUNTIME] 进程退出：exit status: 1".into()],
        }
    }

    #[test]
    fn facts_include_failure_output_tail() {
        let r = failed_result();
        let facts = Composer::facts(&task(), TaskStatus::Failed, Some(&r), None);
        assert!(facts.contains("E_AGENT_RUNTIME"), "应包含错误码：{facts}");
        assert!(
            facts.contains("HTTP 429"),
            "应包含进程输出末尾的真实错误：{facts}"
        );
        assert!(
            facts.chars().count() < 4000,
            "只取输出末尾，不能把回显的长提示词整段塞进去"
        );
    }

    #[test]
    fn facts_skip_output_tail_when_succeeded() {
        let mut r = failed_result();
        r.status = WorkerStatus::Succeeded;
        r.errors.clear();
        let facts = Composer::facts(&task(), TaskStatus::Completed, Some(&r), None);
        assert!(!facts.contains("HTTP 429"), "成功的任务不需要附带进程输出");
    }

    #[test]
    fn fallback_reports_errors_and_output_tail() {
        let r = failed_result();
        let text = Composer::fallback(TaskStatus::Failed, Some(&r), None);
        assert!(text.contains("任务未能完成"));
        assert!(
            text.contains("E_AGENT_RUNTIME") && text.contains("HTTP 429"),
            "{text}"
        );
    }
}
