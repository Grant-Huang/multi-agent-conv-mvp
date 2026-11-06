//! 运行时集成用例 RT-TPL-01 ~ RT-TPL-09（定义见 docs/testing/CASES.md）。
//! 只通过公开接口（an_runtime::template）验证业务行为；不修改断言迎合实现。

use an_runtime::template::{
    assemble_join, decide_split, validate_envelope, validate_template, BranchEnvelope, RouteSlots,
    SplitDecision, Template, TemplateRegistry,
};
use serde_json::{json, Value};

/// 设计文档 9.6 中的 quote 模板原文
const QUOTE: &str = r#"{
  "id": "quote",
  "deliverable": "报价单",
  "required_slots": ["序列号或型号", "交期"],
  "branches": [
    { "id": "damage", "capability": "general",
      "serial_steps": ["读测量值", "对照修复限度", "给出可修结论和工时"],
      "output_required": ["verdict", "hours"] },
    { "id": "parts", "capability": "general",
      "serial_steps": ["取原厂清单", "对库存和替代件", "算缺件金额"],
      "output_required": ["shortage_amount"] },
    { "id": "history", "capability": "general",
      "serial_steps": ["查本机维修", "查同型号返修", "给保修风险"],
      "output_required": ["warranty_risk"] }
  ],
  "join": { "capability": "general", "writes_file": true }
}"#;

fn quote() -> Template {
    serde_json::from_str(QUOTE).expect("设计文档中的 quote 模板应能解析")
}

fn slots(template_id: &str, deliverable: &str, slots_ok: &str) -> RouteSlots {
    RouteSlots {
        template_id: template_id.into(),
        deliverable: deliverable.into(),
        slots_ok: slots_ok.into(),
    }
}

fn env(branch: &str, status: &str, missing: &str, payload: Value) -> BranchEnvelope {
    BranchEnvelope {
        branch_id: branch.into(),
        status: status.into(),
        missing: missing.into(),
        payload: serde_json::from_value(payload).expect("测试数据应为字符串映射"),
    }
}

#[test]
fn rt_tpl_01_quote_template_loads() {
    // RT-TPL-01：设计示例可解析，校验通过（分支数上限 4）
    let t = quote();
    assert_eq!(t.id, "quote");
    assert!(validate_template(&t, 4).is_ok());
    assert!(TemplateRegistry::new(vec![t], 4).is_ok());
}

#[test]
fn rt_tpl_02_capability_is_a_router_role() {
    // RT-TPL-02：能力名与 Router 角色同名
    let writer = QUOTE.replacen(
        r#""capability": "general","#,
        r#""capability": "writer","#,
        1,
    );
    assert!(
        serde_json::from_str::<Template>(&writer).is_err(),
        "writer 不是角色名，应解析失败"
    );
    for name in ["general", "coding", "reviewer"] {
        let ok = QUOTE.replacen(
            r#""capability": "general","#,
            &format!(r#""capability": "{name}","#),
            1,
        );
        assert!(
            serde_json::from_str::<Template>(&ok).is_ok(),
            "{name} 应解析成功"
        );
    }
}

#[test]
fn rt_tpl_03_dependency_edges_are_rejected() {
    // RT-TPL-03：模板不能藏依赖边
    let with_edge = QUOTE.replacen(
        r#""id": "parts","#,
        r#""id": "parts", "depends_on": ["damage"],"#,
        1,
    );
    assert!(serde_json::from_str::<Template>(&with_edge).is_err());
}

#[test]
fn rt_tpl_04_branches_must_fit_background_capacity() {
    // RT-TPL-04：分支数超过后台容量时不得注册
    let t = quote();
    let err = TemplateRegistry::new(vec![t.clone()], 2).expect_err("3 个分支超过容量 2，应被拒绝");
    assert!(err.iter().any(|p| p.contains("超过后台槽位数")), "{err:?}");
    assert!(TemplateRegistry::new(vec![t], 3).is_ok());
}

#[test]
fn rt_tpl_05_unclear_router_slots_stay_single() {
    // RT-TPL-05：只有 yes 且模板与交付物都匹配时才拆分，其余整单执行
    let mut reg_src = vec![quote()];
    let reg = TemplateRegistry::new(std::mem::take(&mut reg_src), 4).unwrap();
    assert_eq!(
        decide_split(&slots("quote", "报价单", "yes"), &reg),
        SplitDecision::Split {
            template_id: "quote".into()
        }
    );
    for v in ["no", "maybe", "", "YES"] {
        assert!(
            matches!(
                decide_split(&slots("quote", "报价单", v), &reg),
                SplitDecision::Single { .. }
            ),
            "slots_ok={v:?} 应整单执行"
        );
    }
    assert!(matches!(
        decide_split(&slots("", "", "yes"), &reg),
        SplitDecision::Single { .. }
    ));
    assert!(matches!(
        decide_split(&slots("stop_line", "停线清单", "yes"), &reg),
        SplitDecision::Single { .. }
    ));
    assert!(matches!(
        decide_split(&slots("quote", "放行单", "yes"), &reg),
        SplitDecision::Single { .. }
    ));
}

#[test]
fn rt_tpl_06_envelope_fields_must_match() {
    // RT-TPL-06：信封字段与分支声明完全一致
    let t = quote();
    let spec = &t.branches[0];
    assert!(validate_envelope(
        spec,
        &env("damage", "ok", "", json!({"verdict": "可修", "hours": "6"}))
    )
    .is_ok());
    assert!(
        validate_envelope(spec, &env("damage", "ok", "", json!({"verdict": "可修"}))).is_err(),
        "缺字段应失败"
    );
    assert!(
        validate_envelope(
            spec,
            &env(
                "damage",
                "ok",
                "",
                json!({"verdict": "可修", "hours": "6", "note": "x"})
            )
        )
        .is_err(),
        "多字段应失败"
    );
    assert!(
        validate_envelope(
            spec,
            &env("parts", "ok", "", json!({"verdict": "可修", "hours": "6"}))
        )
        .is_err(),
        "branch_id 不一致应失败"
    );
}

#[test]
fn rt_tpl_07_payload_values_must_be_strings() {
    // RT-TPL-07：payload 只接受字符串
    for bad in [
        json!({"hours": 6}),
        json!({"ok": true}),
        json!({"x": ["a"]}),
        json!({"x": {"a": "b"}}),
        json!({"x": null}),
    ] {
        let raw = json!({"branch_id": "damage", "status": "ok", "missing": "", "payload": bad});
        assert!(
            serde_json::from_value::<BranchEnvelope>(raw).is_err(),
            "{bad} 应被拒绝"
        );
    }
    let good =
        json!({"branch_id": "damage", "status": "ok", "missing": "", "payload": {"hours": "6"}});
    assert!(serde_json::from_value::<BranchEnvelope>(good).is_ok());
}

#[test]
fn rt_tpl_08_missing_branches_are_not_filled() {
    // RT-TPL-08：缺席或无效的分支记为 missing，payload 为空，不用其他数据填补
    let t = quote();
    let envs = vec![
        env("damage", "ok", "", json!({"verdict": "可修", "hours": "6"})),
        env("history", "ok", "", json!({})), // status 为 ok 却缺少 warranty_risk，应失败
    ];
    let j = assemble_join(&t, &envs).expect("未知或重复的 id 才报错，缺席与无效应降级");
    assert!(!j.complete);
    assert_eq!(
        j.missing_branches,
        vec!["parts".to_string(), "history".to_string()]
    );
    let parts = j.branches.iter().find(|b| b.branch_id == "parts").unwrap();
    assert_eq!(parts.status, "missing");
    assert!(!parts.missing.trim().is_empty(), "缺失原因不能为空");
    assert!(parts.payload.is_empty());
    let history = j
        .branches
        .iter()
        .find(|b| b.branch_id == "history")
        .unwrap();
    assert_eq!(history.status, "missing");
    assert!(history.payload.is_empty());
    assert!(
        history.missing.contains("缺少字段 warranty_risk"),
        "{}",
        history.missing
    );
}

#[test]
fn rt_tpl_09_join_rejects_unknown_or_duplicate() {
    // RT-TPL-09：未知或重复的 branch_id 是调度层错误，整体报错
    let t = quote();
    let unknown = vec![env("ghost", "ok", "", json!({"x": "1"}))];
    assert!(assemble_join(&t, &unknown).is_err());
    let dup = vec![
        env("parts", "ok", "", json!({"shortage_amount": "1"})),
        env("parts", "ok", "", json!({"shortage_amount": "2"})),
    ];
    assert!(assemble_join(&t, &dup).is_err());
}

#[test]
fn rt_tpl_08_all_branches_ok_completes_join() {
    // RT-TPL-08 的正向对照：全部 ok 时汇合完整，输出顺序与模板一致
    let t = quote();
    let envs = vec![
        env("history", "ok", "", json!({"warranty_risk": "低"})),
        env("damage", "ok", "", json!({"verdict": "可修", "hours": "6"})),
        env("parts", "ok", "", json!({"shortage_amount": "1200"})),
    ];
    let j = assemble_join(&t, &envs).unwrap();
    assert!(j.complete);
    let order: Vec<&str> = j.branches.iter().map(|b| b.branch_id.as_str()).collect();
    assert_eq!(order, ["damage", "parts", "history"]);
}
