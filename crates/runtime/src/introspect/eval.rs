//! DES 植入真值评测辅助（S7）：只读核对，不写 MOM / DES。

use std::collections::BTreeSet;

use an_des::GroundTruth;

use super::analyze::AdoptionStats;
use super::finding::FindingDraft;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesEvalReport {
    pub alarm_count: usize,
    pub alarm_assets: Vec<String>,
    pub evidence_case_ids: Vec<String>,
    /// 真值里的设备是否在证据 case_id / hypothesis 文本中出现（启发式覆盖）。
    pub assets_mentioned: Vec<String>,
    pub assets_missing: Vec<String>,
}

/// 从标准答案抽出植入报警涉及的设备。
pub fn truth_alarm_assets(truth: &GroundTruth) -> Vec<String> {
    let mut s = BTreeSet::new();
    for a in &truth.alarms {
        if !a.asset.is_empty() {
            s.insert(a.asset.clone());
        }
    }
    s.into_iter().collect()
}

/// 核对：植入报警的设备是否被内省结论的证据或假设提到。
pub fn coverage_against_truth(truth: &GroundTruth, drafts: &[FindingDraft]) -> DesEvalReport {
    let alarm_assets = truth_alarm_assets(truth);
    let mut evidence_case_ids = BTreeSet::new();
    let mut blob = String::new();
    for d in drafts {
        blob.push_str(&d.hypothesis);
        blob.push(' ');
        blob.push_str(&d.scope);
        for e in &d.evidence {
            evidence_case_ids.insert(e.case_id.clone());
            blob.push_str(&e.case_id);
            blob.push(' ');
        }
    }
    let mut assets_mentioned = Vec::new();
    let mut assets_missing = Vec::new();
    for asset in &alarm_assets {
        if blob.contains(asset) {
            assets_mentioned.push(asset.clone());
        } else {
            assets_missing.push(asset.clone());
        }
    }
    DesEvalReport {
        alarm_count: truth.alarms.len(),
        alarm_assets,
        evidence_case_ids: evidence_case_ids.into_iter().collect(),
        assets_mentioned,
        assets_missing,
    }
}

/// 核对采纳模式统计是否与「植入」期望一致（评测夹具用）。
pub fn assert_adoption_stats(
    stats: &AdoptionStats,
    approvals_total: u32,
    succeeded_verified: u32,
    action_failed: u32,
) -> Result<(), String> {
    if stats.approvals_total != approvals_total {
        return Err(format!(
            "approvals_total: got {} want {approvals_total}",
            stats.approvals_total
        ));
    }
    if stats.succeeded_verified != succeeded_verified {
        return Err(format!(
            "succeeded_verified: got {} want {succeeded_verified}",
            stats.succeeded_verified
        ));
    }
    if stats.action_failed != action_failed {
        return Err(format!(
            "action_failed: got {} want {action_failed}",
            stats.action_failed
        ));
    }
    Ok(())
}
