//! 业务内省 S4：调度 / 租约 / 超时 / 取消（RT-INT-09/13/14/23）。

use an_runtime::config::IntrospectionSection;
use an_runtime::introspect::*;
use an_runtime::store::Store;
use std::path::Path;
use std::time::Duration;

const NOW: u64 = 1000;
const FUTURE: &str = "9999-12-31T00:00:00.000Z";
const PAST: &str = "2000-01-01T00:00:00.000Z";

fn cfg() -> IntrospectionSection {
    IntrospectionSection {
        enabled: true,
        schedule_hours: 24,
        timeout_secs: 30,
        min_samples: 1,
        push_threshold: 4.0,
        daily_cap: 1,
        operator_role: String::new(),
    }
}

fn approval(expires_at: u64) -> ApprovalSummary {
    ApprovalSummary {
        id: "production".into(),
        scope: "production".into(),
        approver: "supervisor".into(),
        expires_at,
        plan_revision: 1,
    }
}

fn ok_action(key: &str) -> ActionSummary {
    ActionSummary {
        key: key.into(),
        state: "ACCEPTED".into(),
        verification: Some("ok".into()),
        accepted_at: 100,
        complete_at: 102,
    }
}

fn case(id: &str, template: &str) -> CaseSummary {
    CaseSummary {
        case_id: id.into(),
        template: template.into(),
        scenario: "s1".into(),
        approvals: vec![approval(10_000)],
        actions: vec![ok_action(&format!("{id}:a"))],
        event_times: vec![(1, 50)],
    }
}

fn source_two_templates() -> MemorySource {
    MemorySource {
        source_id: "mom".into(),
        cases: vec![case("a", "tpl-a"), case("b", "tpl-b")],
    }
}

async fn run(
    store: &Store,
    src: &dyn SourceAdapter,
    c: &IntrospectionSection,
    trigger: &str,
    wm: &str,
    hash: &str,
    o: RunOnceOptions,
) -> RunOnceOutcome {
    run_once(RunOnceRequest {
        store,
        source: src,
        cfg: c,
        trigger,
        watermark_json: wm,
        input_hash: hash,
        analysis_now: NOW,
        opts: o,
    })
    .await
    .unwrap()
}

fn opts() -> RunOnceOptions {
    RunOnceOptions {
        timeout: Duration::from_secs(30),
        lease_until: FUTURE.into(),
        cancel: None,
        after_finding_delay: None,
        cancel_after_findings: None,
    }
}

#[tokio::test]
async fn rt_int_09_rerun_same_watermark_does_not_duplicate_findings() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let src = source_two_templates();
    let c = cfg();
    let o1 = run(
        &store,
        &src,
        &c,
        "schedule",
        r#"{"mom":1}"#,
        "hash-1",
        opts(),
    )
    .await;
    assert!(matches!(o1, RunOnceOutcome::Done { .. }), "{o1:?}");
    let n1 = store.introspection_list_findings(None, 100).unwrap().len();
    assert!(n1 >= 1, "至少应有结论");

    let o2 = run(
        &store,
        &src,
        &c,
        "schedule",
        r#"{"mom":1}"#,
        "hash-1",
        opts(),
    )
    .await;
    assert!(matches!(o2, RunOnceOutcome::Done { .. }), "{o2:?}");
    let n2 = store.introspection_list_findings(None, 100).unwrap().len();
    assert_eq!(n1, n2, "同一水位与输入哈希不得重复生成 Finding");
    assert_eq!(
        store.introspection_latest_watermark().unwrap().as_deref(),
        Some(r#"{"mom":1}"#)
    );
}

#[tokio::test]
async fn rt_int_13_cancel_keeps_written_findings_and_releases_lease() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let src = source_two_templates();
    let c = cfg();
    let mut o = opts();
    o.cancel_after_findings = Some(1);
    let outcome = run(&store, &src, &c, "manual", "{}", "h-cancel", o).await;
    assert!(
        matches!(outcome, RunOnceOutcome::Cancelled { .. }),
        "{outcome:?}"
    );
    let findings = store.introspection_list_findings(None, 100).unwrap();
    assert_eq!(
        findings.len(),
        1,
        "取消前已写入的 Finding 应保留且不再写新的"
    );
    let again = store
        .introspection_acquire_job("manual", "{}", "h2", FUTURE)
        .unwrap();
    assert!(again.is_some(), "取消后应能再抢租约");
}

#[tokio::test]
async fn rt_int_14_timeout_and_stale_running_jobs() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let stale = store
        .introspection_acquire_job("schedule", "{}", "stale", PAST)
        .unwrap()
        .unwrap();
    assert_eq!(stale.state, "running");

    let src = source_two_templates();
    let c = cfg();
    let mut o = opts();
    o.timeout = Duration::from_millis(80);
    o.after_finding_delay = Some(Duration::from_millis(200));
    let outcome = run(&store, &src, &c, "schedule", r#"{"mom":9}"#, "h-to", o).await;
    match outcome {
        RunOnceOutcome::Failed { error, .. } => assert!(error.contains("超时"), "{error}"),
        other => panic!("应超时失败，得到 {other:?}"),
    }
    let stale_job = store.introspection_get_job(&stale.id).unwrap().unwrap();
    assert_eq!(stale_job.state, "failed");
    assert!(stale_job.error.unwrap_or_default().contains("interrupted"));

    let findings = store.introspection_list_findings(None, 100).unwrap();
    assert!(!findings.is_empty(), "超时前已提交的 Finding 应保留");
    assert_eq!(
        store.introspection_latest_watermark().unwrap(),
        None,
        "失败任务不得推进水位"
    );
}

#[tokio::test]
async fn rt_int_23_schedule_gate_and_manual_uses_same_gate() {
    assert!(should_run_schedule(None, "2026-11-19T12:00:00.000Z", 24));
    assert!(!should_run_schedule(
        Some("2026-11-19T10:00:00.000Z"),
        "2026-11-19T12:00:00.000Z",
        24
    ));
    assert!(should_run_schedule(
        Some("2026-11-18T10:00:00.000Z"),
        "2026-11-19T12:00:00.000Z",
        24
    ));

    let def = IntrospectionSection::default();
    assert!(!def.enabled);
    assert_eq!(def.schedule_hours, 24);
    assert_eq!(def.timeout_secs, 120);

    let store = Store::open(Path::new(":memory:")).unwrap();
    let src = MemorySource {
        source_id: "mom".into(),
        cases: vec![case("m1", "manual-tpl")],
    };
    let c = cfg();
    let outcome = run(&store, &src, &c, "manual", "{}", "manual-hash", opts()).await;
    assert!(matches!(outcome, RunOnceOutcome::Done { .. }));
    let f = store.introspection_list_findings(None, 10).unwrap();
    assert_eq!(f.len(), 1);
    assert!(
        !f[0].gate_reason.is_empty(),
        "手动触发也必须走闸门并记录原因"
    );
}
