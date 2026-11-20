//! 业务内省 S8 第一阶段：同设备/同批次描述性共现（RT-INT-02）。

use an_runtime::introspect::*;

const NOW: u64 = 1000;

fn case(id: &str, asset: &str, batch: &str) -> CaseSummary {
    CaseSummary {
        case_id: id.into(),
        template: "t".into(),
        scenario: "s1".into(),
        asset: asset.into(),
        batch: batch.into(),
        approvals: vec![],
        actions: vec![],
        event_times: vec![(1, 50)],
    }
}

/// RT-INT-02：同 asset / 同 batch 共现计数可回算；含案例引用；样本不足提示；标明非根因。
#[test]
fn rt_int_02_descriptive_cooccur_counts_refs_and_insufficient() {
    let src = MemorySource {
        source_id: "mom".into(),
        cases: vec![
            case("a1", "CNC-01", "B-100"),
            case("a2", "CNC-01", "B-200"),
            case("a3", "CNC-01", "B-300"),
            case("b1", "CNC-02", "B-100"),
            case("b2", "CNC-99", "B-100"),
            case("solo", "CNC-SOLO", "B-SOLO"),
            case("thin1", "CNC-THIN", "B-THIN"),
            case("thin2", "CNC-THIN", "B-OTHER"),
        ],
    };

    // 门槛 3：CNC-01 与 B-100 够样本；CNC-THIN（2）样本不足；solo 不产出
    let drafts = analyze_cooccurrence(&src, NOW, 3).unwrap();

    let asset_ok = drafts
        .iter()
        .find(|d| d.topic == TOPIC_COOCCUR_ASSET && d.scope == "CNC-01")
        .expect("同设备 CNC-01");
    match &asset_ok.stats {
        DraftStats::Cooccur(s) => {
            assert_eq!(s.case_count, 3);
            assert_eq!(s.case_ids, vec!["a1", "a2", "a3"]);
            assert!(!s.insufficient_sample);
        }
        other => panic!("期望 Cooccur，得到 {other:?}"),
    }
    assert_eq!(asset_ok.verification, Verification::Supported);
    assert!(asset_ok.hypothesis.contains(NON_ROOT_CAUSE_LABEL));
    assert!(!asset_ok.hypothesis.contains("根因复发"));
    assert_eq!(asset_ok.evidence.len(), 3);
    for e in &asset_ok.evidence {
        assert!(src.resolve_ref(&e.case_id, e.event_seq).unwrap());
    }
    assert!(
        matches!(
            validate_draft(&src, asset_ok).unwrap(),
            ValidateOutcome::Accept(_)
        ),
        "计数须可回算"
    );

    let batch_ok = drafts
        .iter()
        .find(|d| d.topic == TOPIC_COOCCUR_BATCH && d.scope == "B-100")
        .expect("同批次 B-100");
    match &batch_ok.stats {
        DraftStats::Cooccur(s) => {
            assert_eq!(s.case_count, 3);
            assert_eq!(s.case_ids, vec!["a1", "b1", "b2"]);
        }
        other => panic!("期望 Cooccur，得到 {other:?}"),
    }
    assert!(batch_ok.hypothesis.contains(NON_ROOT_CAUSE_LABEL));

    let thin = drafts
        .iter()
        .find(|d| d.topic == TOPIC_COOCCUR_ASSET && d.scope == "CNC-THIN")
        .expect("样本不足组仍产出");
    assert!(thin.stats.insufficient_sample());
    assert_eq!(thin.verification, Verification::Inconclusive);
    assert_eq!(thin.hypothesis, "样本不足");
    assert!(matches!(
        validate_draft(&src, thin).unwrap(),
        ValidateOutcome::MarkInconclusive { .. }
    ));

    assert!(
        drafts
            .iter()
            .all(|d| d.scope != "CNC-SOLO" && d.scope != "B-SOLO"),
        "单例不算反复出现"
    );
}

/// 设备与批次指向同一批案例时合并为一条（避免双结论）。
#[test]
fn same_case_set_asset_and_batch_merge_to_one_finding() {
    let mut cases = Vec::new();
    for i in 0..10 {
        cases.push(case(&format!("c{i}"), "CNC-01", "B-100"));
    }
    let src = MemorySource {
        source_id: "mom".into(),
        cases,
    };
    let drafts = analyze_cooccurrence(&src, NOW, 10).unwrap();
    let supported: Vec<_> = drafts
        .iter()
        .filter(|d| d.verification == Verification::Supported)
        .collect();
    assert_eq!(supported.len(), 1, "同批案例只留一条：{supported:?}");
    assert!(supported[0].hypothesis.contains(NON_ROOT_CAUSE_LABEL));
    assert!(
        supported[0].hypothesis.contains("CNC-01") && supported[0].hypothesis.contains("B-100"),
        "hypothesis={}",
        supported[0].hypothesis
    );
}
