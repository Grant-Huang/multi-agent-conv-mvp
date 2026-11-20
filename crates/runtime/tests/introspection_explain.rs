//! 业务内省 S7：模型解释跳过（RT-INT-15）与 DES 真值评测辅助。

use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_des::{AlarmTruth, GroundTruth, Mechanisms};
use an_runtime::config::IntrospectionSection;
use an_runtime::introspect::*;
use an_runtime::store::Store;
use tracing_subscriber::util::SubscriberInitExt;

const NOW: u64 = 1000;
const FUTURE: &str = "9999-12-31T00:00:00.000Z";

fn base_cfg(model_explain: bool) -> IntrospectionSection {
    IntrospectionSection {
        enabled: true,
        schedule_hours: 24,
        timeout_secs: 30,
        min_samples: 1,
        push_threshold: 4.0,
        daily_cap: 10,
        operator_role: String::new(),
        model_explain,
    }
}

fn case(id: &str, template: &str) -> CaseSummary {
    CaseSummary {
        case_id: id.into(),
        template: template.into(),
        scenario: "s1".into(),
        approvals: vec![ApprovalSummary {
            id: "production".into(),
            scope: "production".into(),
            approver: "supervisor".into(),
            expires_at: 10_000,
            plan_revision: 1,
        }],
        actions: vec![ActionSummary {
            key: format!("{id}:a"),
            state: "ACCEPTED".into(),
            verification: Some("ok".into()),
            accepted_at: 100,
            complete_at: 102,
        }],
        event_times: vec![(1, 50)],
    }
}

fn source_three() -> MemorySource {
    MemorySource {
        source_id: "mom".into(),
        cases: vec![case("a", "tpl"), case("b", "tpl"), case("c", "tpl")],
    }
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

struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn rt_int_15_no_slot_skips_model_keeps_deterministic_finding() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let make = buf.clone();
    let _guard = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || LogBuf(make.clone()))
        .with_ansi(false)
        .set_default();

    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = base_cfg(true);
    let src = source_three();
    let explainer: Arc<dyn ModelExplainer> = Arc::new(AlwaysSkipExplainer {
        reason: "无槽位".into(),
    });

    let out = run_once(RunOnceRequest {
        store: &store,
        source: &src,
        cfg: &cfg,
        trigger: "manual",
        watermark_json: "{}",
        input_hash: "h15",
        analysis_now: NOW,
        opts: opts(),
        bus: None,
        explainer: Some(explainer),
    })
    .await
    .unwrap();
    assert!(matches!(out, RunOnceOutcome::Done { .. }), "{out:?}");

    let findings = store.introspection_list_findings(None, 10).unwrap();
    assert_eq!(findings.len(), 1, "仍应产出确定性结论");
    let json: serde_json::Value = serde_json::from_str(&findings[0].json).unwrap();
    let hyp = json["hypothesis"].as_str().unwrap_or("");
    assert!(!hyp.contains("模型解释"), "无槽位时不应有解释文字：{hyp}");
    assert_eq!(json["approvals_total"], 3);

    let bytes = buf.lock().unwrap().clone();
    let log = String::from_utf8_lossy(&bytes);
    assert!(
        log.contains("introspection.job_skipped") && log.contains("无槽位"),
        "应记 job_skipped / 无槽位：{log}"
    );
}

#[tokio::test]
async fn model_explain_off_ignores_explainer() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = base_cfg(false);
    let src = source_three();
    let explainer: Arc<dyn ModelExplainer> = Arc::new(FixedExplainer {
        suffix: "（不应出现）".into(),
        model_delta: 0,
        model_reason: "x".into(),
    });
    run_once(RunOnceRequest {
        store: &store,
        source: &src,
        cfg: &cfg,
        trigger: "manual",
        watermark_json: "{}",
        input_hash: "hoff",
        analysis_now: NOW,
        opts: opts(),
        bus: None,
        explainer: Some(explainer),
    })
    .await
    .unwrap();
    let f = store.introspection_list_findings(None, 1).unwrap();
    let json: serde_json::Value = serde_json::from_str(&f[0].json).unwrap();
    assert!(!json["hypothesis"].as_str().unwrap().contains("不应出现"));
}

#[tokio::test]
async fn model_explain_applied_appends_text_without_changing_stats() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg = base_cfg(true);
    let src = source_three();
    let explainer: Arc<dyn ModelExplainer> = Arc::new(FixedExplainer {
        suffix: "（模型解释）定性说明".into(),
        model_delta: 0,
        model_reason: "解释层未调整分数".into(),
    });
    run_once(RunOnceRequest {
        store: &store,
        source: &src,
        cfg: &cfg,
        trigger: "manual",
        watermark_json: "{}",
        input_hash: "hon",
        analysis_now: NOW,
        opts: opts(),
        bus: None,
        explainer: Some(explainer),
    })
    .await
    .unwrap();
    let f = store.introspection_list_findings(None, 1).unwrap();
    let json: serde_json::Value = serde_json::from_str(&f[0].json).unwrap();
    assert!(json["hypothesis"]
        .as_str()
        .unwrap()
        .contains("（模型解释）定性说明"));
    assert_eq!(json["approvals_total"], 3);
    assert_eq!(json["succeeded_verified"], 3);
}

#[test]
fn des_eval_covers_planted_alarm_assets_in_evidence() {
    let truth = GroundTruth {
        mechanisms: Mechanisms {
            spindle_wear_to_dimension: true,
            ..Default::default()
        },
        alarms: vec![AlarmTruth {
            event_id: "vib-1".into(),
            kind: "vibration".into(),
            asset: "CNC-XX".into(),
            root_causes: vec!["spindle_wear:CNC-XX".into()],
        }],
        relations: vec![],
        contentions: vec![],
    };
    let drafts = analyze_adoption(&source_with_asset_case(), NOW, 1).unwrap();
    assert_adoption_stats(&drafts[0].stats, 1, 1, 0).unwrap();
    let report = coverage_against_truth(&truth, &drafts);
    assert_eq!(report.alarm_count, 1);
    assert!(
        report.assets_missing.is_empty(),
        "植入设备应被证据覆盖：{report:?}"
    );
    assert!(report.assets_mentioned.iter().any(|a| a == "CNC-XX"));
}

fn source_with_asset_case() -> MemorySource {
    MemorySource {
        source_id: "des-eval".into(),
        cases: vec![CaseSummary {
            case_id: "case-CNC-XX-planted".into(),
            template: "tpl-des".into(),
            scenario: "s1".into(),
            approvals: vec![ApprovalSummary {
                id: "p".into(),
                scope: "production".into(),
                approver: "s".into(),
                expires_at: 10_000,
                plan_revision: 1,
            }],
            actions: vec![ActionSummary {
                key: "a".into(),
                state: "ACCEPTED".into(),
                verification: Some("ok".into()),
                accepted_at: 1,
                complete_at: 2,
            }],
            event_times: vec![(1, 1)],
        }],
    }
}
