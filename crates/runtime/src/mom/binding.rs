//! 专家工作流节点绑定与模板晋升（E1-C 方案 B）。
//!
//! 输入：data-collection 中"专家确认"的工作流图（节点、边、ICOM 对象链接）。
//! 输出：带节点绑定的工作流模板——每个节点由谁做（执行者）、属于哪个领域、能用哪些工具、
//! 加载哪些技能、受什么闸门约束；相邻同领域的 agent 节点合并为一段。
//!
//! 只做确定性的编译、校验和统计，不调用模型，也不执行任务。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// data-collection 导出的一条工作流
#[derive(Clone, Debug, Deserialize)]
pub struct ExpertWorkflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: String,
    pub nodes: Vec<ExpertNode>,
    pub edges: Vec<ExpertEdge>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExpertNode {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    #[serde(default)]
    pub actor_roles: Vec<String>,
    #[serde(default)]
    pub decision_question: Option<String>,
    #[serde(default)]
    pub sla_config: Option<Value>,
    #[serde(default)]
    pub retry_semantics: Option<Value>,
    #[serde(default)]
    pub object_links: Vec<ObjectLink>,
}

/// ICOM 对象链接：role 为 input / control / output / mechanism
#[derive(Clone, Debug, Deserialize)]
pub struct ObjectLink {
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub entity_type: Option<String>,
    #[serde(default)]
    pub document_kind: Option<String>,
    #[serde(default)]
    pub qualifier: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExpertEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub edge_type: String,
    #[serde(default)]
    pub condition: Option<String>,
}

/// 节点由谁执行
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    /// 信息类工作，由领域 agent 执行
    Agent,
    /// 物理动作或签字，由人执行并回执
    Human,
    /// 可编译为确定性规则的判断
    System,
    /// agent 给出判断与证据，人确认
    AgentThenHuman,
    /// 开始、结束、合并等结构节点
    Structural,
    /// 数据不足，无法绑定
    Unbound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// 审批节点本身
    Approval,
    /// 写操作工具在执行前需要批准
    ActionApproval,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub kind: GateKind,
    pub approvers: Vec<String>,
    /// ISO 8601 时长，来自 sla_config.duration
    pub deadline: Option<String>,
    /// 超时边指向的节点标签
    pub escalation_to: Vec<String>,
}

/// 一个节点的绑定卡
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeBinding {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    pub executor: Executor,
    pub domain: Option<String>,
    /// 归一后的系统工具 ID（见 [`TOOL_CATALOG`]）
    pub tools: Vec<String>,
    /// tools 中需要写入的那部分
    pub write_tools: Vec<String>,
    pub skills: Vec<String>,
    pub gate: Option<Gate>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// 命中的规则编号（R1–R8）
    pub rules: Vec<String>,
    /// 需要专家补充的内容；为空表示绑定已完整
    pub needs_expert: Vec<String>,
}

/// 合并后的一段
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub executor: Executor,
    pub domain: Option<String>,
    pub node_ids: Vec<String>,
    /// 段内各节点标签，按执行顺序
    pub steps: Vec<String>,
    pub tools: Vec<String>,
    pub write_tools: Vec<String>,
    pub skills: Vec<String>,
    pub gates: Vec<Gate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentEdge {
    pub from: String,
    pub to: String,
    pub edge_type: String,
    pub condition: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    pub source_workflow: String,
    pub nodes: Vec<NodeBinding>,
    pub segments: Vec<Segment>,
    pub edges: Vec<SegmentEdge>,
    /// 图中的并行分支数（parallel 边）；为 0 时不生成 §9.6 并行模板
    pub parallel_branches: usize,
    /// promoted / pending_expert_confirmation / needs_completion / invalid
    pub promotion: String,
    pub promotion_reasons: Vec<String>,
}

/// 工具清单：(工具 ID, 识别关键词, 默认领域；空字符串表示不推断领域)。
/// 系统名称按关键词归一，例如"ERP / ERP 签字模块 / ERP-QM模块"都归为 `erp`；清单外的系统不绑定。
pub const TOOL_CATALOG: &[(&str, &[&str], &str)] = &[
    ("mes", &["MES"], "生产"),
    ("erp", &["ERP"], ""),
    ("wms", &["WMS", "仓储系统"], "采购物流"),
    ("plm", &["PLM", "图纸库"], "工程"),
    ("cmms", &["CMMS", "设备管理系统"], "设备"),
    ("aps", &["APS", "排程"], "计划"),
    ("spc", &["SPC", "控制图"], "质量"),
    ("logistics", &["物流系统"], "采购物流"),
];

/// 执行角色 → 领域（R7）。不在表里的角色不推断领域。
const ROLE_DOMAINS: &[(&str, &[&str])] = &[
    (
        "质量",
        &[
            "检验员",
            "IPQC",
            "IQC",
            "IQC 检验员",
            "OQC",
            "质量",
            "质量经理",
            "质量工程师",
            "质量总监",
            "QE",
            "品质",
        ],
    ),
    (
        "设备",
        &["维修工", "维修", "设备", "设备工程师", "设备主管"],
    ),
    (
        "计划",
        &["计划员", "车间调度", "调度", "PMC", "生管", "计划经理"],
    ),
    (
        "生产",
        &["操作员", "操作工", "班组长", "车间主任", "生产", "生产主管"],
    ),
    (
        "采购物流",
        &["采购", "采购跟单员", "收货员", "仓库", "仓管", "货车司机"],
    ),
    ("工程", &["工程", "工艺工程师", "工程师"]),
    ("销售", &["销售"]),
];

/// 物理动作（R2）：出现即交给人执行并回执
const PHYSICAL_VERBS: &[&str] = &[
    "搬",
    "贴",
    "复测",
    "停线",
    "挂",
    "隔离",
    "拆",
    "安装",
    "更换",
    "清洁",
    "润滑",
    "紧固",
    "点检",
    "扫码",
    "打牌号",
    "量尺寸",
    "测量",
    "外观检验",
    "100%检验",
    "全检",
    "抽检",
    "抽样",
    "目视",
    "送检",
    "取样",
    "调试",
    "试切",
    "装夹",
    "上料",
    "下料",
    "返工",
    "报废",
    "加工",
    "量具",
    "目测",
    "外观",
    "测试",
    "换",
    "保养",
    "大修",
    "维修",
    "称重",
    "入库",
    "出库",
    "清线",
    "巡检",
    "锁机",
    "包装",
    "装箱",
];

/// 写操作（R3）：对外部系统有副作用，执行前需要批准
const WRITE_VERBS: &[&str] = &[
    "上传", "解锁", "下达", "签字", "签名", "录入", "登记", "生成", "发布", "创建", "开具", "提交",
    "下发", "更新", "标注",
];

/// 信息类动作：有角色、没有链接系统时，判为 agent 但需要专家指定工具
const INFO_VERBS: &[&str] = &[
    "查", "核对", "确认", "判断", "记录", "计算", "分析", "汇总", "评估", "通知", "审核", "比对",
    "对比", "统计", "编制", "制定", "排", "发起", "关联", "整理", "输出", "接单", "归档", "标记",
    "寻找",
];

/// 判断节点里表示"有明确规则"的记号（R4）；另外任何阿拉伯数字也算
const EXPLICIT_RULE_MARKS: &[&str] =
    &["%", "±", "≥", "≤", "关键词", "超过", "大于", "小于", "以内"];

const STRUCTURAL_TYPES: &[&str] = &["start", "end", "merge", "parallel_split", "handoff", "wait"];

fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

fn push_unique(v: &mut Vec<String>, s: &str) {
    if !s.is_empty() && !v.iter().any(|x| x == s) {
        v.push(s.to_string());
    }
}

/// 把系统名称归一为工具 ID；不在清单里返回 None
pub fn tool_for_system(name: &str) -> Option<&'static str> {
    let upper = name.to_uppercase();
    TOOL_CATALOG
        .iter()
        .find(|(_, keys, _)| keys.iter().any(|k| upper.contains(&k.to_uppercase())))
        .map(|(id, _, _)| *id)
}

fn is_known_tool(id: &str) -> bool {
    TOOL_CATALOG.iter().any(|(t, _, _)| *t == id)
}

fn tool_domain(id: &str) -> Option<String> {
    TOOL_CATALOG
        .iter()
        .find(|(t, _, d)| *t == id && !d.is_empty())
        .map(|(_, _, d)| d.to_string())
}

/// 角色 → 领域。多个角色属于不同领域时返回 None（例如三方会签）。
fn role_domain(roles: &[String]) -> Option<String> {
    let mut found: Vec<&str> = vec![];
    for r in roles {
        let r = r.trim();
        if let Some((d, _)) = ROLE_DOMAINS.iter().find(|(_, names)| names.contains(&r)) {
            if !found.contains(d) {
                found.push(d);
            }
        }
    }
    (found.len() == 1).then(|| found[0].to_string())
}

fn links<'a>(node: &'a ExpertNode, role: &'a str) -> impl Iterator<Item = &'a ObjectLink> {
    node.object_links.iter().filter(move |l| l.role == role)
}

fn is_mechanism(node: &ExpertNode, entity: &str) -> bool {
    links(node, "mechanism").any(|l| l.entity_type.as_deref() == Some(entity))
}

/// 给单个节点生成绑定卡（规则 R1–R8，见 docs/mom/EXPERT_BINDING.md）
pub fn bind_node(wf: &ExpertWorkflow, node: &ExpertNode) -> NodeBinding {
    let mut b = NodeBinding {
        node_id: node.node_id.clone(),
        node_type: node.node_type.clone(),
        label: node.label.clone(),
        executor: Executor::Unbound,
        domain: None,
        tools: vec![],
        write_tools: vec![],
        skills: vec![],
        gate: None,
        inputs: links(node, "input").map(|l| l.name.clone()).collect(),
        outputs: links(node, "output").map(|l| l.name.clone()).collect(),
        rules: vec![],
        needs_expert: vec![],
    };
    if STRUCTURAL_TYPES.contains(&node.node_type.as_str()) {
        b.executor = Executor::Structural;
        return b;
    }
    let label = node.label.as_str();
    // 系统机制 → 工具；清单外的系统记下来
    let mut unknown_systems = vec![];
    for l in links(node, "mechanism").filter(|l| l.entity_type.as_deref() == Some("system")) {
        match tool_for_system(&l.name) {
            Some(id) => push_unique(&mut b.tools, id),
            None => unknown_systems.push(l.name.clone()),
        }
    }
    b.tools.sort();

    match node.node_type.as_str() {
        // R1：审批永远由人完成，agent 至多准备材料
        "approval" => {
            b.executor = Executor::Human;
            b.rules.push("R1".into());
            b.tools.clear();
            let escalation_to = wf
                .edges
                .iter()
                .filter(|e| e.from == node.node_id && e.edge_type == "timeout")
                .filter_map(|e| wf.nodes.iter().find(|n| n.node_id == e.to))
                .map(|n| n.label.clone())
                .collect();
            let deadline = node
                .sla_config
                .as_ref()
                .and_then(|s| s.get("duration"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if node.actor_roles.is_empty() {
                b.needs_expert.push("审批人未指定".into());
            }
            b.gate = Some(Gate {
                kind: GateKind::Approval,
                approvers: node.actor_roles.clone(),
                deadline,
                escalation_to,
            });
        }
        // R4 / R5：判断节点
        "decision" => {
            let mut text = format!(
                "{}{}",
                label,
                node.decision_question.as_deref().unwrap_or("")
            );
            for l in links(node, "control") {
                text.push_str(&l.name);
                text.push_str(l.qualifier.as_deref().unwrap_or(""));
            }
            let explicit = text.chars().any(|c| c.is_ascii_digit())
                || contains_any(&text, EXPLICIT_RULE_MARKS);
            if explicit {
                b.executor = Executor::System;
                b.rules.push("R4".into());
            } else {
                b.executor = Executor::AgentThenHuman;
                b.rules.push("R5".into());
            }
        }
        // 活动及其他工作节点
        _ => {
            let roles = !node.actor_roles.is_empty();
            // 标签以信息/写操作动词开头时（如"输出保养记录"），不按物理词判为人工
            let leading_info = INFO_VERBS
                .iter()
                .chain(WRITE_VERBS)
                .any(|v| label.starts_with(v));
            let info_work = contains_any(label, INFO_VERBS)
                || contains_any(label, WRITE_VERBS)
                || (label.contains('单')
                    && ['打', '开', '改', '接'].iter().any(|c| label.contains(*c)));
            if label.contains("自动") {
                // R4：系统自动执行的动作（定时升级、自动锁批次等）
                b.executor = Executor::System;
                b.rules.push("R4".into());
            } else if is_mechanism(node, "equipment")
                || (!leading_info
                    && b.tools.is_empty()
                    && unknown_systems.is_empty()
                    && contains_any(label, PHYSICAL_VERBS))
            {
                // 专家标了系统机制时按系统工作处理，物理词只在没有系统机制时生效
                // R2：物理动作交给人
                b.executor = Executor::Human;
                b.rules.push("R2".into());
                b.tools.clear();
            } else if !unknown_systems.is_empty() {
                b.executor = Executor::Unbound;
                b.rules.push("R8".into());
                b.tools.clear();
                b.needs_expert
                    .push(format!("系统不在工具清单：{}", unknown_systems.join("、")));
            } else if !b.tools.is_empty() {
                // R3：系统机制 → agent + 工具
                b.executor = Executor::Agent;
                b.rules.push("R3".into());
            } else if info_work && (roles || !node.object_links.is_empty()) {
                // 信息类工作但没有链接系统：判为 agent，工具待专家指定
                b.executor = Executor::Agent;
                b.rules.push("R3".into());
                b.needs_expert.push("未链接系统，需指定工具".into());
            } else {
                // R8：不猜
                b.executor = Executor::Unbound;
                b.rules.push("R8".into());
                b.needs_expert.push(if roles {
                    "看不出是物理动作还是信息工作，需指定执行方式".into()
                } else {
                    "无执行角色、无对象链接，无法判断执行者".into()
                });
            }
            if b.executor == Executor::Agent && contains_any(label, WRITE_VERBS) {
                b.write_tools = b.tools.clone();
                b.gate = Some(Gate {
                    kind: GateKind::ActionApproval,
                    approvers: vec![],
                    deadline: None,
                    escalation_to: vec![],
                });
            }
        }
    }
    // R6：控制依据 → 技能
    for l in links(node, "control") {
        push_unique(&mut b.skills, &l.name);
    }
    if !b.skills.is_empty() {
        b.rules.push("R6".into());
    }
    // R7：角色 → 领域；没有角色时用工具的默认领域
    b.domain = role_domain(&node.actor_roles).or_else(|| {
        let ds: Vec<_> = b.tools.iter().filter_map(|t| tool_domain(t)).collect();
        (ds.len() == 1).then(|| ds[0].clone())
    });
    if b.domain.is_some() {
        b.rules.push("R7".into());
    } else if b.executor == Executor::Agent {
        b.needs_expert.push("未能确定领域（缺少执行角色）".into());
    }
    b
}

/// 节点的拓扑顺序（原始顺序作为平局规则）；万一有环，剩余节点按原始顺序接在后面
fn topo_order(wf: &ExpertWorkflow) -> Vec<String> {
    let ids: Vec<&str> = wf.nodes.iter().map(|n| n.node_id.as_str()).collect();
    let mut indeg: BTreeMap<&str, usize> = ids.iter().map(|i| (*i, 0)).collect();
    for e in &wf.edges {
        if let Some(d) = indeg.get_mut(e.to.as_str()) {
            *d += 1;
        }
    }
    let mut order = vec![];
    let mut done = BTreeSet::new();
    loop {
        let next = ids
            .iter()
            .find(|i| !done.contains(*i) && indeg[*i] == 0)
            .copied();
        let Some(n) = next else { break };
        done.insert(n);
        order.push(n.to_string());
        for e in wf.edges.iter().filter(|e| e.from == n) {
            if let Some(d) = indeg.get_mut(e.to.as_str()) {
                *d = d.saturating_sub(1);
            }
        }
    }
    for i in ids {
        if !done.contains(i) {
            order.push(i.to_string());
        }
    }
    order
}

/// 编译一条工作流。`bindings_confirmed` 表示专家已逐个确认绑定卡。
pub fn compile(wf: &ExpertWorkflow, bindings_confirmed: bool) -> WorkflowTemplate {
    let nodes: Vec<NodeBinding> = wf.nodes.iter().map(|n| bind_node(wf, n)).collect();
    let by_id: BTreeMap<&str, &NodeBinding> =
        nodes.iter().map(|b| (b.node_id.as_str(), b)).collect();
    let order = topo_order(wf);
    let pos: BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    // 合并：A→B 为普通/交接边，A 只有这一条出边、B 只有这一条入边，两者都是同领域 agent
    let outdeg = |id: &str| wf.edges.iter().filter(|e| e.from == id).count();
    let indeg = |id: &str| wf.edges.iter().filter(|e| e.to == id).count();
    let mut group: BTreeMap<String, String> = wf
        .nodes
        .iter()
        .map(|n| (n.node_id.clone(), n.node_id.clone()))
        .collect();
    fn root(g: &BTreeMap<String, String>, mut x: String) -> String {
        while g[&x] != x {
            x = g[&x].clone();
        }
        x
    }
    for e in &wf.edges {
        let (Some(a), Some(b)) = (by_id.get(e.from.as_str()), by_id.get(e.to.as_str())) else {
            continue;
        };
        let mergeable = matches!(e.edge_type.as_str(), "normal" | "handoff")
            && outdeg(&e.from) == 1
            && indeg(&e.to) == 1
            && a.executor == Executor::Agent
            && b.executor == Executor::Agent
            && a.domain.is_some()
            && a.domain == b.domain;
        if mergeable {
            let (ra, rb) = (root(&group, e.from.clone()), root(&group, e.to.clone()));
            if ra != rb {
                group.insert(rb, ra);
            }
        }
    }
    // 按拓扑顺序生成段
    let mut seg_index: BTreeMap<String, usize> = BTreeMap::new();
    let mut segments: Vec<Segment> = vec![];
    for id in &order {
        let r = root(&group, id.clone());
        let b = by_id[id.as_str()];
        let i = *seg_index.entry(r).or_insert_with(|| {
            segments.push(Segment {
                id: format!("S{}", segments.len() + 1),
                executor: b.executor,
                domain: b.domain.clone(),
                node_ids: vec![],
                steps: vec![],
                tools: vec![],
                write_tools: vec![],
                skills: vec![],
                gates: vec![],
            });
            segments.len() - 1
        });
        let s = &mut segments[i];
        s.node_ids.push(id.clone());
        s.steps.push(b.label.clone());
        for t in &b.tools {
            push_unique(&mut s.tools, t);
        }
        for t in &b.write_tools {
            push_unique(&mut s.write_tools, t);
        }
        for k in &b.skills {
            push_unique(&mut s.skills, k);
        }
        if let Some(g) = &b.gate {
            s.gates.push(g.clone());
        }
    }
    for s in &mut segments {
        s.tools.sort();
        s.write_tools.sort();
    }
    let seg_of = |id: &str| -> Option<String> {
        segments
            .iter()
            .find(|s| s.node_ids.iter().any(|n| n == id))
            .map(|s| s.id.clone())
    };
    let mut edges: Vec<SegmentEdge> = vec![];
    let mut sorted_edges: Vec<&ExpertEdge> = wf.edges.iter().collect();
    sorted_edges.sort_by_key(|e| {
        (
            pos.get(e.from.as_str()).copied(),
            pos.get(e.to.as_str()).copied(),
        )
    });
    for e in sorted_edges {
        let (Some(f), Some(t)) = (seg_of(&e.from), seg_of(&e.to)) else {
            continue;
        };
        if f == t {
            continue;
        }
        let se = SegmentEdge {
            from: f,
            to: t,
            edge_type: e.edge_type.clone(),
            condition: e.condition.clone(),
        };
        if !edges.contains(&se) {
            edges.push(se);
        }
    }
    let parallel_branches = wf
        .edges
        .iter()
        .filter(|e| e.edge_type == "parallel")
        .count();
    let mut t = WorkflowTemplate {
        id: format!("WF-{}", wf.id),
        name: wf.name.clone(),
        source_workflow: wf.id.clone(),
        nodes,
        segments,
        edges,
        parallel_branches,
        promotion: String::new(),
        promotion_reasons: vec![],
    };
    // 晋升判定：校验失败 → invalid；有未完成的绑定 → needs_completion；
    // 绑定完整但专家未确认 → pending_expert_confirmation；否则 promoted
    match validate_workflow_template(&t) {
        Err(problems) => {
            t.promotion = "invalid".into();
            t.promotion_reasons = problems;
        }
        Ok(()) => {
            let reasons: Vec<String> = t
                .nodes
                .iter()
                .flat_map(|b| {
                    b.needs_expert
                        .iter()
                        .map(move |r| format!("{}（{}）：{}", b.label, b.node_id, r))
                })
                .collect();
            if !reasons.is_empty() {
                t.promotion = "needs_completion".into();
                t.promotion_reasons = reasons;
            } else if !bindings_confirmed {
                t.promotion = "pending_expert_confirmation".into();
            } else {
                t.promotion = "promoted".into();
            }
        }
    }
    t
}

/// 校验工作流模板，返回全部问题
pub fn validate_workflow_template(t: &WorkflowTemplate) -> Result<(), Vec<String>> {
    let mut p = vec![];
    let seg_ids: BTreeSet<&str> = t.segments.iter().map(|s| s.id.as_str()).collect();
    if seg_ids.len() != t.segments.len() {
        p.push("段 id 重复".to_string());
    }
    // 1. 每个节点恰好属于一段，段里不能有不存在的节点
    for b in &t.nodes {
        let n = t
            .segments
            .iter()
            .filter(|s| s.node_ids.contains(&b.node_id))
            .count();
        if n != 1 {
            p.push(format!("节点 {} 属于 {n} 个段（应为 1）", b.node_id));
        }
    }
    for s in &t.segments {
        for id in &s.node_ids {
            if !t.nodes.iter().any(|b| &b.node_id == id) {
                p.push(format!("段 {} 含不存在的节点 {id}", s.id));
            }
        }
    }
    // 2. 段间边指向存在的段，且无环
    for e in &t.edges {
        if !seg_ids.contains(e.from.as_str()) || !seg_ids.contains(e.to.as_str()) {
            p.push(format!("段间边 {}→{} 指向不存在的段", e.from, e.to));
        }
    }
    let mut indeg: BTreeMap<&str, usize> = seg_ids.iter().map(|s| (*s, 0)).collect();
    for e in &t.edges {
        if let Some(d) = indeg.get_mut(e.to.as_str()) {
            *d += 1;
        }
    }
    let mut ready: Vec<&str> = indeg
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(s, _)| *s)
        .collect();
    let mut seen = 0;
    while let Some(s) = ready.pop() {
        seen += 1;
        for e in t.edges.iter().filter(|e| e.from == s) {
            if let Some(d) = indeg.get_mut(e.to.as_str()) {
                *d -= 1;
                if *d == 0 {
                    ready.push(e.to.as_str());
                }
            }
        }
    }
    if seen != seg_ids.len() {
        p.push("段间路由有环".to_string());
    }
    // 3. 工具都在清单内；4. 写工具必须带闸门
    let has_action_gate = |gates: &[&Gate]| {
        gates
            .iter()
            .any(|g| matches!(g.kind, GateKind::ActionApproval | GateKind::Approval))
    };
    for b in &t.nodes {
        for tool in b.tools.iter().chain(&b.write_tools) {
            if !is_known_tool(tool) {
                p.push(format!("节点 {} 的工具 {tool} 不在清单内", b.node_id));
            }
        }
        if b.write_tools.iter().any(|w| !b.tools.contains(w)) {
            p.push(format!("节点 {} 的写工具不在其工具列表里", b.node_id));
        }
        if !b.write_tools.is_empty() && !has_action_gate(&b.gate.iter().collect::<Vec<_>>()) {
            p.push(format!("节点 {} 有写操作但没有批准闸门", b.node_id));
        }
        // 5. 审批节点必须由人完成且带审批闸门
        if b.node_type == "approval"
            && (b.executor != Executor::Human
                || b.gate.as_ref().map(|g| g.kind) != Some(GateKind::Approval))
        {
            p.push(format!("审批节点 {} 必须由人执行并带审批闸门", b.node_id));
        }
    }
    for s in &t.segments {
        for tool in s.tools.iter().chain(&s.write_tools) {
            if !is_known_tool(tool) {
                p.push(format!("段 {} 的工具 {tool} 不在清单内", s.id));
            }
        }
        if !s.write_tools.is_empty() && !has_action_gate(&s.gates.iter().collect::<Vec<_>>()) {
            p.push(format!("段 {} 有写操作但没有批准闸门", s.id));
        }
        // 6. 多节点段只能由同领域 agent 组成
        let members: Vec<&NodeBinding> = t
            .nodes
            .iter()
            .filter(|b| s.node_ids.contains(&b.node_id))
            .collect();
        if s.node_ids.len() > 1
            && !members.iter().all(|b| {
                b.executor == Executor::Agent && b.domain.is_some() && b.domain == s.domain
            })
        {
            p.push(format!("段 {} 合并了不同执行者或不同领域的节点", s.id));
        }
        if members.iter().any(|b| b.executor != s.executor) {
            p.push(format!("段 {} 的执行者与成员节点不一致", s.id));
        }
    }
    if p.is_empty() {
        Ok(())
    } else {
        Err(p)
    }
}

fn executor_name(e: Executor) -> &'static str {
    match e {
        Executor::Agent => "agent",
        Executor::Human => "human",
        Executor::System => "system",
        Executor::AgentThenHuman => "agent_then_human",
        Executor::Structural => "structural",
        Executor::Unbound => "unbound",
    }
}

/// 晋升漏斗与绑定比例统计（未经专家确认，`promoted` 恒为 0）
pub fn promotion_report(workflows: &[ExpertWorkflow]) -> Value {
    let mut rows = vec![];
    let mut by_exec: BTreeMap<&str, u64> = BTreeMap::new();
    let mut rules: BTreeMap<String, u64> = BTreeMap::new();
    let mut tools: BTreeMap<String, u64> = BTreeMap::new();
    let (mut nodes, mut work, mut needs, mut segs, mut agent_segs, mut multi) = (0, 0, 0, 0, 0, 0);
    let (mut valid, mut bound, mut parallel, mut pending, mut promoted) = (0, 0, 0, 0, 0);
    for wf in workflows {
        let t = compile(wf, false);
        let mut ex: BTreeMap<&str, u64> = BTreeMap::new();
        for b in &t.nodes {
            *ex.entry(executor_name(b.executor)).or_default() += 1;
            *by_exec.entry(executor_name(b.executor)).or_default() += 1;
            for r in &b.rules {
                *rules.entry(r.clone()).or_default() += 1;
            }
            for tool in &b.tools {
                *tools.entry(tool.clone()).or_default() += 1;
            }
        }
        let w = t
            .nodes
            .iter()
            .filter(|b| b.executor != Executor::Structural)
            .count();
        let n_needs = t
            .nodes
            .iter()
            .filter(|b| !b.needs_expert.is_empty())
            .count();
        let a_segs = t
            .segments
            .iter()
            .filter(|s| s.executor == Executor::Agent)
            .count();
        let m_segs = t
            .segments
            .iter()
            .filter(|s| s.executor == Executor::Agent && s.node_ids.len() > 1)
            .count();
        nodes += t.nodes.len();
        work += w;
        needs += n_needs;
        segs += t.segments.len();
        agent_segs += a_segs;
        multi += m_segs;
        valid += usize::from(t.promotion != "invalid");
        bound += usize::from(matches!(
            t.promotion.as_str(),
            "pending_expert_confirmation" | "promoted"
        ));
        parallel += usize::from(t.parallel_branches > 0);
        pending += usize::from(t.promotion == "pending_expert_confirmation");
        promoted += usize::from(t.promotion == "promoted");
        rows.push(json!({
            "id": wf.id, "name": wf.name, "nodes": t.nodes.len(), "work_nodes": w,
            "by_executor": ex, "needs_expert": n_needs, "segments": t.segments.len(),
            "agent_segments": a_segs, "multi_node_agent_segments": m_segs,
            "parallel_branches": t.parallel_branches, "promotion": t.promotion,
            "promotion_reasons": t.promotion_reasons,
        }));
    }
    json!({
        "workflows": rows,
        "totals": {
            "nodes": nodes, "work_nodes": work, "by_executor": by_exec, "needs_expert": needs,
            "segments": segs, "agent_segments": agent_segs,
            "multi_node_agent_segments": multi, "rules": rules, "tools": tools,
        },
        "funnel": {
            "workflows": workflows.len(), "valid": valid, "fully_bound": bound,
            "parallel_eligible": parallel, "pending_expert_confirmation": pending,
            "promoted": promoted,
        },
    })
}
