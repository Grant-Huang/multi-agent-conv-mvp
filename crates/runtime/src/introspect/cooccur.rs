//! 同设备 / 同批次描述性共现（S8 第一阶段）：非根因结论。
//! 「已确认根因的复发」为第二阶段，见 ACCEPTANCE RT-INT-02b。

use std::collections::BTreeMap;

use super::finding::{DraftStats, EvidenceRef, FindingDraft, Verification};
use super::source::{CaseSummary, SourceAdapter};

pub const TOPIC_COOCCUR_ASSET: &str = "cooccur_asset";
pub const TOPIC_COOCCUR_BATCH: &str = "cooccur_batch";

pub const NON_ROOT_CAUSE_LABEL: &str = "描述性共现，非根因";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooccurStats {
    /// `"asset"` 或 `"batch"`。
    pub key_kind: String,
    pub key: String,
    pub case_count: u32,
    /// 排序后的案例 id，便于回算与 digest。
    pub case_ids: Vec<String>,
    pub sample_size: u32,
    pub insufficient_sample: bool,
}

fn nonempty_key(raw: &str) -> Option<&str> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// 按 `key_kind`（asset|batch）分组统计；仅保留出现 ≥2 次的键（单例不算「反复」）。
pub fn stats_for_cooccur_key(
    cases: &[&CaseSummary],
    key_kind: &str,
    key: &str,
    min_samples: u32,
) -> CooccurStats {
    let mut case_ids: Vec<String> = cases.iter().map(|c| c.case_id.clone()).collect();
    case_ids.sort();
    case_ids.dedup();
    let case_count = case_ids.len() as u32;
    CooccurStats {
        key_kind: key_kind.into(),
        key: key.into(),
        case_count,
        case_ids,
        sample_size: case_count,
        insufficient_sample: case_count < min_samples,
    }
}

fn group_by(
    cases: &[CaseSummary],
    pick: impl Fn(&CaseSummary) -> &str,
) -> BTreeMap<String, Vec<&CaseSummary>> {
    let mut map: BTreeMap<String, Vec<&CaseSummary>> = BTreeMap::new();
    for c in cases {
        if let Some(k) = nonempty_key(pick(c)) {
            map.entry(k.to_string()).or_default().push(c);
        }
    }
    map
}

fn drafts_for_kind(
    groups: BTreeMap<String, Vec<&CaseSummary>>,
    key_kind: &str,
    topic: &'static str,
    now: u64,
    min_samples: u32,
) -> Vec<FindingDraft> {
    let mut out = Vec::new();
    for (key, group) in groups {
        if group.len() < 2 {
            continue;
        }
        let stats = stats_for_cooccur_key(&group, key_kind, &key, min_samples);
        let evidence: Vec<EvidenceRef> = group
            .iter()
            .map(|c| EvidenceRef {
                case_id: c.case_id.clone(),
                event_seq: c.event_times.first().map(|(s, _)| *s),
            })
            .collect();
        let (verification, hypothesis, next_step) = if stats.insufficient_sample {
            (Verification::Inconclusive, "样本不足".into(), String::new())
        } else {
            (
                Verification::Supported,
                format!(
                    "{label}：同{kind}「{key}」出现 {n} 次（案例 {ids}）",
                    label = NON_ROOT_CAUSE_LABEL,
                    kind = if key_kind == "asset" {
                        "设备"
                    } else {
                        "批次"
                    },
                    key = key,
                    n = stats.case_count,
                    ids = stats.case_ids.join(","),
                ),
                "核对同键案例是否需合并处置；外发前仍须闸门审核重要性与相关性".into(),
            )
        };
        out.push(FindingDraft {
            topic,
            scope: key,
            stats: DraftStats::Cooccur(stats),
            evidence,
            verification,
            hypothesis,
            next_step,
            analysis_now: now,
            min_samples,
        });
    }
    out
}

/// 同一批案例的设备/批次共现合并为一条，避免内省页双条、闸门双推。
pub fn merge_cooccur_by_case_ids(drafts: Vec<FindingDraft>) -> Vec<FindingDraft> {
    let mut kept: BTreeMap<String, FindingDraft> = BTreeMap::new();
    let mut rest = Vec::new();
    for d in drafts {
        let Some(stats) = d.stats.as_cooccur() else {
            rest.push(d);
            continue;
        };
        let id_key = stats.case_ids.join("\0");
        match kept.get_mut(&id_key) {
            None => {
                kept.insert(id_key, d);
            }
            Some(existing) => {
                let other_kind = stats.key_kind.as_str();
                let other_key = stats.key.as_str();
                if existing.verification == Verification::Supported
                    && !existing.hypothesis.contains(other_key)
                {
                    let kind_zh = if other_kind == "batch" {
                        "批次"
                    } else {
                        "设备"
                    };
                    existing.hypothesis =
                        format!("{}；兼同{kind_zh}「{other_key}」", existing.hypothesis);
                }
            }
        }
    }
    rest.extend(kept.into_values());
    rest
}

/// 产出同设备与同批次共现草稿（可为空）；同案例集合合并为一条。
pub fn analyze_cooccurrence(
    src: &dyn SourceAdapter,
    now: u64,
    min_samples: u32,
) -> anyhow::Result<Vec<FindingDraft>> {
    let cases = src.list_case_summaries()?;
    let by_asset = group_by(&cases, |c| c.asset.as_str());
    let by_batch = group_by(&cases, |c| c.batch.as_str());
    let mut out = drafts_for_kind(by_asset, "asset", TOPIC_COOCCUR_ASSET, now, min_samples);
    out.extend(drafts_for_kind(
        by_batch,
        "batch",
        TOPIC_COOCCUR_BATCH,
        now,
        min_samples,
    ));
    Ok(merge_cooccur_by_case_ids(out))
}
