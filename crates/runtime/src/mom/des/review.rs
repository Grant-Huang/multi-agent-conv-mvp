//! 阶段④：审核层（设计文档 §8 与 §8.4 修订说明）。
//!
//! 三步：确定性规则检查 → AI 审核（`Reviewer` 接口，测试用脚本替身）→ 人工确认或推翻。
//! 审核只评价，不改变案例与仿真状态；审核记录与标准答案都不进入 agent 视图。
//!
//! **当前状态：接口骨架**，行为尚未实现（RT-DES-12～14、19 与 HTTP-DES-04 应为 Red）。

use crate::config::AppConfig;
use crate::mom::Engine;
use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

use super::GroundTruth;

/// 规则编号。
pub const RULE_HITL_BEFORE_WRITE: &str = "hitl_before_write";
pub const RULE_ROOT_CAUSE: &str = "root_cause_attribution";
pub const RULE_PLAN_SAFE: &str = "plan_safe";
pub const RULE_EFFECTIVENESS: &str = "effectiveness";
pub const RULE_TIMELINESS: &str = "timeliness";

/// 规则结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Pass,
    Fail,
    Warn,
    Pending,
    NotApplicable,
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleResult {
    pub rule: String,
    pub status: RuleStatus,
    /// 说明；失败时写明原因（例如 `misattributed`）。
    pub detail: String,
    /// 支撑本结果的证据编号（`event:<序号>`、`truth:<事件编号>` 等）。
    pub evidence_refs: Vec<String>,
}

/// 案例时间线中的一条看板事件。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub seq: u64,
    pub kind: String,
    pub at: u64,
}

/// 动作摘要。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionSummary {
    pub tool: String,
    pub state: String,
    pub accepted_at: u64,
    pub complete_at: u64,
}

/// 规则检查的输入：一个 DES 案例的处理记录（可由引擎导出，也可人为构造）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub case_id: String,
    pub simulation_id: String,
    pub asset: String,
    /// 导出时的虚拟时间。
    pub now: u64,
    pub timeline: Vec<TimelineEntry>,
    /// 本案例收到的报警事件编号（带仿真编号前缀，与标准答案一致）。
    pub alarm_event_ids: Vec<String>,
    /// 记录的根因假设，例如 `spindle_wear:CNC-XX`。
    pub claimed_causes: Vec<String>,
    /// 已执行的方案（排产方案中的一项），未执行时为 None。
    pub executed_plan: Option<Value>,
    pub actions: Vec<ActionSummary>,
    /// 报警设备当前的可观测状态（振动、Cpk、运行状态）。
    pub asset_state: Option<Value>,
}

/// 规则检查（纯函数）。
///
/// 骨架：尚未实现。
pub fn check_rules(_record: &ReviewRecord, _truth: &GroundTruth) -> Vec<RuleResult> {
    vec![]
}

/// 审核模型的选择结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewerChoice {
    /// 使用的模型名；None 表示只做规则检查。
    pub model: Option<String>,
    /// 选择来源：`mom_reviewer` / `llm_default` / `rules_only`。
    pub source: String,
    /// 审核模型是否不同于 MOM 主 agent 使用的模型。
    pub independent: bool,
    /// 非独立审核时的标注。
    pub label: Option<String>,
}

/// 按 `[models].mom_reviewer` → `[llm].model` 的顺序选择审核模型。
///
/// 骨架：尚未实现。
pub fn resolve_reviewer(_config: &AppConfig) -> ReviewerChoice {
    ReviewerChoice {
        model: None,
        source: "unimplemented".into(),
        independent: false,
        label: None,
    }
}

/// AI 审核接口（与 `Master` 同一写法）。输入为 `Engine::des_ai_review_input` 的结果，
/// 输出须为 `{"conclusions":[{"judgement","statement","evidence_refs":[…]}],"summary"}`。
#[async_trait]
pub trait Reviewer: Send + Sync {
    fn choice(&self) -> ReviewerChoice;
    async fn review(&self, input: Value) -> Result<Value>;
}

/// 测试与演示用的脚本审核替身：等待 `delay` 后返回固定结果。
pub struct ScriptedReviewer {
    pub choice: ReviewerChoice,
    pub response: std::result::Result<Value, String>,
    pub delay: Duration,
}

#[async_trait]
impl Reviewer for ScriptedReviewer {
    fn choice(&self) -> ReviewerChoice {
        self.choice.clone()
    }
    async fn review(&self, _input: Value) -> Result<Value> {
        tokio::time::sleep(self.delay).await;
        match &self.response {
            Ok(v) => Ok(v.clone()),
            Err(e) => bail!("{e}"),
        }
    }
}

/// 在引擎锁外调用审核模型（有超时），把结果或降级原因记入审核记录，返回审核报告。
///
/// 骨架：尚未实现。
pub async fn run_ai_review(
    _engine: &Mutex<Engine>,
    _case_id: &str,
    _reviewer: &dyn Reviewer,
    _timeout: Duration,
) -> Result<Value> {
    bail!("AI review is not implemented yet")
}

impl Engine {
    /// 记录对本案例报警的根因假设（agent 或人）。
    ///
    /// 骨架：尚未实现。
    pub fn record_cause_claim(
        &mut self,
        _case_id: &str,
        _causes: Vec<String>,
        _actor: &str,
    ) -> Result<()> {
        bail!("cause claims are not implemented yet")
    }

    /// 导出 DES 案例的处理记录，供规则检查使用。
    ///
    /// 骨架：尚未实现。
    pub fn des_review_record(&self, _case_id: &str) -> Result<ReviewRecord> {
        bail!("review records are not implemented yet")
    }

    /// 审核报告：规则检查结果、AI 审核记录、人工结论与分歧记录。
    ///
    /// 骨架：尚未实现。
    pub fn des_review_report(&self, _case_id: &str) -> Result<Value> {
        bail!("review reports are not implemented yet")
    }

    /// AI 审核的输入：处理记录、规则结果、本案例的标准答案，以及可引用的证据编号列表。
    ///
    /// 骨架：尚未实现。
    pub fn des_ai_review_input(&self, _case_id: &str) -> Result<Value> {
        bail!("AI review input is not implemented yet")
    }

    /// 人工确认（`agree`）或推翻（`override`，必须写理由）AI 审核结论。
    ///
    /// 骨架：尚未实现。
    pub fn des_human_review(
        &mut self,
        _case_id: &str,
        _decision: &str,
        _reason: &str,
        _actor: &str,
    ) -> Result<()> {
        bail!("human review is not implemented yet")
    }
}
