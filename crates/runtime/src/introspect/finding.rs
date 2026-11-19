//! Finding 草稿、稳定 ID、校验器。

use super::analyze::AdoptionStats;
use super::source::SourceAdapter;
use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Supported,
    Inconclusive,
    Contradicted,
}

impl Verification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Inconclusive => "inconclusive",
            Self::Contradicted => "contradicted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    pub case_id: String,
    pub event_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FindingDraft {
    pub topic: &'static str,
    pub scope: String,
    pub stats: AdoptionStats,
    pub evidence: Vec<EvidenceRef>,
    pub verification: Verification,
    pub hypothesis: String,
    pub next_step: String,
    /// 分析时使用的虚拟时间（校验回算必须一致）。
    pub analysis_now: u64,
    pub min_samples: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidateOutcome {
    Accept(FindingDraft),
    Drop { reason: String },
    MarkInconclusive { draft: FindingDraft, reason: String },
}

/// 稳定 ID：同一 topic/scope/input_digest 必须相同。不使用 `DefaultHasher`。
pub fn stable_finding_id(topic: &str, scope: &str, input_digest: &str) -> String {
    let s = format!("{topic}\0{scope}\0{input_digest}");
    let mut sum = 2166136261u64;
    for (i, b) in s.bytes().enumerate() {
        sum ^= b as u64;
        sum = sum.wrapping_mul(16777619);
        sum ^= (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
    }
    format!("intf-{sum:016x}")
}

pub fn input_digest_from_stats(stats: &AdoptionStats) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        stats.approvals_total,
        stats.succeeded_verified,
        stats.action_failed,
        stats.approval_expired_unused,
        stats.sample_size,
        stats.latency_secs.len()
    )
}

/// 校验：引用可解析、数字可回算；样本不足保持 inconclusive。
pub fn validate_draft(src: &dyn SourceAdapter, draft: &FindingDraft) -> Result<ValidateOutcome> {
    for e in &draft.evidence {
        if !src.resolve_ref(&e.case_id, e.event_seq)? {
            return Ok(ValidateOutcome::Drop {
                reason: format!(
                    "证据引用不可解析：case_id={} event_seq={:?}",
                    e.case_id, e.event_seq
                ),
            });
        }
    }

    let cases = src.list_case_summaries()?;
    let group: Vec<_> = cases
        .iter()
        .filter(|c| {
            let t = if c.template.is_empty() {
                "_default"
            } else {
                c.template.as_str()
            };
            t == draft.scope
        })
        .collect();

    let recomputed = super::analyze::stats_for_template(
        &group,
        &draft.scope,
        draft.analysis_now,
        draft.min_samples,
    );
    let core_ok = recomputed.approvals_total == draft.stats.approvals_total
        && recomputed.succeeded_verified == draft.stats.succeeded_verified
        && recomputed.action_failed == draft.stats.action_failed
        && recomputed.approval_expired_unused == draft.stats.approval_expired_unused
        && recomputed.latency_secs == draft.stats.latency_secs;
    if !core_ok {
        return Ok(ValidateOutcome::Drop {
            reason: "统计数字与回算不一致".into(),
        });
    }

    if draft.stats.insufficient_sample || draft.verification == Verification::Inconclusive {
        let mut d = draft.clone();
        d.verification = Verification::Inconclusive;
        return Ok(ValidateOutcome::MarkInconclusive {
            draft: d,
            reason: "证据不足或样本不足".into(),
        });
    }

    Ok(ValidateOutcome::Accept(draft.clone()))
}
