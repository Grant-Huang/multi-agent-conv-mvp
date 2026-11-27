//! 外部 agent 的 HTTP 适配器（E3：agent 准入实验与运行时接入）。
//!
//! 外部 agent 以 HTTP 服务的形式运行，适配器向 `POST {endpoint}/run` 发送任务：
//!
//! ```text
//! 请求 {"task_id", "conversation_id", "goal", "context", "risk": "low|medium|high"}
//! 响应 {"status": "succeeded|failed", "summary": "...", "errors": ["..."]}
//! ```
//!
//! - 没有子进程；超时由 HTTP 客户端控制（编排器另有任务总超时），任务被取消时请求随 future 一起丢弃。
//! - 外部 agent 的输出视为**不可信**：结果附一条"未经核实"的证据，摘要和错误长度有上限。
//! - 是否允许它领任务、结果是否被接受，由准入账本决定（见 `an_runtime::admission`），不在这里判断。

use std::sync::Arc;
use std::time::Duration;

use an_core::{
    codes, AgentAdapter, AgentCard, AgentError, EventSink, Evidence, Role, TaskResult, TaskSpec,
    WorkerStatus,
};
use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

/// 与 `an_runtime::admission::A2A_KIND` 一致
pub const KIND: &str = "a2a";
/// 摘要最多保留的字符数
const MAX_SUMMARY: usize = 20_000;
/// 单条错误最多保留的字符数，最多保留的条数
const MAX_ERROR: usize = 500;
const MAX_ERRORS: usize = 10;

/// 构造适配器所需的信息（来自准入记录中的接入信息）
pub struct A2aSpec {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub agent_type: String,
    /// 外部 agent 服务地址，例如 `http://127.0.0.1:9100`
    pub endpoint: String,
    pub max_concurrent: u32,
    /// 单次请求超时（含连接）
    pub timeout: Duration,
}

pub struct A2aHttpAgent {
    card: AgentCard,
    url: String,
    http: reqwest::Client,
    /// 出站外发检查守卫（#360 防线二）：任务上下文发给外部 agent 前评估；
    /// `[egress]` off（默认）时为 None，行为与以前完全一致
    egress: Option<Arc<crate::egress::EgressGuard>>,
}

#[derive(Deserialize)]
struct RunResponse {
    status: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    errors: Vec<String>,
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

impl A2aHttpAgent {
    pub fn new(spec: A2aSpec) -> Result<Self> {
        let endpoint = spec.endpoint.trim_end_matches('/').to_string();
        anyhow::ensure!(
            endpoint.starts_with("http://") || endpoint.starts_with("https://"),
            "外部 agent 地址必须以 http:// 或 https:// 开头"
        );
        anyhow::ensure!(spec.max_concurrent >= 1, "max_concurrent 至少为 1");
        let http = reqwest::Client::builder().timeout(spec.timeout).build()?;
        Ok(Self {
            card: AgentCard {
                id: spec.id,
                kind: KIND.into(),
                agent_type: spec.agent_type,
                location: endpoint.clone(),
                max_concurrent: spec.max_concurrent,
                role: spec.role,
                name: spec.name,
                description: "运行时接入的外部 agent（经准入协议）".into(),
                tools: vec![],
                enabled: true,
                disabled_reason: None,
            },
            url: format!("{endpoint}/run"),
            http,
            egress: None,
        })
    }

    /// 设置出站外发检查守卫（Runtime 装配时传入；与 tools/LLM 同一守卫实例）
    pub fn set_egress_guard(&mut self, guard: Arc<crate::egress::EgressGuard>) {
        self.egress = Some(guard);
    }

    /// 出站前处理任务文本（#360 防线二）。守卫在场时 goal 与 context 分别评估：
    /// 全部通过 → 发脱敏文本；任一未通过/不可用 → enforce 拒发（任务失败），audit 放行原文并记录。
    /// 守卫不在场（off）→ 原文照发。分开评估是为了两个字段各自拿到自己的脱敏文本，
    /// 不把 goal 混进 context（外部协议两个字段语义不同）。
    async fn outbound_texts(&self, task: &TaskSpec) -> Result<(String, String)> {
        let Some(guard) = &self.egress else {
            return Ok((task.goal.clone(), task.context.clone()));
        };
        let dest = self.card.location.as_str();
        let goal_ev = guard
            .evaluate_text(&task.task_id, "a2a", dest, "delegation", &task.goal)
            .await;
        let ctx_ev = guard
            .evaluate_text(&task.task_id, "a2a", dest, "delegation", &task.context)
            .await;
        let audit = guard.mode() == crate::egress::EgressMode::Audit;
        let mut goal = task.goal.clone();
        let mut context = task.context.clone();
        let mut verdicts: Vec<String> = vec![];
        let mut failures: Vec<String> = vec![];
        for (name, ev) in [("goal", goal_ev), ("context", ctx_ev)] {
            match ev {
                Ok(ev) if ev.ready_to_send => {
                    if let Some(sanitized) = ev.sanitized_text {
                        match name {
                            "goal" => goal = sanitized,
                            _ => context = sanitized,
                        }
                    }
                }
                Ok(ev) => verdicts.push(format!("{name}={}", ev.decision)),
                Err(e) => failures.push(format!(
                    "{name}: {}",
                    an_core::truncate_chars(&format!("{e}"), 120)
                )),
            }
        }
        if verdicts.is_empty() && failures.is_empty() {
            tracing::info!(
                event = "agent.a2a_egress_pass",
                task_id = %task.task_id,
                agent_id = %self.card.id,
                "外部委派内容已过外发检查"
            );
            return Ok((goal, context));
        }
        if audit {
            tracing::warn!(
                event = "agent.a2a_egress_audit_allow",
                task_id = %task.task_id,
                agent_id = %self.card.id,
                verdicts = ?verdicts,
                failures = ?failures,
                "audit 模式：外发检查未通过/不可用，放行原文（仅记录）"
            );
            return Ok((task.goal.clone(), task.context.clone()));
        }
        let code = if failures.is_empty() {
            let review = verdicts.iter().any(|v| v.contains("REVIEW"));
            if review {
                an_core::codes::EGRESS_REVIEW
            } else {
                an_core::codes::EGRESS_BLOCKED
            }
        } else {
            an_core::codes::EGRESS_UNAVAILABLE
        };
        Err(AgentError::new(
            code,
            format!(
                "外发检查未通过（{verdicts:?}{failures:?}），已停止向外部 agent {} 委派",
                self.card.id
            ),
        )
        .into())
    }
}

#[async_trait::async_trait]
impl AgentAdapter for A2aHttpAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        events.progress(format!("发送到外部 agent {}", self.card.name));
        // 出站外发检查（#360 防线二）：通过只发脱敏文本；未通过 enforce 拒发、audit 放行原文
        let (goal, context) = self.outbound_texts(&task).await?;
        let risk = serde_json::to_value(task.route.risk_level)?;
        let body = json!({
            "task_id": task.task_id,
            "conversation_id": task.conversation_id,
            "goal": goal,
            "context": context,
            "risk": risk,
        });
        let resp = match self.http.post(&self.url).json(&body).send().await {
            Ok(r) => r,
            Err(e) if e.is_timeout() => {
                return Err(AgentError::new(
                    codes::AGENT_TIMEOUT,
                    format!("外部 agent {} 超时：{e}", self.card.id),
                )
                .into())
            }
            Err(e) => {
                return Err(AgentError::new(
                    codes::AGENT_RUNTIME,
                    format!("连不上外部 agent {}：{e}", self.card.id),
                )
                .into())
            }
        };
        if !resp.status().is_success() {
            return Err(AgentError::new(
                codes::AGENT_RUNTIME,
                format!("外部 agent {} 返回 HTTP {}", self.card.id, resp.status()),
            )
            .into());
        }
        let parsed: RunResponse = match resp.json().await {
            Ok(v) => v,
            Err(e) if e.is_timeout() => {
                return Err(AgentError::new(
                    codes::AGENT_TIMEOUT,
                    format!("外部 agent {} 超时：{e}", self.card.id),
                )
                .into())
            }
            Err(e) => {
                return Err(AgentError::new(
                    codes::AGENT_RUNTIME,
                    format!("外部 agent {} 的响应不是合法 JSON：{e}", self.card.id),
                )
                .into())
            }
        };
        let status = match parsed.status.as_str() {
            "succeeded" => WorkerStatus::Succeeded,
            "failed" => WorkerStatus::Failed,
            other => {
                return Err(AgentError::new(
                    codes::AGENT_RUNTIME,
                    format!(
                        "外部 agent {} 返回未知状态 {}",
                        self.card.id,
                        clip(other, 40)
                    ),
                )
                .into())
            }
        };
        Ok(TaskResult {
            status,
            summary: clip(&parsed.summary, MAX_SUMMARY),
            artifacts: vec![],
            evidence: vec![Evidence {
                kind: "untrusted_source".into(),
                content: format!("外部 agent {} 的输出，未经核实", self.card.id),
            }],
            errors: parsed
                .errors
                .iter()
                .take(MAX_ERRORS)
                .map(|e| clip(e, MAX_ERROR))
                .collect(),
        })
    }
}
