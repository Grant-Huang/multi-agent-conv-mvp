//! RT-BIND-01..09：专家工作流节点绑定与模板晋升（用例见 docs/testing/CASES.md）。

use an_runtime::mom::binding::*;
use serde_json::{json, Value};

/// 用 JSON 构造一条工作流，节点之间默认按 normal 边顺序相连（可另给 edges 覆盖）
fn wf(nodes: Value, edges: Option<Value>) -> ExpertWorkflow {
    let nodes_arr = nodes.as_array().unwrap().clone();
    let edges = edges.unwrap_or_else(|| {
        let ids: Vec<_> = nodes_arr.iter().map(|n| n["node_id"].clone()).collect();
        Value::Array(
            ids.windows(2)
                .map(|w| json!({"from": w[0], "to": w[1], "edge_type": "normal"}))
                .collect(),
        )
    });
    serde_json::from_value(json!({
        "id": "t", "name": "测试", "status": "expert_confirmed",
        "nodes": nodes_arr, "edges": edges
    }))
    .unwrap()
}

fn node(id: &str, ty: &str, label: &str, roles: &[&str], links: Value) -> Value {
    json!({"node_id": id, "node_type": ty, "label": label, "actor_roles": roles,
           "object_links": links})
}

fn bind(w: &ExpertWorkflow, id: &str) -> NodeBinding {
    let n = w.nodes.iter().find(|n| n.node_id == id).unwrap();
    bind_node(w, n)
}

fn real_workflows() -> Vec<ExpertWorkflow> {
    serde_json::from_str(include_str!("../../../fixtures/mom/expert/workflows.json")).unwrap()
}

#[test]
fn rt_bind_01_approval_is_human_gate() {
    let mut n = node("a", "approval", "质量经理签字", &["质量经理"], json!([]));
    n["sla_config"] = json!({"duration": "PT24H", "type": "deadline"});
    let w = wf(
        json!([
            n,
            node(
                "x",
                "activity",
                "红牌14天未处置自动升级到质量总监",
                &["质量总监"],
                json!([])
            )
        ]),
        Some(json!([{"from": "a", "to": "x", "edge_type": "timeout", "condition": "超时"}])),
    );
    let b = bind(&w, "a");
    assert_eq!(b.executor, Executor::Human);
    assert!(b.rules.contains(&"R1".to_string()));
    let g = b.gate.expect("审批节点必须有闸门");
    assert_eq!(g.kind, GateKind::Approval);
    assert_eq!(g.approvers, vec!["质量经理".to_string()]);
    assert_eq!(g.deadline.as_deref(), Some("PT24H"));
    assert_eq!(
        g.escalation_to,
        vec!["红牌14天未处置自动升级到质量总监".to_string()]
    );
    // 即使审批节点挂了系统机制，也不能交给 agent 或 system
    let w2 = wf(
        json!([node(
            "a",
            "approval",
            "IQC签字放行",
            &["IQC"],
            json!([{"role": "mechanism", "name": "ERP签字模块", "entity_type": "system"}])
        )]),
        None,
    );
    assert_eq!(bind(&w2, "a").executor, Executor::Human);
}

#[test]
fn rt_bind_02_physical_work_is_human() {
    let w = wf(
        json!([
            node(
                "e",
                "activity",
                "用XRF枪打牌号",
                &["检验员"],
                json!([{"role": "mechanism", "name": "XRF枪", "entity_type": "equipment"}])
            ),
            node(
                "p",
                "activity",
                "搬到待检区、贴黄色Hold标识",
                &["收货员"],
                json!([])
            ),
            node("s", "activity", "停线", &[], json!([])),
        ]),
        None,
    );
    for id in ["e", "p", "s"] {
        let b = bind(&w, id);
        assert_eq!(b.executor, Executor::Human, "{id}");
        assert!(b.rules.contains(&"R2".to_string()), "{id}");
    }
}

#[test]
fn rt_bind_03_system_work_is_agent_with_tools() {
    let w = wf(
        json!([
            node(
                "r",
                "activity",
                "查看现货、在制和已锁定库存",
                &["计划员"],
                json!([{"role": "mechanism", "name": "ERP", "entity_type": "system"}])
            ),
            node(
                "w",
                "activity",
                "收货检验报告当日17:00前上传ERP",
                &["IQC 检验员"],
                json!([{"role": "mechanism", "name": "ERP-QM模块", "entity_type": "system"}])
            ),
        ]),
        None,
    );
    let r = bind(&w, "r");
    assert_eq!(r.executor, Executor::Agent);
    assert_eq!(r.tools, vec!["erp".to_string()]);
    assert!(r.write_tools.is_empty());
    assert!(r.gate.is_none());
    assert!(r.rules.contains(&"R3".to_string()));
    let x = bind(&w, "w");
    assert_eq!(x.executor, Executor::Agent);
    assert_eq!(x.write_tools, vec!["erp".to_string()]);
    assert_eq!(
        x.gate.expect("写操作必须带闸门").kind,
        GateKind::ActionApproval
    );
}

#[test]
fn rt_bind_04_decisions_rule_or_human_confirm() {
    let mut d1 = node(
        "d1",
        "decision",
        "XRF读数是否落在牌号±0.3%边界值",
        &["检验员"],
        json!([]),
    );
    d1["decision_question"] = json!("XRF读数是否落在牌号±0.3%边界值？");
    let mut d2 = node("d2", "decision", "过程是否稳定？", &[], json!([]));
    d2["decision_question"] = json!("过程是否稳定？");
    let w = wf(json!([d1, d2]), Some(json!([])));
    let a = bind(&w, "d1");
    assert_eq!(a.executor, Executor::System);
    assert!(a.rules.contains(&"R4".to_string()));
    let b = bind(&w, "d2");
    assert_eq!(b.executor, Executor::AgentThenHuman);
    assert!(b.rules.contains(&"R5".to_string()));
}

#[test]
fn rt_bind_05_controls_become_skills_and_roles_map_domains() {
    let w = wf(
        json!([
            node(
                "q",
                "activity",
                "确认失控",
                &["IPQC"],
                json!([{"role": "control", "name": "Western Electric规则", "entity_type": "document",
                        "document_kind": "standard"},
                       {"role": "mechanism", "name": "控制图软件", "entity_type": "system"},
                       {"role": "input", "name": "控制图"}, {"role": "output", "name": "失控判定"}])
            ),
            node(
                "m",
                "activity",
                "在CMMS里登记保养结果",
                &["维修工"],
                json!([{"role": "mechanism", "name": "CMMS系统", "entity_type": "system"}])
            ),
            node(
                "p",
                "activity",
                "核对产品编码、版本、数量和承诺交期",
                &["计划员"],
                json!([{"role": "mechanism", "name": "APS系统", "entity_type": "system"}])
            ),
        ]),
        None,
    );
    let q = bind(&w, "q");
    assert_eq!(q.skills, vec!["Western Electric规则".to_string()]);
    assert!(q.rules.contains(&"R6".to_string()));
    assert_eq!(q.domain.as_deref(), Some("质量"));
    assert_eq!(q.tools, vec!["spc".to_string()]);
    assert_eq!(q.inputs, vec!["控制图".to_string()]);
    assert_eq!(q.outputs, vec!["失控判定".to_string()]);
    assert_eq!(bind(&w, "m").domain.as_deref(), Some("设备"));
    assert_eq!(bind(&w, "p").domain.as_deref(), Some("计划"));
}

#[test]
fn rt_bind_06_unbound_with_reason_not_guessed() {
    let w = wf(
        json!([
            node("u", "activity", "解锁", &[], json!([])),
            node(
                "k",
                "activity",
                "在XYZ平台登记",
                &["计划员"],
                json!([{"role": "mechanism", "name": "XYZ平台", "entity_type": "system"}])
            ),
        ]),
        None,
    );
    for id in ["u", "k"] {
        let b = bind(&w, id);
        assert_eq!(b.executor, Executor::Unbound, "{id}");
        assert!(b.rules.contains(&"R8".to_string()), "{id}");
        assert!(!b.needs_expert.is_empty(), "{id} 必须说明原因");
    }
}

#[test]
fn rt_bind_07_segments_merge_same_domain_agents() {
    let sys = |n: &str| json!([{"role": "mechanism", "name": n, "entity_type": "system"}]);
    let mut d = node("d", "decision", "是否需要100%检验？", &[], json!([]));
    d["decision_question"] = json!("是否需要100%检验？");
    let w = wf(
        json!([
            node("s", "start", "开始", &[], json!([])),
            node(
                "a",
                "activity",
                "查询批次检验记录",
                &["IPQC"],
                sys("MES系统")
            ),
            node("b", "activity", "汇总不良数据", &["IPQC"], sys("SPC看板")),
            node("h", "activity", "停线", &["班组长"], json!([])),
            node("c", "activity", "记录处理过程", &["IPQC"], sys("MES系统")),
            d,
            node(
                "x",
                "activity",
                "对受影响产品100%检验",
                &["检验员"],
                json!([])
            ),
            node("e", "end", "结束", &[], json!([])),
        ]),
        Some(json!([
            {"from": "s", "to": "a", "edge_type": "normal"},
            {"from": "a", "to": "b", "edge_type": "normal"},
            {"from": "b", "to": "h", "edge_type": "normal"},
            {"from": "h", "to": "c", "edge_type": "normal"},
            {"from": "c", "to": "d", "edge_type": "normal"},
            {"from": "d", "to": "x", "edge_type": "conditional", "condition": "必要时"},
            {"from": "d", "to": "e", "edge_type": "conditional", "condition": "非必要时"},
            {"from": "x", "to": "e", "edge_type": "normal"}
        ])),
    );
    let t = compile(&w, false);
    let seg_of = |id: &str| {
        t.segments
            .iter()
            .find(|s| s.node_ids.contains(&id.to_string()))
            .unwrap()
            .clone()
    };
    let ab = seg_of("a");
    assert_eq!(ab.node_ids, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(
        ab.steps,
        vec!["查询批次检验记录".to_string(), "汇总不良数据".to_string()]
    );
    assert_eq!(ab.executor, Executor::Agent);
    assert_eq!(ab.domain.as_deref(), Some("质量"));
    assert_eq!(ab.tools, vec!["mes".to_string(), "spc".to_string()]);
    assert_eq!(seg_of("h").node_ids, vec!["h".to_string()]);
    assert_ne!(seg_of("c").id, ab.id);
    // 条件边保留为段间路由
    let (sd, sx) = (seg_of("d").id, seg_of("x").id);
    assert!(t
        .edges
        .iter()
        .any(|e| e.from == sd && e.to == sx && e.condition.as_deref() == Some("必要时")));
    assert_eq!(validate_workflow_template(&t), Ok(()));
}

#[test]
fn rt_bind_08_real_expert_workflows() {
    let wfs = real_workflows();
    assert_eq!(wfs.len(), 9);
    let mut total_nodes = 0;
    for w in &wfs {
        let t = compile(w, false);
        // 每个节点恰好一张绑定卡、恰好属于一段
        assert_eq!(t.nodes.len(), w.nodes.len(), "{}", w.name);
        for n in &w.nodes {
            let in_segments = t
                .segments
                .iter()
                .filter(|s| s.node_ids.contains(&n.node_id))
                .count();
            assert_eq!(in_segments, 1, "{} {}", w.name, n.node_id);
        }
        for b in &t.nodes {
            if b.node_type == "approval" {
                assert_eq!(b.executor, Executor::Human, "{} {}", w.name, b.label);
            }
            assert!(
                !b.rules.is_empty() || b.executor == Executor::Structural,
                "{}",
                b.label
            );
        }
        assert_eq!(validate_workflow_template(&t), Ok(()), "{}", w.name);
        // 绑定未经专家确认，一律不晋升
        assert_ne!(t.promotion, "promoted", "{}", w.name);
        assert!(!t.promotion.is_empty());
        total_nodes += w.nodes.len();
    }
    let r = promotion_report(&wfs);
    assert_eq!(r["funnel"]["workflows"], 9);
    assert_eq!(r["funnel"]["promoted"], 0);
    let per: u64 = r["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["nodes"].as_u64().unwrap())
        .sum();
    assert_eq!(per as usize, total_nodes);
    assert_eq!(r["totals"]["nodes"].as_u64().unwrap() as usize, total_nodes);
    let by_exec: u64 = r["totals"]["by_executor"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(by_exec as usize, total_nodes);
}

#[test]
fn rt_bind_09_validator_rejects_bad_templates() {
    let sys = json!([{"role": "mechanism", "name": "ERP", "entity_type": "system"}]);
    let w = wf(
        json!([
            node("a", "activity", "查看库存", &["计划员"], sys.clone()),
            node("b", "approval", "计划经理签字", &["计划经理"], json!([])),
            node("c", "activity", "下达工单到ERP", &["计划员"], sys),
        ]),
        None,
    );
    let good = compile(&w, false);
    assert_eq!(validate_workflow_template(&good), Ok(()));

    let expect_err = |t: WorkflowTemplate, what: &str| {
        let e = validate_workflow_template(&t).expect_err(what);
        assert!(!e.is_empty(), "{what}");
    };
    // 段间有环
    let mut t = good.clone();
    let (first, last) = (
        t.segments[0].id.clone(),
        t.segments.last().unwrap().id.clone(),
    );
    t.edges.push(SegmentEdge {
        from: last,
        to: first,
        edge_type: "normal".into(),
        condition: None,
    });
    expect_err(t, "环");
    // 写工具没有闸门
    let mut t = good.clone();
    for s in t.segments.iter_mut().filter(|s| !s.write_tools.is_empty()) {
        s.gates.clear();
    }
    for n in t.nodes.iter_mut().filter(|n| !n.write_tools.is_empty()) {
        n.gate = None;
    }
    expect_err(t, "写工具无闸门");
    // 工具不在清单
    let mut t = good.clone();
    t.segments[0].tools.push("unknown_system".into());
    expect_err(t, "未知工具");
    // 审批绑成 agent
    let mut t = good.clone();
    for n in t.nodes.iter_mut().filter(|n| n.node_type == "approval") {
        n.executor = Executor::Agent;
    }
    expect_err(t, "审批绑成 agent");
    // 节点遗漏
    let mut t = good;
    t.segments
        .retain(|s| !s.node_ids.contains(&"b".to_string()));
    expect_err(t, "节点遗漏");
}

#[test]
fn rt_bind_10_leading_info_verb_auto_actions_and_roleless_info() {
    let w = wf(
        json!([
            node(
                "o",
                "activity",
                "输出保养记录和下次保养周期",
                &["维修工"],
                json!([])
            ),
            node("s", "activity", "自动锁批次", &[], json!([])),
            node(
                "c",
                "activity",
                "确认失控",
                &[],
                json!([{"role": "control", "name": "Western Electric规则", "entity_type": "document"}])
            ),
            node("m", "activity", "C档每月换滤芯", &["维修工"], json!([])),
        ]),
        None,
    );
    let o = bind(&w, "o");
    assert_eq!(o.executor, Executor::Agent, "开头的信息动作优先于物理词");
    let s = bind(&w, "s");
    assert_eq!(s.executor, Executor::System);
    assert!(s.rules.contains(&"R4".to_string()));
    let c = bind(&w, "c");
    assert_eq!(c.executor, Executor::Agent);
    assert!(c.domain.is_none());
    assert!(
        c.needs_expert.iter().any(|r| r.contains("领域")),
        "{:?}",
        c.needs_expert
    );
    assert_eq!(bind(&w, "m").executor, Executor::Human);
}
