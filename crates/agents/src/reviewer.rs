//! Reviewer Agent：只读的独立审查。
//!
//! 两种用法：
//! 1. 作为编排层的验收环节：`review(goal, result)` 审查其他 Worker 的结果；
//! 2. 作为普通 Agent 被 Router 选中：用户直接要求"帮我审查这段内容"。
//!
//! 它只给出"评估意见"，任务最终状态由编排层按规则决定。

use an_core::{
    truncate_chars, AgentAdapter, AgentCard, EventSink, Evidence, ReviewVerdict, Role, TaskResult,
    TaskSpec, Verdict, WorkerStatus,
};
use an_llm::{ChatMessage, LlmClient};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

pub struct ReviewerAgent {
    llm: LlmClient,
    card: AgentCard,
}

/// 模型输出格式
#[derive(Debug, Deserialize)]
struct RawVerdict {
    verdict: Verdict,
    #[serde(default)]
    issues: Vec<String>,
    #[serde(default)]
    comment: String,
}

fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "verdict": { "type": "string", "enum": ["pass", "fail", "needs_human"] },
            "issues": { "type": "array", "items": { "type": "string" } },
            "comment": { "type": "string" }
        },
        "required": ["verdict", "issues", "comment"]
    })
}

impl ReviewerAgent {
    pub fn new(llm: LlmClient) -> Self {
        let card = AgentCard {
            id: "reviewer".into(),
            kind: "builtin".into(),
            agent_type: "reviewer".into(),
            location: "本机".into(),
            max_concurrent: 4,
            role: Role::Reviewer,
            name: "Reviewer Agent".into(),
            description: Role::Reviewer.description().into(),
            tools: vec![],
            enabled: true,
            disabled_reason: None,
        };
        Self { llm, card }
    }

    async fn judge(&self, system: &str, user: String) -> Result<ReviewVerdict> {
        let messages = [ChatMessage::system(system), ChatMessage::user(user)];
        let out = self
            .llm
            .chat_json::<RawVerdict, _>(&messages, "review_verdict", &verdict_schema(), |v| {
                // fail 时必须给出问题，方便返工
                if v.verdict == Verdict::Fail && v.issues.is_empty() {
                    Err("verdict=fail 时 issues 不能为空".into())
                } else {
                    Ok(())
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(ReviewVerdict {
            verdict: out.value.verdict,
            issues: out.value.issues,
            comment: out.value.comment,
        })
    }

    /// 验收环节：审查 Worker 的执行结果是否满足任务目标
    pub async fn review(&self, goal: &str, result: &TaskResult) -> Result<ReviewVerdict> {
        let system = "你是 AgentNexus 的独立审查员（Reviewer）。你只能阅读，不能修改任何内容。\n\
                      根据任务目标，审查执行者提交的结果、产物和证据：\n\
                      - pass：结果满足目标，没有明显错误；\n\
                      - fail：结果不满足目标或有明显错误，必须在 issues 中逐条写出要修正的问题；\n\
                      - needs_human：涉及你无法判断的事项（例如业务决策、缺少关键证据），需要人工确认。\n\
                      不要因为风格偏好判 fail。证据不足以证明结果正确时，倾向 needs_human 而不是 pass。";
        let artifacts = result
            .artifacts
            .iter()
            .map(|a| {
                format!(
                    "### [{}] {}\n{}",
                    a.kind,
                    a.name,
                    truncate_chars(&a.content, 4000)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let evidence = result
            .evidence
            .iter()
            .map(|e| format!("- [{}] {}", e.kind, truncate_chars(&e.content, 800)))
            .collect::<Vec<_>>()
            .join("\n");
        let user = format!(
            "## 任务目标\n{goal}\n\n## 执行者的结果摘要\n{}\n\n## 产物\n{}\n\n## 证据\n{}",
            result.summary,
            if artifacts.is_empty() {
                "无".into()
            } else {
                artifacts
            },
            if evidence.is_empty() {
                "无".into()
            } else {
                evidence
            },
        );
        self.judge(system, user).await
    }
}

#[async_trait::async_trait]
impl AgentAdapter for ReviewerAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    /// 用户直接请求审查时：审查对象就在任务目标和上下文里
    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        events.progress("Reviewer 正在审查");
        let system = "你是 AgentNexus 的独立审查员（Reviewer）。审查用户提交的内容，\n\
                      verdict 表示内容是否可以接受（pass/fail/needs_human），issues 逐条列出问题，\n\
                      comment 写一段总体评价和修改建议。";
        let user = format!(
            "## 审查要求\n{}\n\n## 相关上下文\n{}",
            task.goal, task.context
        );
        match self.judge(system, user).await {
            Ok(v) => {
                let summary = format!(
                    "审查结论：{:?}\n{}\n{}",
                    v.verdict,
                    v.comment,
                    v.issues
                        .iter()
                        .map(|i| format!("- {i}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                Ok(TaskResult {
                    status: WorkerStatus::Succeeded,
                    summary,
                    artifacts: vec![],
                    evidence: vec![Evidence {
                        kind: "review_verdict".into(),
                        content: serde_json::to_string(&v)?,
                    }],
                    errors: vec![],
                })
            }
            Err(e) => Ok(TaskResult::failed_code(
                an_core::codes::REVIEW_FAILED,
                format!("审查失败：{e:#}"),
            )),
        }
    }
}
