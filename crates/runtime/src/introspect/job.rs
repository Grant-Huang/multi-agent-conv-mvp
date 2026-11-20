//! 内省调度：租约、超时、取消、清理。S4 不推送消息。

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::watch;

use super::analyze::analyze_adoption;
use super::finding::{
    input_digest_from_stats, stable_finding_id, validate_draft, ValidateOutcome, Verification,
};
use super::gate::{decide, GateConfig, GateInput};
use super::model::IntrospectionFinding;
use super::source::SourceAdapter;
use crate::bus::Bus;
use crate::config::IntrospectionSection;
use crate::store::Store;
use an_core::now;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOnceOutcome {
    Skipped { reason: String },
    Done { job_id: String },
    Failed { job_id: String, error: String },
    Cancelled { job_id: String },
}

/// `run_once` 的可测选项（生产路径用默认值）。
#[derive(Clone)]
pub struct RunOnceOptions {
    pub timeout: Duration,
    pub lease_until: String,
    pub cancel: Option<watch::Receiver<bool>>,
    /// 每写入一条 finding 后休眠（仅测试用来触发超时）
    pub after_finding_delay: Option<Duration>,
    /// 写入这么多条 finding 后视为取消（仅测试；生产为 None）
    pub cancel_after_findings: Option<usize>,
}

impl RunOnceOptions {
    pub fn from_config(cfg: &IntrospectionSection) -> Self {
        Self {
            timeout: Duration::from_secs(cfg.timeout_secs.max(1)),
            lease_until: far_lease(),
            cancel: None,
            after_finding_delay: None,
            cancel_after_findings: None,
        }
    }
}

fn far_lease() -> String {
    "9999-12-31T00:00:00.000Z".into()
}

fn cancelled(rx: &Option<watch::Receiver<bool>>) -> bool {
    rx.as_ref().is_some_and(|r| *r.borrow())
}

/// 距上次成功结束是否已满 `schedule_hours`。`last_finished_at` / `now` 为 RFC3339。
pub fn should_run_schedule(
    last_finished_at: Option<&str>,
    now_rfc3339: &str,
    schedule_hours: u64,
) -> bool {
    let Some(last) = last_finished_at else {
        return true;
    };
    let Ok(last_t) = chrono::DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    let Ok(now_t) = chrono::DateTime::parse_from_rfc3339(now_rfc3339) else {
        return true;
    };
    let due = last_t + chrono::Duration::hours(schedule_hours as i64);
    now_t >= due
}

fn gate_cfg(cfg: &IntrospectionSection) -> GateConfig {
    GateConfig {
        push_threshold: cfg.push_threshold,
        daily_cap: cfg.daily_cap,
        impact_threshold: 3,
    }
}

/// `run_once` 的入参（避免过长参数列表）。
pub struct RunOnceRequest<'a> {
    pub store: &'a Store,
    pub source: &'a dyn SourceAdapter,
    pub cfg: &'a IntrospectionSection,
    pub trigger: &'a str,
    pub watermark_json: &'a str,
    pub input_hash: &'a str,
    pub analysis_now: u64,
    pub opts: RunOnceOptions,
    /// 闸门通过后推送到主窗口时可选广播；测试可传 None。
    pub bus: Option<&'a Bus>,
}

/// 跑一轮内省（定时或手动）。闸门通过时写入主窗口（S5），不走 handle_message。
pub async fn run_once(req: RunOnceRequest<'_>) -> Result<RunOnceOutcome> {
    let RunOnceRequest {
        store,
        source,
        cfg,
        trigger,
        watermark_json,
        input_hash,
        analysis_now,
        opts,
        bus,
    } = req;
    let stale = store.introspection_fail_stale_jobs()?;
    if stale > 0 {
        tracing::info!(
            event = "introspection.job_failed",
            count = stale,
            "清理租约过期的 running 任务"
        );
    }

    let Some(job) =
        store.introspection_acquire_job(trigger, watermark_json, input_hash, &opts.lease_until)?
    else {
        tracing::info!(
            event = "introspection.job_skipped",
            reason = "lease_held",
            "已有未过期租约，本轮跳过"
        );
        return Ok(RunOnceOutcome::Skipped {
            reason: "lease_held".into(),
        });
    };

    tracing::info!(
        event = "introspection.job_start",
        task_id = %job.id,
        trigger,
        "内省任务开始"
    );

    let job_id = job.id.clone();
    let min_samples = cfg.min_samples;
    let gcfg = gate_cfg(cfg);
    let cancel = opts.cancel.clone();
    let delay = opts.after_finding_delay;
    let cancel_after = opts.cancel_after_findings;
    let timeout = opts.timeout;

    // 分析放 blocking，避免堵 tokio；source 数据已是摘要拷贝，可在闭包里再读。
    let work = async {
        if cancelled(&cancel) {
            return Ok::<_, anyhow::Error>(WorkEnd::Cancelled);
        }
        let drafts = {
            let source_id = source.source_id().to_string();
            let cases = source.list_case_summaries()?;
            tokio::task::spawn_blocking(move || {
                let mem = super::source::MemorySource { source_id, cases };
                analyze_adoption(&mem, analysis_now, min_samples)
            })
            .await
            .context("分析任务 join 失败")??
        };

        let mut written = 0usize;
        for d in drafts {
            if cancelled(&cancel) {
                return Ok(WorkEnd::Cancelled);
            }
            match validate_draft(source, &d)? {
                ValidateOutcome::Drop { reason } => {
                    tracing::info!(
                        event = "introspection.finding",
                        task_id = %job_id,
                        outcome = "drop",
                        %reason,
                        "丢弃结论"
                    );
                }
                ValidateOutcome::MarkInconclusive { draft, reason } => {
                    tracing::info!(
                        event = "introspection.finding",
                        task_id = %job_id,
                        outcome = "inconclusive",
                        %reason,
                        scope = %draft.scope,
                        "样本或证据不足，不入库推送候选"
                    );
                }
                ValidateOutcome::Accept(draft) => {
                    if draft.verification != Verification::Supported {
                        continue;
                    }
                    let digest = input_digest_from_stats(&draft.stats);
                    let fid = stable_finding_id(draft.topic, &draft.scope, &digest);
                    let at = now();
                    let json = serde_json::to_string(&serde_json::json!({
                        "template": draft.stats.template,
                        "approvals_total": draft.stats.approvals_total,
                        "succeeded_verified": draft.stats.succeeded_verified,
                        "action_failed": draft.stats.action_failed,
                        "approval_expired_unused": draft.stats.approval_expired_unused,
                        "latency_secs": draft.stats.latency_secs,
                        "hypothesis": draft.hypothesis,
                        "next_step": draft.next_step,
                    }))?;
                    let row = IntrospectionFinding {
                        id: fid.clone(),
                        job_id: job_id.clone(),
                        topic: draft.topic.into(),
                        json,
                        verification: draft.verification.as_str().into(),
                        gate_score: None,
                        gate_reason: String::new(),
                        pushed: false,
                        verdict: None,
                        verdict_note: None,
                        created_at: at.clone(),
                        updated_at: at,
                    };
                    let inserted = store.introspection_insert_finding(&row)?;
                    tracing::info!(
                        event = "introspection.finding",
                        task_id = %job_id,
                        finding_id = %fid,
                        inserted,
                        "写入结论"
                    );

                    let already_pushed_unread =
                        store.introspection_has_unread_push(&fid).unwrap_or(false);
                    let gin = GateInput {
                        finding_id: fid.clone(),
                        verification: draft.verification.clone(),
                        impact_cases: draft.stats.approvals_total.max(draft.evidence.len() as u32),
                        has_next_step: !draft.next_step.is_empty(),
                        first_or_worsened: inserted,
                        time_sensitive: false,
                        already_pushed_unread,
                        pushed_today: 0,
                        model_delta: 0,
                        model_reason: String::new(),
                        extra_pending: 0,
                    };
                    let decision = decide(&gin, &gcfg);
                    store.introspection_set_gate(
                        &fid,
                        decision.score,
                        &decision.reason,
                        decision.should_push,
                    )?;
                    tracing::info!(
                        event = "introspection.gate",
                        task_id = %job_id,
                        finding_id = %fid,
                        should_push = decision.should_push,
                        score = decision.score,
                        reason = %decision.reason,
                        "闸门判定"
                    );

                    if decision.should_push {
                        let summary = if draft.hypothesis.is_empty() {
                            format!("内省结论（{}）", draft.scope)
                        } else {
                            draft.hypothesis.clone()
                        };
                        super::push::push_finding(
                            store,
                            bus,
                            &fid,
                            draft.topic,
                            &summary,
                            &decision.reason,
                            draft.verification.as_str(),
                        )?;
                    }

                    written += 1;
                    if cancel_after == Some(written) {
                        return Ok(WorkEnd::Cancelled);
                    }
                    if let Some(d) = delay {
                        tokio::time::sleep(d).await;
                    }
                }
            }
        }
        Ok(WorkEnd::Complete)
    };

    let result = tokio::time::timeout(timeout, work).await;
    match result {
        Ok(Ok(WorkEnd::Complete)) => {
            store.introspection_finish_job(&job_id, "done", None)?;
            tracing::info!(event = "introspection.job_end", task_id = %job_id, "内省任务完成");
            Ok(RunOnceOutcome::Done { job_id })
        }
        Ok(Ok(WorkEnd::Cancelled)) => {
            store.introspection_finish_job(&job_id, "cancelled", Some("cancelled"))?;
            tracing::info!(event = "introspection.job_end", task_id = %job_id, "内省任务取消");
            Ok(RunOnceOutcome::Cancelled { job_id })
        }
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            store.introspection_finish_job(&job_id, "failed", Some(&msg))?;
            tracing::warn!(event = "introspection.job_failed", task_id = %job_id, error = %msg, "内省任务失败");
            Ok(RunOnceOutcome::Failed { job_id, error: msg })
        }
        Err(_elapsed) => {
            let msg = "超时：内省任务超过 timeout_secs";
            store.introspection_finish_job(&job_id, "failed", Some(msg))?;
            tracing::warn!(event = "introspection.job_failed", task_id = %job_id, error = msg, "内省任务超时");
            Ok(RunOnceOutcome::Failed {
                job_id,
                error: msg.into(),
            })
        }
    }
}

enum WorkEnd {
    Complete,
    Cancelled,
}
