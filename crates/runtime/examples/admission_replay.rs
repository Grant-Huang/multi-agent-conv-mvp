//! E3 回放：用**正式代码**（`an_runtime::admission::AdmissionLedger`）对录制好的 agent 数据
//! 分别按三种准入方案判定，输出每个 agent 的结果。不调用模型，结果确定。
//!
//! 用法：cargo run -p an-runtime --example admission_replay -- <输入.json> [输出.json]
//!
//! 输入（由 RelOS `experiments/e3` 录制生成）：
//! ```json
//! {"sample_rates": [0.1, 0.2, 0.5], "theta": 0.5, "seed": 0,
//!  "agents": [{"agent_id": "H-01", "type": "H", "claim": {AgentClaim},
//!              "clarify_answers": ["..."],
//!              "scores": {"completeness": 1.0, "conversation": 0.8, "test": 0.9},
//!              "tasks": [{"task_id": "w-0001", "correct": true}],
//!              "degrade_start": null}]}
//! ```
//!
//! 方案：
//! - `A1` 只凭声明：授予 Card 申请的等级，不设试用期；
//! - `A2` 只做测试：走 S1–S7 定等级（Trusted 视为已审批），不设试用期；
//! - `A3@r` 完整协议：同 A2，外加试用期，按比例 r 抽检，警告与降级由账本决定。
//!
//! 上岗任务都按中风险派发：等级不够（Guest 或已降级）时不再领任务。

use std::path::Path;
use std::sync::Arc;

use an_core::Risk;
use an_runtime::admission::{
    AdmissionConfig, AdmissionLedger, AgentClaim, CheckOutcome, Contract, Scores, TrustLevel,
};
use an_runtime::store::Store;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Input {
    sample_rates: Vec<f64>,
    theta: f64,
    #[serde(default)]
    seed: u64,
    agents: Vec<AgentData>,
}

#[derive(Deserialize, Clone)]
struct AgentData {
    agent_id: String,
    #[serde(rename = "type")]
    kind: String,
    claim: AgentClaim,
    clarify_answers: Vec<String>,
    scores: Scores,
    tasks: Vec<TaskData>,
    degrade_start: Option<usize>,
}

#[derive(Deserialize, Clone)]
struct TaskData {
    task_id: String,
    correct: bool,
}

fn contract() -> Contract {
    Contract {
        role: "executor".into(),
        working_language: "zh".into(),
        context_scope: vec![],
        allowed_tools: vec![],
    }
}

/// 走 S1–S7，返回授予的等级（审批视为通过）
fn admit(l: &AdmissionLedger, a: &AgentData) -> Result<TrustLevel> {
    let q = l.submit(a.claim.clone())?;
    // 回答数与问题数对齐：录制的回答不足时补"未回答"（这本身会拉低对话质量分，已体现在 scores 里）
    let answers: Vec<String> = (0..q.len())
        .map(|i| {
            a.clarify_answers
                .get(i)
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "未回答".into())
        })
        .collect();
    l.clarify(&a.agent_id, answers)?;
    l.score(&a.agent_id, a.scores)?;
    l.contract(&a.agent_id, contract())?;
    let rec = l.get(&a.agent_id).context("记录丢失")?;
    if rec.granted == Some(TrustLevel::Trusted) {
        l.approve(&a.agent_id, "simulated-approver")?;
    }
    l.activate(&a.agent_id)?;
    Ok(l.get(&a.agent_id)
        .and_then(|r| r.granted)
        .unwrap_or(TrustLevel::Guest))
}

fn reliability(a: &AgentData) -> f64 {
    if a.tasks.is_empty() {
        return 0.0;
    }
    a.tasks.iter().filter(|t| t.correct).count() as f64 / a.tasks.len() as f64
}

fn level_str(l: TrustLevel) -> &'static str {
    match l {
        TrustLevel::Guest => "guest",
        TrustLevel::Member => "member",
        TrustLevel::Trusted => "trusted",
    }
}

/// 判定一个 agent 在某方案下的结果
fn run_arm(arm: &str, rate: Option<f64>, seed: u64, theta: f64, a: &AgentData) -> Result<Value> {
    let rel = reliability(a);
    let store = Arc::new(Store::open(Path::new(":memory:"))?);
    let cfg = AdmissionConfig {
        sample_rate: rate.unwrap_or(0.0),
        seed,
        ..AdmissionConfig::default()
    };
    let l = AdmissionLedger::open(store, cfg)?;
    let admitted = if arm == "A1" {
        a.claim.requested_level
    } else {
        admit(&l, a)?
    };
    let mut executed = 0usize;
    let mut bad_accepted = 0usize;
    let mut downgraded_at: Option<usize> = None;
    let mut sampled = 0usize;
    for (i, t) in a.tasks.iter().enumerate() {
        let allowed = if arm == "A1" {
            admitted >= TrustLevel::Member
        } else {
            l.permits(&a.agent_id, Risk::Medium)
        };
        if !allowed {
            break;
        }
        executed += 1;
        if !t.correct {
            bad_accepted += 1;
        }
        if rate.is_some() {
            let s = l.should_sample(&a.agent_id, &t.task_id);
            l.note_completed(&a.agent_id, &t.task_id, s)?;
            if s {
                sampled += 1;
                if l.record_check_for(&a.agent_id, &t.task_id, t.correct)?
                    == CheckOutcome::Downgraded
                {
                    downgraded_at = Some(i);
                }
            }
        }
    }
    let rec = l.get(&a.agent_id);
    let detection_delay = match (a.degrade_start, downgraded_at) {
        (Some(s), Some(d)) if d >= s => Some(d - s),
        _ => None,
    };
    Ok(json!({
        "arm": arm, "sample_rate": rate, "agent_id": a.agent_id, "type": a.kind,
        "true_reliability": rel, "untrustworthy": rel < theta,
        "admitted_level": level_str(admitted),
        "false_admission": admitted >= TrustLevel::Member && rel < theta,
        "false_rejection": admitted == TrustLevel::Guest && rel >= theta,
        "score_total": rec.as_ref().and_then(|r| r.total),
        "tasks_executed": executed, "bad_accepted": bad_accepted, "sampled": sampled,
        "downgraded_at": downgraded_at, "degrade_start": a.degrade_start,
        "detection_delay": detection_delay,
        "warnings": rec.map(|r| r.probation.warnings).unwrap_or(0),
    }))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .context("用法：admission_replay <输入.json> [输出.json]")?;
    let input: Input = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let mut rows = vec![];
    for a in &input.agents {
        rows.push(run_arm("A1", None, input.seed, input.theta, a)?);
        rows.push(run_arm("A2", None, input.seed, input.theta, a)?);
        for r in &input.sample_rates {
            rows.push(run_arm("A3", Some(*r), input.seed, input.theta, a)?);
        }
    }
    let out = json!({
        "theta": input.theta, "seed": input.seed, "sample_rates": input.sample_rates,
        "rules": AdmissionConfig::default(), "rows": rows,
    });
    let text = serde_json::to_string_pretty(&out)?;
    match args.get(2) {
        Some(p) => std::fs::write(p, text)?,
        None => println!("{text}"),
    }
    Ok(())
}
