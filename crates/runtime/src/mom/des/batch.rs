//! 阶段④：无界面批量实验（设计文档 §10 与 §8.4 修订说明）。
//!
//! 同一组 seed 分别以 AI-Native 与传统 MOM 策略运行。两种策略都按推荐方案决策，差别只在决策延迟：
//! AI-Native 为一次人工审批延迟；传统 MOM 为若干次部门间交接延迟之和。延迟参数是建模假设，可配置。
//!
//! **当前状态：接口骨架**，行为尚未实现（RT-DES-15 应为 Red）。

use super::{FactoryConfig, Pattern};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 决策策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPolicy {
    /// 统一上下文 + 一次人工审批。
    AiNative,
    /// 计划、维修、质量部门依次交接后才决策。
    Traditional,
}

/// 模拟审批与交接的延迟（分钟，对数正态分布的均值；建模假设）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApproverPolicy {
    pub approval_delay_mean_minutes: f64,
    pub handoff_delay_mean_minutes: f64,
    /// 传统策略的部门间交接次数。
    pub handoffs: u32,
}

impl Default for ApproverPolicy {
    fn default() -> Self {
        Self {
            approval_delay_mean_minutes: 5.0,
            handoff_delay_mean_minutes: 25.0,
            handoffs: 3,
        }
    }
}

/// 批量实验配置。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    pub factory: Option<FactoryConfig>,
    pub pattern: Pattern,
    pub horizon_minutes: u64,
    pub seeds: Vec<u64>,
    pub policies: Vec<DecisionPolicy>,
    pub approver: ApproverPolicy,
}

/// 一个 seed、一种策略的结果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchRow {
    pub seed: u64,
    pub policy: DecisionPolicy,
    pub cases: u32,
    pub alarms: u32,
    pub decisions: u32,
    /// 从案例进入人工决策到执行的平均分钟数；没有决策时为 None。
    pub mean_decision_minutes: Option<f64>,
    pub risk_parts: f64,
    pub on_time: u32,
    pub late: u32,
    pub verified_cases: u32,
    pub rule_failures: u32,
}

/// 运行批量实验。
///
/// 骨架：尚未实现。
pub fn run_batch(_config: &BatchConfig) -> Result<Vec<BatchRow>> {
    Ok(vec![])
}

/// 导出 CSV（含表头）。
///
/// 骨架：尚未实现。
pub fn to_csv(_rows: &[BatchRow]) -> String {
    String::new()
}
