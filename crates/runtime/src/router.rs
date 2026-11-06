//! Router：给定任务目标，选择执行角色、判断任务类别、所需工具和风险。
//!
//! 流程：Intent Classifier（LLM 结构化输出） → Policy（确定性规则） → 返回给编排层派发。
//! Router 自己不执行任何工具，只产出"经过校验的路由决策"。

use std::sync::Arc;

use an_core::{PolicyVerdict, Role, RouteDecision};
use an_llm::{ChatMessage, LlmClient, StructuredError};
use serde_json::{json, Value};

use crate::policy::Policy;
use crate::registry::AgentRegistry;

pub struct Router {
    llm: LlmClient,
    registry: Arc<AgentRegistry>,
    /// General Agent 可用的工具名（用于约束 required_tools）
    tool_names: Vec<String>,
    policy: Policy,
}

/// 路由成功的结果
pub struct RouteOutcome {
    pub decision: RouteDecision,
    pub policy: PolicyVerdict,
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
}

/// 路由失败：模型输出不合格，或 Policy 拒绝
pub struct RouteFailure {
    pub message: String,
    pub decision: Option<RouteDecision>,
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
}

impl Router {
    pub fn new(
        llm: LlmClient,
        registry: Arc<AgentRegistry>,
        tool_names: Vec<String>,
        policy: Policy,
    ) -> Self {
        Self {
            llm,
            registry,
            tool_names,
            policy,
        }
    }

    /// 路由用的 JSON Schema：角色和工具都用 enum 限定在注册表范围内
    fn schema(&self, roles: &[Role]) -> Value {
        let role_enum: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
        json!({
            "type": "object",
            "properties": {
                "task_class": { "type": "string", "enum": ["L1", "L2", "L3", "L4"] },
                "execution_role": { "type": "string", "enum": role_enum },
                "required_tools": { "type": "array", "items": { "type": "string", "enum": self.tool_names } },
                "risk_level": { "type": "string", "enum": ["low", "medium", "high"] },
                "reason": { "type": "string" }
            },
            "required": ["task_class", "execution_role", "required_tools", "risk_level", "reason"]
        })
    }

    fn system_prompt(
        &self,
        roles: &[Role],
        examples: &[crate::prompts::CorrectionExample],
    ) -> String {
        // 角色是"做什么"，同一角色可能有多个 agent，这里按角色描述，避免重复
        let agents = roles
            .iter()
            .map(|r| format!("- {}：{}", r.as_str(), r.description()))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "你是 AgentNexus 的任务路由器（Router）。你只负责分类和选择执行者，不负责完成任务。\n\n\
             ## 可选执行角色\n{agents}\n\n\
             ## General 可用工具\n{tools}\n\n\
             ## 任务类别（task_class）\n\
             - L1：不需要工具，也不需要生成文档\n\
             - L2：需要工具（查询、计算、读文件等），不需要生成文档\n\
             - L3：不需要获取数据的工具，但需要生成文档/报告（保存文档用的 write_file 不算\"需要工具\"）\n\
             - L4：既需要获取数据的工具（读文件、计算、查时间等），也需要生成文档/报告\n\n\
             ## 选择规则\n\
             1. 需要修改代码仓库、写程序、修 bug、跑测试，或生成网页（HTML/CSS/JS）、脚本、程序等代码产物（即使任务描述称之为文档或页面）→ coding；\n\
             2. 用户要求审查/评估/挑错对话中给出的内容 → reviewer；\n\
             \u{20}  审查对象是工作目录中的文件（需要先读取）→ general；审查代码仓库中的代码或改动 → coding；\n\
             3. 其他（问答、分析、计算、读写普通文件、写 Markdown 或纯文本的文档）→ general；\n\
             4. required_tools 只列 general 真正需要的工具，其他角色留空数组；\n\
             5. risk_level：只读操作为 low；会写文件或改代码为 medium；删除数据、对外发送、涉及生产环境为 high；\n\
             6. reason 用一句中文说明理由。{corrections}",
            tools = if self.tool_names.is_empty() { "无".into() } else { self.tool_names.join(", ") },
            corrections = crate::prompts::correction_section(examples),
        )
    }

    /// 执行路由：分类 + 规则校验。`examples` 为空时系统提示词与没有纠错时相同。
    pub async fn route(
        &self,
        task_goal: &str,
        context: &str,
        examples: &[crate::prompts::CorrectionExample],
    ) -> Result<RouteOutcome, RouteFailure> {
        // 始终让模型在全部角色中选择："该由谁做"只看任务本身。
        // 某角色当前没有可用的 agent 时由 Policy 明确报错（E_ROUTE_NO_AGENT），
        // 而不是让模型只能在可用角色里挑，把写代码的请求悄悄改派给别的角色。
        let roles = Role::ALL.to_vec();

        let user = if context.is_empty() {
            format!("任务目标：{task_goal}")
        } else {
            format!("任务目标：{task_goal}\n\n对话背景：\n{context}")
        };
        let messages = [
            ChatMessage::system(self.system_prompt(&roles, examples)),
            ChatMessage::user(user),
        ];
        let tool_names = self.tool_names.clone();

        // 业务校验：工具名必须存在（即使有语法约束，prompt_only 模式下也需要）
        let structured = self
            .llm
            .chat_json::<RouteDecision, _>(&messages, "route_decision", &self.schema(&roles), |d| {
                if let Some(bad) = d.required_tools.iter().find(|t| !tool_names.contains(t)) {
                    return Err(format!(
                        "工具 {bad} 不存在，可用工具：{}",
                        tool_names.join(", ")
                    ));
                }
                Ok(())
            })
            .await
            .map_err(|e: StructuredError| RouteFailure {
                message: e.message,
                decision: None,
                raw_outputs: e.raw_outputs,
                attempts: e.attempts,
                latency_ms: e.latency_ms,
            })?;

        let mut decision = structured.value;
        match self.policy.evaluate(&mut decision, &self.registry) {
            Ok(policy) => Ok(RouteOutcome {
                decision,
                policy,
                raw_outputs: structured.raw_outputs,
                attempts: structured.attempts,
                latency_ms: structured.latency_ms,
            }),
            Err(message) => Err(RouteFailure {
                message,
                decision: Some(decision),
                raw_outputs: structured.raw_outputs,
                attempts: structured.attempts,
                latency_ms: structured.latency_ms,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_core::Risk;
    use an_llm::LlmConfig;
    use std::collections::BTreeMap;

    fn router() -> Router {
        let llm = LlmClient::new(LlmConfig {
            base_url: "http://127.0.0.1:9/v1".into(),
            api_key: String::new(),
            model: "m".into(),
            temperature: 0.2,
            max_tokens: None,
            timeout_secs: 1,
            structured_mode: Default::default(),
            max_retries: 0,
        })
        .unwrap();
        Router::new(
            llm,
            AgentRegistry::new(BTreeMap::new()),
            vec!["write_file".into()],
            Policy {
                approval_risk: Risk::High,
            },
        )
    }

    #[test]
    fn prompt_routes_code_artifacts_to_coding() {
        let p = router().system_prompt(&Role::ALL, &[]);
        let coding_rule = p
            .lines()
            .find(|l| l.contains("→ coding") && l.contains("HTML"))
            .unwrap_or_default();
        assert!(
            coding_rule.contains("网页"),
            "网页 / HTML 等代码产物应交给 coding：{p}"
        );
        assert!(
            p.contains("Markdown 或纯文本"),
            "general 的文档应限定为 Markdown 或纯文本：{p}"
        );
        assert!(!p.contains("以往纠正"));
        let with = router().system_prompt(
            &Role::ALL,
            &[crate::prompts::CorrectionExample {
                user_text: "写一个网页".into(),
                correct_action: "delegate".into(),
                correct_role: Some("coding".into()),
                note: String::new(),
            }],
        );
        assert!(with.starts_with(&p));
        assert!(with.contains("delegate / coding"));
    }
}
