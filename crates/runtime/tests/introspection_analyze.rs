//! 业务内省 S2：方案采纳模式统计与校验（RT-INT-03/06/07/08）。
//! 用例见 docs/memory/ACCEPTANCE.md；可执行说明见 docs/memory/INTROSPECTION_PLAYBOOK.md。

use an_runtime::introspect::*;
use an_runtime::mom::*;

const NOW: u64 = 1000;
const MIN_SAMPLES: u32 = 10;

fn approval(id: &str, expires_at: u64) -> ApprovalSummary {
    ApprovalSummary {
        id: id.into(),
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

fn failed_action(key: &str) -> ActionSummary {
    ActionSummary {
        key: key.into(),
        state: "FAILED".into(),
        verification: None,
        accepted_at: 100,
        complete_at: 101,
    }
}

/// RT-INT-03 夹具：同一 template 下 13 个带审批 Case = 9 成功验证 + 2 失败 + 2 过期未用。
fn adoption_fixture_13() -> MemorySource {
    let mut cases = Vec::new();
    for i in 0..9 {
        cases.push(CaseSummary {
            case_id: format!("ok-{i}"),
            template: "line_incident".into(),
            scenario: "s1".into(),
            approvals: vec![approval("production", 10_000)],
            actions: vec![ok_action(&format!("ok-{i}:a"))],
            event_times: vec![(1, 50)],
        });
    }
    for i in 0..2 {
        cases.push(CaseSummary {
            case_id: format!("fail-{i}"),
            template: "line_incident".into(),
            scenario: "s1".into(),
            approvals: vec![approval("production", 10_000)],
            actions: vec![failed_action(&format!("fail-{i}:a"))],
            event_times: vec![(1, 50)],
        });
    }
    for i in 0..2 {
        cases.push(CaseSummary {
            case_id: format!("exp-{i}"),
            template: "line_incident".into(),
            scenario: "s1".into(),
            approvals: vec![approval("production", 100)], // < NOW，过期
            actions: vec![],
            event_times: vec![(1, 50)],
        });
    }
    MemorySource {
        source_id: "mom".into(),
        cases,
    }
}

#[test]
fn rt_int_03_adoption_pattern_matches_recomputed_stats() {
    let src = adoption_fixture_13();
    let drafts = analyze_adoption(&src, NOW, MIN_SAMPLES).unwrap();
    assert_eq!(drafts.len(), 1, "同一 template 应合成一条");
    let d = &drafts[0];
    assert_eq!(d.topic, TOPIC_ADOPTION);
    assert_eq!(d.scope, "line_incident");
    assert_eq!(d.stats.approvals_total, 13);
    assert_eq!(d.stats.succeeded_verified, 9);
    assert_eq!(d.stats.action_failed, 2);
    assert_eq!(d.stats.approval_expired_unused, 2);
    assert!(!d.stats.insufficient_sample);
    assert_eq!(d.verification, Verification::Supported);
    assert_eq!(
        d.stats.latency_secs,
        vec![2; 9],
        "耗时 = complete_at - accepted_at"
    );
    // 不含源数据没有的字段
    assert!(!d.hypothesis.contains("驳回"));

    let id1 = stable_finding_id(d.topic, &d.scope, &input_digest_from_stats(&d.stats));
    let id2 = stable_finding_id(d.topic, &d.scope, &input_digest_from_stats(&d.stats));
    assert_eq!(id1, id2);
    assert!(id1.starts_with("intf-"));

    match validate_draft(&src, d).unwrap() {
        ValidateOutcome::Accept(_) => {}
        other => panic!("应 Accept，得到 {other:?}"),
    }

    // 样本不足分组不下结论
    let small = MemorySource {
        source_id: "mom".into(),
        cases: vec![CaseSummary {
            case_id: "s".into(),
            template: "tiny".into(),
            scenario: "s1".into(),
            approvals: vec![approval("production", 10_000)],
            actions: vec![ok_action("s:a")],
            event_times: vec![],
        }],
    };
    let tiny = analyze_adoption(&small, NOW, MIN_SAMPLES).unwrap();
    assert_eq!(tiny.len(), 1);
    assert!(tiny[0].stats.insufficient_sample);
    assert_eq!(tiny[0].verification, Verification::Inconclusive);
    assert!(tiny[0].hypothesis.contains("样本不足"));
}

#[test]
fn rt_int_06_analysis_makes_no_mom_writes() {
    let mut engine = Engine::memory().unwrap();
    let base = engine.now().max(497);
    let mut a = EventInput::s1("order", base);
    a.id = "int06-order".into();
    a.batch = "INT06".into();
    let id = engine.ingest(a).unwrap();
    for (k, d) in [("vibration", 6), ("quality", 14)] {
        engine.advance(base + d).unwrap();
        let mut x = EventInput::s1(k, base + d);
        x.id = format!("int06-{k}");
        x.batch = "INT06".into();
        engine.ingest(x).unwrap();
    }
    engine.advance(base + 17).unwrap();
    let rev = engine.case(&id).unwrap().plan_revision;
    engine
        .approve(&id, rev, "production", "supervisor")
        .unwrap();

    let before_seq = engine.sequence();
    let before_now = engine.now();
    let before_stage = engine.case(&id).unwrap().stage.clone();
    let before_actions = engine.case(&id).unwrap().actions.len();

    let src = MomSource::from_engine(&engine).unwrap();
    let _ = analyze_adoption(&src, MomSource::now_from_engine(&engine), MIN_SAMPLES).unwrap();

    assert_eq!(engine.sequence(), before_seq, "分析不得推进事件序号");
    assert_eq!(engine.now(), before_now);
    assert_eq!(engine.case(&id).unwrap().stage, before_stage);
    assert_eq!(engine.case(&id).unwrap().actions.len(), before_actions);
    assert!(
        src.list_case_summaries()
            .unwrap()
            .iter()
            .any(|c| c.case_id == id),
        "MomSource 应能从真实 Engine 只读拷出案例"
    );
}

#[test]
fn rt_int_07_insufficient_evidence_is_inconclusive() {
    let src = MemorySource {
        source_id: "mom".into(),
        cases: vec![CaseSummary {
            case_id: "c1".into(),
            template: "t".into(),
            scenario: "s1".into(),
            approvals: vec![approval("production", 10_000)],
            actions: vec![],
            event_times: vec![(1, 1)],
        }],
    };
    let drafts = analyze_adoption(&src, NOW, MIN_SAMPLES).unwrap();
    let d = &drafts[0];
    assert_eq!(d.verification, Verification::Inconclusive);
    match validate_draft(&src, d).unwrap() {
        ValidateOutcome::MarkInconclusive { draft, reason } => {
            assert_eq!(draft.verification, Verification::Inconclusive);
            assert!(reason.contains("不足"));
        }
        other => panic!("应 MarkInconclusive，得到 {other:?}"),
    }
}

#[test]
fn rt_int_08_unresolvable_evidence_is_dropped() {
    let src = adoption_fixture_13();
    let mut drafts = analyze_adoption(&src, NOW, MIN_SAMPLES).unwrap();
    let mut bad = drafts.remove(0);
    bad.evidence.push(EvidenceRef {
        case_id: "does-not-exist".into(),
        event_seq: Some(99),
    });
    // 篡改数字也应 Drop
    let mut numbers_tampered = bad.clone();
    numbers_tampered.evidence.pop(); // 先恢复引用
    numbers_tampered.stats.succeeded_verified = 0;

    match validate_draft(&src, &bad).unwrap() {
        ValidateOutcome::Drop { reason } => assert!(reason.contains("不可解析")),
        other => panic!("坏引用应 Drop，得到 {other:?}"),
    }
    match validate_draft(&src, &numbers_tampered).unwrap() {
        ValidateOutcome::Drop { reason } => assert!(reason.contains("回算")),
        other => panic!("篡改数字应 Drop，得到 {other:?}"),
    }

    // 其余完好草稿仍可 Accept
    let good = analyze_adoption(&src, NOW, MIN_SAMPLES).unwrap().remove(0);
    match validate_draft(&src, &good).unwrap() {
        ValidateOutcome::Accept(_) => {}
        other => panic!("{other:?}"),
    }
}
