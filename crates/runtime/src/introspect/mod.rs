//! 业务内省：定时分析运营数据，经发送闸门后才推送给运营负责人。
//!
//! 默认关闭（`[introspection] enabled = false`）。方案与进度见
//! `docs/memory/INTROSPECTION_PLAN.md`、`docs/memory/INTROSPECTION_PLAYBOOK.md`。

pub mod analyze;
pub mod finding;
pub mod gate;
pub mod job;
pub mod model;
pub mod source;

pub use analyze::{analyze_adoption, stats_for_template, AdoptionStats, TOPIC_ADOPTION};
pub use finding::{
    input_digest_from_stats, stable_finding_id, validate_draft, EvidenceRef, FindingDraft,
    ValidateOutcome, Verification,
};
pub use gate::{decide, GateConfig, GateDecision, GateInput};
pub use job::{run_once, should_run_schedule, RunOnceOptions, RunOnceOutcome, RunOnceRequest};
pub use source::{
    ActionSummary, ApprovalSummary, CaseSummary, MemorySource, MomSource, SourceAdapter,
};
