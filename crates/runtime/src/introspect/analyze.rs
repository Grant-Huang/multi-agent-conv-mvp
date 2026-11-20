//! 方案采纳模式：确定性统计，不调用模型。

use super::finding::{DraftStats, EvidenceRef, FindingDraft, Verification};
use super::source::{ActionSummary, CaseSummary, SourceAdapter};

pub const TOPIC_ADOPTION: &str = "adoption_pattern";

#[derive(Debug, Clone, PartialEq)]
pub struct AdoptionStats {
    pub template: String,
    pub approvals_total: u32,
    pub succeeded_verified: u32,
    pub action_failed: u32,
    pub approval_expired_unused: u32,
    /// 成功并验证的动作上，`complete_at - accepted_at`。
    pub latency_secs: Vec<u64>,
    pub sample_size: u32,
    pub insufficient_sample: bool,
}

fn template_key(t: &str) -> &str {
    if t.is_empty() {
        "_default"
    } else {
        t
    }
}

pub(crate) fn action_succeeded(a: &ActionSummary) -> bool {
    matches!(
        a.state.as_str(),
        "ACCEPTED" | "VERIFIED" | "succeeded" | "completed" | "done"
    )
}

fn action_failed(a: &ActionSummary) -> bool {
    let s = a.state.to_ascii_lowercase();
    s.contains("fail") || s.contains("error")
}

fn action_verified_ok(a: &ActionSummary) -> bool {
    action_succeeded(a)
        && a.verification
            .as_ref()
            .is_some_and(|v| !v.is_empty() && !v.to_ascii_lowercase().contains("fail"))
}

fn case_has_success(c: &CaseSummary) -> bool {
    c.actions.iter().any(action_succeeded)
}

/// 对单个 template 分组做统计。`now` 用于判断审批是否过期。
pub fn stats_for_template(
    cases: &[&CaseSummary],
    template: &str,
    now: u64,
    min_samples: u32,
) -> AdoptionStats {
    let mut approvals_total = 0u32;
    let mut succeeded_verified = 0u32;
    let mut action_failed_n = 0u32;
    let mut approval_expired_unused = 0u32;
    let mut latency_secs = Vec::new();

    for c in cases {
        approvals_total += c.approvals.len() as u32;
        for a in &c.actions {
            if action_verified_ok(a) {
                succeeded_verified += 1;
                latency_secs.push(a.complete_at.saturating_sub(a.accepted_at));
            } else if action_failed(a) {
                action_failed_n += 1;
            }
        }
        let used = case_has_success(c);
        for ap in &c.approvals {
            if ap.expires_at < now && !used {
                approval_expired_unused += 1;
            }
        }
    }

    let sample_size = approvals_total;
    AdoptionStats {
        template: template.into(),
        approvals_total,
        succeeded_verified,
        action_failed: action_failed_n,
        approval_expired_unused,
        latency_secs,
        sample_size,
        insufficient_sample: sample_size < min_samples,
    }
}

pub fn analyze_adoption(
    src: &dyn SourceAdapter,
    now: u64,
    min_samples: u32,
) -> anyhow::Result<Vec<FindingDraft>> {
    let cases = src.list_case_summaries()?;
    let mut by_tpl: std::collections::BTreeMap<String, Vec<&CaseSummary>> =
        std::collections::BTreeMap::new();
    for c in &cases {
        by_tpl
            .entry(template_key(&c.template).to_string())
            .or_default()
            .push(c);
    }

    let mut out = Vec::new();
    for (tpl, group) in by_tpl {
        let stats = stats_for_template(&group, &tpl, now, min_samples);
        let mut evidence = Vec::new();
        for c in &group {
            evidence.push(EvidenceRef {
                case_id: c.case_id.clone(),
                event_seq: c.event_times.first().map(|(s, _)| *s),
            });
        }
        let (verification, hypothesis, next_step) = if stats.insufficient_sample {
            (Verification::Inconclusive, "样本不足".into(), String::new())
        } else {
            (
                Verification::Supported,
                format!(
                    "模板 {tpl}：审批 {a}，成功并验证 {s}，失败 {f}，过期未用 {e}",
                    a = stats.approvals_total,
                    s = stats.succeeded_verified,
                    f = stats.action_failed,
                    e = stats.approval_expired_unused
                ),
                "核对失败与过期案例的审批流程".into(),
            )
        };
        out.push(FindingDraft {
            topic: TOPIC_ADOPTION,
            scope: tpl,
            stats: DraftStats::Adoption(stats),
            evidence,
            verification,
            hypothesis,
            next_step,
            analysis_now: now,
            min_samples,
        });
    }
    Ok(out)
}
