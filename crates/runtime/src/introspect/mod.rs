//! 业务内省：定时分析运营数据，经发送闸门后才推送给运营负责人。
//!
//! 默认关闭（`[introspection] enabled = false`）。方案与进度见
//! `docs/memory/INTROSPECTION_PLAN.md`、`docs/memory/INTROSPECTION_PLAYBOOK.md`。

pub mod analyze;
pub mod cooccur;
pub mod eval;
pub mod explain;
pub mod finding;
pub mod gate;
pub mod job;
pub mod model;
pub mod operator;
pub mod push;
pub mod source;

pub use analyze::{analyze_adoption, stats_for_template, AdoptionStats, TOPIC_ADOPTION};
pub use cooccur::{
    analyze_cooccurrence, merge_cooccur_by_case_ids, stats_for_cooccur_key, CooccurStats,
    NON_ROOT_CAUSE_LABEL, TOPIC_COOCCUR_ASSET, TOPIC_COOCCUR_BATCH,
};
pub use eval::{assert_adoption_stats, coverage_against_truth, truth_alarm_assets, DesEvalReport};
pub use explain::{
    slot_explainer_from_backends, AlwaysSkipExplainer, ExplainOutcome, FixedExplainer,
    ModelExplainer, SlotGatedExplainer,
};
pub use finding::{
    input_digest_from_draft_stats, input_digest_from_stats, stable_finding_id, validate_draft,
    DraftStats, EvidenceRef, FindingDraft, ValidateOutcome, Verification,
};
pub use gate::{decide, GateConfig, GateDecision, GateInput};
pub use job::{run_once, should_run_schedule, RunOnceOptions, RunOnceOutcome, RunOnceRequest};
pub use operator::{
    ensure_operator, ensure_operator_with, is_operator, lock_operator_override,
    log_operator_role_status, set_operator_override, AccessDenied,
};
pub use push::{push_finding, PushResult, INTROSPECTION_PREFIX, MAIN_CONVERSATION_TITLE};
pub use source::{
    ActionSummary, ApprovalSummary, CaseSummary, MemorySource, MomSource, SourceAdapter,
};
