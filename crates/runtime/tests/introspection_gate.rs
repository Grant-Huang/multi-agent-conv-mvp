//! 业务内省 S3：发送闸门（RT-INT-16/17/18/19/20/22）。
//! 见 docs/memory/INTROSPECTION_PLAYBOOK.md §3。

use an_runtime::introspect::gate::{decide, GateConfig, GateInput};
use an_runtime::introspect::Verification;
use an_runtime::store::Store;
use std::path::Path;

fn cfg() -> GateConfig {
    GateConfig::default()
}

fn base(ver: Verification) -> GateInput {
    GateInput {
        finding_id: "f1".into(),
        verification: ver,
        impact_cases: 0,
        has_next_step: false,
        first_or_worsened: false,
        time_sensitive: false,
        already_pushed_unread: false,
        pushed_today: 0,
        model_delta: 0,
        model_reason: String::new(),
        extra_pending: 0,
        interrupt_eligible: true,
    }
}

/// 满分规则侧：Supported(+2) + 影响面(+1) + 下一步(+1) + 首次(+1) + 时效(+1) = 6
fn strong() -> GateInput {
    GateInput {
        finding_id: "strong".into(),
        verification: Verification::Supported,
        impact_cases: 5,
        has_next_step: true,
        first_or_worsened: true,
        time_sensitive: true,
        already_pushed_unread: false,
        pushed_today: 0,
        model_delta: 0,
        model_reason: String::new(),
        extra_pending: 0,
        interrupt_eligible: true,
    }
}

#[test]
fn rt_int_16_none_pass_gate_when_all_weak() {
    let items = [
        base(Verification::Inconclusive),
        GateInput {
            impact_cases: 1,
            has_next_step: true,
            ..base(Verification::Supported)
        }, // 规则分 2+1=3 < 4
        GateInput {
            already_pushed_unread: true,
            ..strong()
        },
    ];
    for (i, inp) in items.iter().enumerate() {
        let d = decide(inp, &cfg());
        assert!(!d.should_push, "第 {i} 条不应推送");
        assert!(!d.reason.is_empty(), "第 {i} 条必须有未推送原因");
    }
}

#[test]
fn rt_int_17_supported_with_impact_and_next_step_pushes_inconclusive_does_not() {
    // Supported(+2) + 影响面(+1) + 下一步(+1) = 4，刚好达阈值
    let pass = GateInput {
        finding_id: "pass".into(),
        verification: Verification::Supported,
        impact_cases: 3,
        has_next_step: true,
        first_or_worsened: false,
        time_sensitive: false,
        already_pushed_unread: false,
        pushed_today: 0,
        model_delta: 0,
        model_reason: String::new(),
        extra_pending: 0,
        interrupt_eligible: true,
    };
    let fail = base(Verification::Inconclusive);
    let a = decide(&pass, &cfg());
    let b = decide(&fail, &cfg());
    assert!(a.should_push, "前者应推送：{}", a.reason);
    assert!(a.reason.contains("推送"));
    assert!(!b.should_push, "证据不足不推送");
    assert!(!b.reason.is_empty());
}

#[test]
fn rt_int_18_model_cannot_alone_cross_threshold_and_blank_reason_ignored() {
    // 规则分 3（Supported+下一步），模型 +1 想越到 4 → 仍不推
    let alone = GateInput {
        finding_id: "alone".into(),
        verification: Verification::Supported,
        impact_cases: 0,
        has_next_step: true,
        first_or_worsened: false,
        time_sensitive: false,
        already_pushed_unread: false,
        pushed_today: 0,
        model_delta: 1,
        model_reason: "我觉得很重要".into(),
        extra_pending: 0,
        interrupt_eligible: true,
    };
    let d = decide(&alone, &cfg());
    assert!(!d.should_push);
    assert!(
        d.reason.contains("不能单独越阈") || d.reason.contains("模型不能单独"),
        "reason={}",
        d.reason
    );

    // 规则分已 ≥ 阈值；模型想 -1 但无理由 → 忽略，仍推
    let mut keep = strong();
    keep.model_delta = -1;
    keep.model_reason = String::new();
    let d2 = decide(&keep, &cfg());
    assert!(d2.should_push, "无理由压分应忽略：{}", d2.reason);
    assert!(d2.score >= 4.0);

    // 规则分已 ≥ 阈值；模型有理由 -1 且压到阈下 → 不推，reason 含模型理由
    let press = GateInput {
        finding_id: "press".into(),
        verification: Verification::Supported,
        impact_cases: 3,
        has_next_step: true,
        first_or_worsened: false,
        time_sensitive: false,
        already_pushed_unread: false,
        pushed_today: 0,
        model_delta: -1,
        model_reason: "不相关".into(),
        extra_pending: 0,
        interrupt_eligible: true,
    };
    // rules=4, score=3 → 不推
    let d3 = decide(&press, &cfg());
    assert!(!d3.should_push);
    assert!(d3.reason.contains("不相关"), "reason={}", d3.reason);
}

#[test]
fn rt_int_19_already_pushed_unread_is_not_re_pushed() {
    let mut inp = strong();
    inp.already_pushed_unread = true;
    let d = decide(&inp, &cfg());
    assert!(!d.should_push);
    assert!(d.reason.contains("未读") || d.reason.contains("不重复"));
}

#[test]
fn rt_int_20_daily_cap_blocks_with_extra_pending_hint() {
    let mut inp = strong();
    inp.pushed_today = 1;
    inp.extra_pending = 3;
    let d = decide(&inp, &cfg());
    assert!(!d.should_push);
    assert!(d.reason.contains("日上限"), "reason={}", d.reason);
    assert!(d.reason.contains("另有 3 项"), "reason={}", d.reason);
}

#[test]
fn investigate_worthy_without_interrupt_stays_on_page() {
    let mut inp = strong();
    inp.interrupt_eligible = false;
    let d = decide(&inp, &cfg());
    assert!(!d.should_push);
    assert!(
        d.reason.contains("仅内省页") || d.reason.contains("不主动打扰"),
        "reason={}",
        d.reason
    );
}

#[test]
fn rt_int_22_labels_do_not_mutate_gate_config() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let before = cfg();
    store
        .introspection_add_label("f1", "should_push", "很有用")
        .unwrap();
    store
        .introspection_add_label("f1", "should_not_push", "太吵")
        .unwrap();
    let kinds: Vec<_> = store
        .introspection_labels("f1")
        .unwrap()
        .into_iter()
        .map(|l| l.kind)
        .collect();
    assert_eq!(kinds, ["should_push", "should_not_push"]);
    let after = cfg();
    assert_eq!(before, after, "标注不得改闸门配置");
    // 再跑一次 decide，配置对象仍不变
    let _ = decide(&strong(), &after);
    assert_eq!(before, after);
}
