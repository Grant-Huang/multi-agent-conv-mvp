//! Router：给定任务目标，选择执行角色、判断任务类别、所需工具和风险。
//!
//! 流程：Intent Classifier（LLM 结构化输出） → Policy（确定性规则） → 返回给编排层派发。
//! Router 自己不执行任何工具，只产出"经过校验的路由决策"。

use std::sync::Arc;
use std::time::Instant;

use an_core::{PolicyVerdict, Risk, Role, RouteDecision};
use an_llm::{ChatMessage, LlmClient, Structured, StructuredError};
use serde_json::{json, Value};

use crate::policy::Policy;
use crate::registry::AgentRegistry;

pub struct Router {
    llm: LlmClient,
    registry: Arc<AgentRegistry>,
    /// General Agent 可用的工具名（用于约束 required_tools）
    tool_names: Vec<String>,
    policy: Policy,
    /// FT 实验 B：按规则把风险只升不降
    risk_rules: bool,
}

/// 判断依据字段的最大长度（FT 实验 A）
pub const REASON_MAX_CHARS: u64 = 40;

/// 任务描述命中这些词时风险至少为 high（删除数据、对外发送、部署 / 生产环境）。统一按小写比较。
const HIGH_RISK_WORDS: &[&str] = &[
    "删除",
    "删掉",
    "清空",
    "drop table",
    "rm -rf",
    "truncate",
    "对外发送",
    "发送邮件",
    "发邮件",
    "发给客户",
    "群发",
    "发布到",
    "部署",
    "上线",
    "生产环境",
    "deploy",
    "production",
];

/// 命中这些词时风险至少为 medium（会写文件或改代码）
const MEDIUM_RISK_WORDS: &[&str] = &[
    "修改",
    "改成",
    "改一下",
    "重构",
    "修复",
    "写入",
    "保存",
    "写到文件",
    "写文件",
    "创建文件",
    "新建文件",
    "生成文件",
    "提交代码",
    "git commit",
    "refactor",
];

/// FT 实验 B：按任务描述把风险只升不降。返回规则是否提升了风险。
pub fn apply_risk_rules(task_goal: &str, decision: &mut RouteDecision) -> bool {
    let text = task_goal.to_lowercase();
    let floor = if HIGH_RISK_WORDS.iter().any(|w| text.contains(w)) {
        Risk::High
    } else if MEDIUM_RISK_WORDS.iter().any(|w| text.contains(w)) {
        Risk::Medium
    } else {
        return false;
    };
    if decision.risk_level < floor {
        decision.risk_level = floor;
        true
    } else {
        false
    }
}

/// 路由成功的结果
pub struct RouteOutcome {
    pub decision: RouteDecision,
    pub policy: PolicyVerdict,
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
    /// 经过开思考重判时的触发原因（FT 实验 D）
    pub escalated: Option<String>,
}

/// 路由失败：模型输出不合格，或 Policy 拒绝
pub struct RouteFailure {
    pub message: String,
    pub decision: Option<RouteDecision>,
    pub raw_outputs: Vec<String>,
    pub attempts: u32,
    pub latency_ms: u64,
    /// 经过开思考重判时的触发原因（FT 实验 D）
    pub escalated: Option<String>,
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
            risk_rules: false,
        }
    }

    /// FT 实验 B：路由后按规则把风险只升不降
    pub fn with_risk_rules(mut self, on: bool) -> Self {
        self.risk_rules = on;
        self
    }

    /// 路由用的 JSON Schema：角色和工具都用 enum 限定在注册表范围内
    fn schema(&self, roles: &[Role]) -> Value {
        let role_enum: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
        // FT 实验 A：不思考时让模型先写判断依据（llama.cpp 按属性顺序生成，所以 reason 放第一个）
        if self.llm.front_options().reason_first {
            return json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string", "maxLength": REASON_MAX_CHARS },
                    "task_class": { "type": "string", "enum": ["L1", "L2", "L3", "L4"] },
                    "execution_role": { "type": "string", "enum": role_enum },
                    "required_tools": { "type": "array", "items": { "type": "string", "enum": self.tool_names } },
                    "risk_level": { "type": "string", "enum": ["low", "medium", "high"] }
                },
                "required": ["reason", "task_class", "execution_role", "required_tools", "risk_level"]
            });
        }
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
             {web}\
             6. reason 用一句中文说明理由。{corrections}",
            tools = if self.tool_names.is_empty() { "无".into() } else { self.tool_names.join(", ") },
            web = if self.tool_names.iter().any(|t| t == "web_search") {
                "5a. 任务需要最新事实、新闻、行情、选举、政策等外部信息，或要求引用公开资料 → 不是 L3：生成文档时为 L4，required_tools 必须包含 web_search（需要打开具体网页时加 web_fetch）；\n"
            } else {
                ""
            },
            corrections = crate::prompts::correction_section(examples),
        )
    }

    /// FT 实验 D：不思考的路由结果是否可疑（需要开思考重判）。返回触发原因。
    fn suspicious(&self, task_goal: &str, d: &RouteDecision) -> Option<&'static str> {
        let mut d = d.clone();
        if self.risk_rules {
            apply_risk_rules(task_goal, &mut d);
        }
        if d.risk_level == Risk::High {
            return Some("high_risk");
        }
        if !self.registry.is_enabled(d.execution_role) {
            return Some("no_agent");
        }
        None
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
        let schema = self.schema(&roles);

        // 业务校验：工具名必须存在（即使有语法约束，prompt_only 模式下也需要）
        let validate = |d: &RouteDecision| {
            if let Some(bad) = d.required_tools.iter().find(|t| !tool_names.contains(t)) {
                return Err(format!(
                    "工具 {bad} 不存在，可用工具：{}",
                    tool_names.join(", ")
                ));
            }
            Ok(())
        };
        let to_failure = |e: StructuredError| RouteFailure {
            message: e.message,
            decision: None,
            raw_outputs: e.raw_outputs,
            attempts: e.attempts,
            latency_ms: e.latency_ms,
            escalated: None,
        };

        let escalation = self.llm.front_options().thinking_escalation;
        let started = Instant::now();
        // FT 实验 D：开了升级时，不思考的第一次只试一次，不合格直接交给思考重判
        let fast = if escalation {
            self.llm.with_max_retries(0)
        } else {
            self.llm.clone()
        };
        let first = fast
            .chat_json::<RouteDecision, _>(&messages, "route_decision", &schema, &validate)
            .await;
        let trigger = match &first {
            _ if !escalation => None,
            Err(e) if e.code == an_core::codes::LLM_FORMAT => Some("format"),
            Err(_) => None,
            Ok(s) => self.suspicious(task_goal, &s.value),
        };
        let structured = match (first, trigger) {
            (first, None) => first.map_err(to_failure)?,
            (first, Some(reason)) => {
                let (fast_ms, mut raw_outputs, fast_attempts) = match &first {
                    Ok(s) => (s.latency_ms, s.raw_outputs.clone(), s.attempts),
                    Err(e) => (e.latency_ms, e.raw_outputs.clone(), e.attempts),
                };
                let think = self
                    .llm
                    .with_thinking()
                    .chat_json::<RouteDecision, _>(&messages, "route_decision", &schema, &validate)
                    .await;
                let total_ms = started.elapsed().as_millis() as u64;
                tracing::info!(
                    event = "llm.escalated",
                    schema = "route_decision",
                    reason,
                    fast_ms,
                    total_ms,
                    ok = think.is_ok(),
                    "前台判断可疑，已开思考重判"
                );
                match think {
                    Ok(s) => {
                        raw_outputs.extend(s.raw_outputs);
                        Structured {
                            value: s.value,
                            raw_outputs,
                            attempts: fast_attempts + s.attempts,
                            latency_ms: total_ms,
                            escalated: Some(reason.to_string()),
                        }
                    }
                    Err(mut e) => {
                        raw_outputs.extend(e.raw_outputs);
                        e.raw_outputs = raw_outputs;
                        e.attempts += fast_attempts;
                        e.latency_ms = total_ms;
                        return Err(to_failure(e));
                    }
                }
            }
        };

        let mut decision = structured.value;
        if self.risk_rules && apply_risk_rules(task_goal, &mut decision) {
            tracing::info!(event = "route.risk_raised", risk = ?decision.risk_level, "规则提升了风险等级");
        }
        match self.policy.evaluate(&mut decision, &self.registry) {
            Ok(policy) => Ok(RouteOutcome {
                decision,
                policy,
                raw_outputs: structured.raw_outputs,
                attempts: structured.attempts,
                latency_ms: structured.latency_ms,
                escalated: structured.escalated,
            }),
            Err(message) => Err(RouteFailure {
                message,
                decision: Some(decision),
                raw_outputs: structured.raw_outputs,
                attempts: structured.attempts,
                latency_ms: structured.latency_ms,
                escalated: structured.escalated,
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
        router_with(&["write_file"])
    }

    fn router_with(tools: &[&str]) -> Router {
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
            tools.iter().map(|t| t.to_string()).collect(),
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

    #[test]
    fn prompt_sends_reports_needing_external_facts_to_web_search() {
        let p =
            router_with(&["web_search", "web_fetch", "write_file"]).system_prompt(&Role::ALL, &[]);
        let rule = p
            .lines()
            .find(|l| l.contains("引用公开资料"))
            .unwrap_or_default();
        assert!(rule.contains("L4"), "需要外部事实的报告必须是 L4：{p}");
        assert!(
            rule.contains("web_search"),
            "需要外部事实时必须带 web_search：{p}"
        );
        assert!(
            !router()
                .system_prompt(&Role::ALL, &[])
                .contains("引用公开资料"),
            "没有联网工具时不能要求 web_search"
        );
    }
}
