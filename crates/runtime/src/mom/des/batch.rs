//! 阶段④：无界面批量实验（设计文档 §10 与 §8.4 修订说明）。
//!
//! 同一组 seed 分别以 AI-Native 与传统 MOM 策略运行。两种策略都按推荐方案决策，差别只在决策延迟：
//! AI-Native 为一次人工审批延迟；传统 MOM 为若干次部门间交接延迟之和。延迟参数是建模假设，可配置。
//!
//! 每个 seed、每种策略在独立的内存引擎中运行，互不影响；按 seed 分配到多个线程并行，
//! 结果按（seed，策略）排序，与线程数无关，可复现。
//!
//! 运行范围：从 S1 起点（08:17，第 497 分钟）推进到场景结束；还有待执行的决策时继续推进，
//! 直到决策全部执行或超过上限（场景结束后 1440 分钟）。维修通常在这之后才完成，
//! 所以 `verified_cases` 只统计运行结束时动作已全部核验的案例。

use super::review::{check_rules, RuleStatus};
use super::rng::Rng;
use super::{DesSimulationConfig, FactoryConfig, Pattern, ScenarioConfig};
use crate::mom::Engine;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// 决策到期时证据已过期、刷新计划后已没有可行方案的次数（决策延误的代价，计入 `late`）。
    pub expired_decisions: u32,
}

/// S1 夹具的起点（08:17）。
const START_MINUTE: u64 = 497;
/// 场景结束后为等待决策继续推进的上限（分钟）。
const DRAIN_LIMIT_MINUTES: u64 = 1440;
/// 对数正态延迟的形状参数（建模假设）。
const DELAY_SIGMA: f64 = 0.5;
const MAX_SEEDS: usize = 10_000;
const MAX_HORIZON_MINUTES: u64 = 7 * 24 * 60;
/// 模拟审批人：引擎只接受夹具审批人 `supervisor`（尚无企业认证），批量实验沿用它。
const APPROVER: &str = "supervisor";

impl DecisionPolicy {
    fn label(self) -> &'static str {
        match self {
            Self::AiNative => "ai_native",
            Self::Traditional => "traditional",
        }
    }
}

/// 均值为 `mean` 的对数正态分布，取整到分钟，至少 1 分钟。
fn lognormal_minutes(rng: &mut Rng, mean: f64) -> u64 {
    let mean = mean.max(0.0);
    if mean == 0.0 {
        return 1;
    }
    let mu = mean.ln() - DELAY_SIGMA * DELAY_SIGMA / 2.0;
    ((mu + DELAY_SIGMA * rng.normal()).exp().ceil() as u64).max(1)
}

/// 某策略下一次决策的延迟（分钟）。
fn decision_delay(rng: &mut Rng, policy: DecisionPolicy, approver: &ApproverPolicy) -> u64 {
    let handoffs = match policy {
        DecisionPolicy::AiNative => 0,
        DecisionPolicy::Traditional => approver.handoffs,
    };
    let mut delay = lognormal_minutes(rng, approver.approval_delay_mean_minutes);
    for _ in 0..handoffs {
        delay += lognormal_minutes(rng, approver.handoff_delay_mean_minutes);
    }
    delay
}

/// 一次待执行的决策：案例、方案修订号、请求时刻、到期时刻、推荐方案。
struct Pending {
    revision: u64,
    requested_at: u64,
    due_at: u64,
    plan: String,
}

/// 运行一个 seed、一种策略。
fn run_one(config: &BatchConfig, seed: u64, policy: DecisionPolicy) -> Result<BatchRow> {
    let mut e = Engine::memory()?;
    let sim = e.create_des_simulation(DesSimulationConfig {
        name: format!("batch-{seed}-{}", policy.label()),
        factory: config.factory.clone(),
        scenario: ScenarioConfig {
            seed,
            pattern: config.pattern.clone(),
            start_minute: START_MINUTE,
            horizon_minutes: config.horizon_minutes,
            mechanisms: None,
        },
    })?;
    // 延迟的随机流与场景的随机流相互独立；两种策略各用一条。
    let mut rng = Rng::stream(seed, 0, &format!("batch:decision:{}", policy.label()));
    // 场景从 S1 起点开始；引擎时钟可能更早，先推进到起点。
    let start = START_MINUTE.max(e.now());
    let end = start + config.horizon_minutes;
    let mut pending: BTreeMap<String, Pending> = BTreeMap::new();
    // 决策到期时证据已过有效期的案例：刷新计划后立即按新的推荐方案决策（审议已经完成），
    // 等待时间仍从最初的决策请求算起。值为最初的请求时刻。
    let mut refreshed: BTreeMap<String, u64> = BTreeMap::new();
    let mut waits: Vec<u64> = vec![];
    let (mut risk_parts, mut on_time, mut late) = (0.0, 0, 0);
    let mut blocked_after_refresh = 0u32;
    let mut t = start;
    loop {
        e.advance(t)?;
        // 新的决策请求：等待人工、有推荐方案、尚未排上（或方案已更新）。
        for id in e.case_ids() {
            let c = e.case(&id)?;
            let recommended = c
                .proposal
                .as_ref()
                .and_then(|p| p["recommended"].as_str())
                .map(String::from);
            match (c.stage.as_str(), recommended) {
                ("WAITING_HUMAN", Some(plan)) => {
                    if pending
                        .get(&id)
                        .is_none_or(|p| p.revision != c.plan_revision)
                    {
                        let (requested_at, due_at) = match refreshed.remove(&id) {
                            Some(first) => (first, t),
                            None => (t, t + decision_delay(&mut rng, policy, &config.approver)),
                        };
                        pending.insert(
                            id.clone(),
                            Pending {
                                revision: c.plan_revision,
                                requested_at,
                                due_at,
                                plan,
                            },
                        );
                    }
                }
                (stage, _) => {
                    pending.remove(&id);
                    // 刷新后没有可行方案（例如交期已赶不上）：这次决策作废，计为延误。
                    if stage == "BLOCKED" && c.proposal.is_some() && refreshed.remove(&id).is_some()
                    {
                        late += 1;
                        blocked_after_refresh += 1;
                    }
                }
            }
        }
        // 到期的决策：按推荐方案选定、批准、执行。
        let due: Vec<String> = pending
            .iter()
            .filter(|(_, p)| p.due_at <= t)
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let p = pending.remove(&id).context("pending decision")?;
            let c = e.case(&id)?;
            let ttl = crate::mom::fixture_policy(&c.scenario).snapshot_ttl_minutes;
            if t > c.resource_observed_at + ttl {
                // 证据过期：刷新计划（引擎限制刷新次数，超过时放弃这次决策）。
                if e.refresh_plan(&id).is_ok() {
                    refreshed.insert(id, p.requested_at);
                }
                continue;
            }
            e.select_des_plan(&id, &p.plan)?;
            let revision = e.case(&id)?.plan_revision;
            e.approve(&id, revision, "production", APPROVER)?;
            e.execute(&id, "production")?;
            waits.push(t - p.requested_at);
            let chosen = e
                .case(&id)?
                .proposal
                .as_ref()
                .and_then(|pr| pr["plans"].as_array())
                .and_then(|plans| plans.iter().find(|x| x["id"] == p.plan.as_str()).cloned())
                .unwrap_or_default();
            risk_parts += chosen["risk_parts"].as_f64().unwrap_or(0.0);
            if chosen["on_time"] == true {
                on_time += 1;
            } else {
                late += 1;
            }
        }
        if t >= end
            && ((pending.is_empty() && refreshed.is_empty()) || t >= end + DRAIN_LIMIT_MINUTES)
        {
            break;
        }
        t += 1;
    }

    let truth: super::GroundTruth = serde_json::from_value(e.des_review_truth(&sim)?)?;
    let mut cases = 0;
    let mut verified_cases = 0;
    let mut rule_failures = 0;
    for id in e.case_ids() {
        let c = e.case(&id)?;
        if !c.batch.starts_with(&format!("{sim}:")) {
            continue;
        }
        cases += 1;
        if !c.actions.is_empty() && c.actions.values().all(|a| a.state == "VERIFIED") {
            verified_cases += 1;
        }
        let record = e.des_review_record(&id)?;
        rule_failures += check_rules(&record, &truth)
            .iter()
            .filter(|r| r.status == RuleStatus::Fail)
            .count() as u32;
    }
    Ok(BatchRow {
        seed,
        policy,
        cases,
        alarms: truth.alarms.len() as u32,
        decisions: waits.len() as u32,
        mean_decision_minutes: (!waits.is_empty())
            .then(|| waits.iter().sum::<u64>() as f64 / waits.len() as f64),
        risk_parts: (risk_parts * 10.0).round() / 10.0,
        on_time,
        late,
        verified_cases,
        rule_failures,
        expired_decisions: blocked_after_refresh,
    })
}

/// 运行批量实验：每个 seed × 每种策略一行，按（seed，策略在配置中的顺序）排列。
pub fn run_batch(config: &BatchConfig) -> Result<Vec<BatchRow>> {
    ensure!(
        !config.seeds.is_empty() && config.seeds.len() <= MAX_SEEDS,
        "1..={MAX_SEEDS} seeds are required"
    );
    ensure!(
        !config.policies.is_empty(),
        "at least one policy is required"
    );
    ensure!(
        (1..=MAX_HORIZON_MINUTES).contains(&config.horizon_minutes),
        "horizon must be 1..={MAX_HORIZON_MINUTES} minutes"
    );
    let jobs: Vec<(u64, DecisionPolicy)> = config
        .seeds
        .iter()
        .flat_map(|&s| config.policies.iter().map(move |&p| (s, p)))
        .collect();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(jobs.len())
        .max(1);
    let chunk = jobs.len().div_ceil(threads);
    // 每个线程处理一段连续的任务；各任务互不共享状态，结果按原顺序拼接。
    let results: Vec<Result<Vec<BatchRow>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|&(seed, policy)| {
                            run_one(config, seed, policy)
                                .with_context(|| format!("seed {seed} {}", policy.label()))
                        })
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("batch worker panicked")))
            })
            .collect()
    });
    let mut rows = vec![];
    for r in results {
        rows.extend(r?);
    }
    Ok(rows)
}

/// CSV 字段：数字原样输出，缺失值为空。
fn csv_number(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}")).unwrap_or_default()
}

/// 导出 CSV（含表头），每行一个 seed × 策略。
pub fn to_csv(rows: &[BatchRow]) -> String {
    let mut out = String::from(
        "seed,policy,cases,alarms,decisions,mean_decision_minutes,risk_parts,on_time,late,verified_cases,rule_failures,expired_decisions\n",
    );
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{:.1},{},{},{},{},{}\n",
            r.seed,
            r.policy.label(),
            r.cases,
            r.alarms,
            r.decisions,
            csv_number(r.mean_decision_minutes),
            r.risk_parts,
            r.on_time,
            r.late,
            r.verified_cases,
            r.rule_failures,
            r.expired_decisions,
        ));
    }
    out
}
