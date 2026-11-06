//! M4.3 模板拆分与输入输出 JSON（设计见 `docs/DESIGN.md` 第 9.6 节）。
//!
//! 本模块只做确定性的校验，不调用模型，也不执行任务：
//! - [`validate_template`]：模板加载时检查（无未知字段、能力存在、分支数不超过槽位数……）；
//! - [`decide_split`]：Router 输出 `template_id` / `deliverable` / `slots_ok` 后决定拆分还是整单；
//! - [`validate_envelope`] / [`assemble_join`]：分支信封校验，缺一路记为 `missing`。
//!
//! 设计要点：
//! - 模板里没有任何"边"字段（`deny_unknown_fields`），分支之间不可能形成依赖环；
//!   分支内部的先后关系只以 `serial_steps` 文本形式交给该分支的 agent。
//! - 吃不准就不拆：任何不明确的 Router 输出都退回单动作。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use an_core::Role;
use serde::{Deserialize, Serialize};

/// 信封 `status` 允许的取值
pub const STATUS_OK: &str = "ok";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_FAILED: &str = "failed";

/// 整路未返回时补上的缺失原因
const MISSING_NOT_RETURNED: &str = "整路未返回";

/// 一个并行模板。字段与设计文档中的 JSON 一一对应，未知字段直接拒绝。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    pub deliverable: String,
    pub required_slots: Vec<String>,
    pub branches: Vec<BranchSpec>,
    pub join: JoinSpec,
}

/// 一路分支：由哪种能力执行、内部按什么顺序做、必须交回哪些字段
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchSpec {
    pub id: String,
    /// 与 Router 的执行角色同一套名字（general / coding / reviewer）；其他值解析时报错
    pub capability: Role,
    pub serial_steps: Vec<String>,
    pub output_required: Vec<String>,
}

/// 汇合任务
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinSpec {
    pub capability: Role,
    pub writes_file: bool,
}

/// 分支返回的信封
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchEnvelope {
    pub branch_id: String,
    pub status: String,
    pub missing: String,
    /// 只允许字符串值（设计示例如 `"hours": "6"`）。数字、布尔、数组、对象在解析时即被拒绝，
    /// 调用方应把解析失败记为该路 `missing`。
    pub payload: BTreeMap<String, String>,
}

/// Router 的三个扁平字段（设计 9.6 的表格）。字段都必填，不适用的填空字符串。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSlots {
    pub template_id: String,
    pub deliverable: String,
    pub slots_ok: String,
}

/// 拆分决定
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitDecision {
    /// 整单走现有的单动作路径；`reason` 写入日志，便于判断为什么没拆
    Single { reason: String },
    /// 按模板并行
    Split { template_id: String },
}

/// 已注册模板的集合。id 必须唯一。
#[derive(Debug, Clone, Default)]
pub struct TemplateRegistry {
    templates: Vec<Template>,
}

impl TemplateRegistry {
    /// 构造注册表：每个模板都必须通过 [`validate_template`]，id 不得重复。
    /// 任何一个模板有问题都整体失败，而不是跳过它。
    pub fn new(templates: Vec<Template>, max_branches: usize) -> Result<Self, Vec<String>> {
        let mut problems = Vec::new();
        let mut seen = HashSet::new();
        for t in &templates {
            if !seen.insert(t.id.as_str()) {
                problems.push(format!("模板 id 重复：{}", t.id));
            }
            if let Err(mut p) = validate_template(t, max_branches) {
                problems.append(&mut p);
            }
        }
        if problems.is_empty() {
            Ok(Self { templates })
        } else {
            Err(problems)
        }
    }

    pub fn get(&self, id: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.id == id)
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
}

/// 检查单个模板。返回全部问题（而不是遇到第一个就停），方便一次改完。
pub fn validate_template(t: &Template, max_branches: usize) -> Result<(), Vec<String>> {
    let mut p = Vec::new();
    let who = if t.id.is_empty() {
        "<无 id>"
    } else {
        t.id.as_str()
    };

    if t.id.trim().is_empty() {
        p.push("模板 id 不能为空".to_string());
    }
    if t.deliverable.trim().is_empty() {
        p.push(format!("[{who}] deliverable 不能为空"));
    }
    if t.branches.is_empty() {
        p.push(format!("[{who}] 至少需要一个分支"));
    }
    if t.branches.len() > max_branches {
        p.push(format!(
            "[{who}] 分支数 {} 超过后台槽位数 {max_branches}",
            t.branches.len()
        ));
    }

    let mut branch_ids = HashSet::new();
    for b in &t.branches {
        if b.id.trim().is_empty() {
            p.push(format!("[{who}] 分支 id 不能为空"));
        } else if !branch_ids.insert(b.id.as_str()) {
            p.push(format!("[{who}] 分支 id 重复：{}", b.id));
        }
        if b.serial_steps.is_empty() || b.serial_steps.iter().any(|s| s.trim().is_empty()) {
            p.push(format!(
                "[{who}] 分支 {} 的 serial_steps 为空或含空步骤",
                b.id
            ));
        }
        if b.output_required.is_empty() {
            p.push(format!("[{who}] 分支 {} 没有声明 output_required", b.id));
        }
        let unique: HashSet<&str> = b.output_required.iter().map(|s| s.as_str()).collect();
        if unique.len() != b.output_required.len() {
            p.push(format!(
                "[{who}] 分支 {} 的 output_required 有重复字段",
                b.id
            ));
        }
    }

    if p.is_empty() {
        Ok(())
    } else {
        Err(p)
    }
}

/// 根据 Router 的三个字段决定是否拆分。任何不明确的情况都退回单动作。
pub fn decide_split(slots: &RouteSlots, registry: &TemplateRegistry) -> SplitDecision {
    if slots.template_id.is_empty() {
        return SplitDecision::Single {
            reason: "template_id 为空，不拆分".into(),
        };
    }
    if slots.slots_ok != "yes" {
        // 包括 "no" 以及格式不对的值（如 "maybe"）：吃不准就不拆
        return SplitDecision::Single {
            reason: format!("slots_ok={} 不是 yes，不拆分", slots.slots_ok),
        };
    }
    let Some(t) = registry.get(&slots.template_id) else {
        return SplitDecision::Single {
            reason: format!("模板 {} 未注册，不拆分", slots.template_id),
        };
    };
    if slots.deliverable != t.deliverable {
        return SplitDecision::Single {
            reason: format!(
                "交付物 {} 与模板 {} 的 {} 不一致，不拆分",
                slots.deliverable, t.id, t.deliverable
            ),
        };
    }
    SplitDecision::Split {
        template_id: t.id.clone(),
    }
}

/// 校验单路信封是否符合该分支的规则。通过返回 `Ok(())`，否则返回原因。
pub fn validate_envelope(spec: &BranchSpec, env: &BranchEnvelope) -> Result<(), String> {
    if env.branch_id != spec.id {
        return Err(format!(
            "branch_id {} 与分支 {} 不一致",
            env.branch_id, spec.id
        ));
    }
    match env.status.as_str() {
        STATUS_OK => {
            if !env.missing.is_empty() {
                return Err("status 为 ok 时 missing 必须为空".into());
            }
            let got: BTreeSet<&str> = env.payload.keys().map(|k| k.as_str()).collect();
            let want: BTreeSet<&str> = spec.output_required.iter().map(|k| k.as_str()).collect();
            if let Some(extra) = got.difference(&want).next() {
                return Err(format!("payload 含未声明的字段 {extra}"));
            }
            if let Some(lack) = want.difference(&got).next() {
                return Err(format!("payload 缺少字段 {lack}"));
            }
            for (k, v) in &env.payload {
                if v.trim().is_empty() {
                    return Err(format!("payload 字段 {k} 为空"));
                }
            }
            Ok(())
        }
        STATUS_MISSING => {
            if env.missing.trim().is_empty() {
                return Err("status 为 missing 时必须说明缺的是什么".into());
            }
            if !env.payload.is_empty() {
                return Err("status 不是 ok 时 payload 必须为空对象".into());
            }
            Ok(())
        }
        STATUS_FAILED => {
            if !env.payload.is_empty() {
                return Err("status 不是 ok 时 payload 必须为空对象".into());
            }
            Ok(())
        }
        other => Err(format!("未知的 status：{other}")),
    }
}

/// 汇合的结果
#[derive(Debug, Clone, PartialEq)]
pub struct JoinInput {
    /// 按模板分支顺序排列，每个模板分支恰好一条
    pub branches: Vec<BranchEnvelope>,
    /// 全部 ok 时为 true
    pub complete: bool,
    /// 被记为 missing 的分支 id（包括整路未返回和信封校验失败）
    pub missing_branches: Vec<String>,
}

/// 把各路信封组装成汇合输入。
/// - 同一个 branch_id 出现两次、或出现模板里没有的 branch_id：整体报错（这是调度层的 bug）。
/// - 某路整体缺席：记为 `missing`。
/// - 某路信封校验失败：也记为 `missing`，原因写入 `missing` 字段，payload 清空。
pub fn assemble_join(
    template: &Template,
    envelopes: &[BranchEnvelope],
) -> Result<JoinInput, String> {
    let known: HashSet<&str> = template.branches.iter().map(|b| b.id.as_str()).collect();
    let mut by_id = std::collections::HashMap::new();
    for env in envelopes {
        if !known.contains(env.branch_id.as_str()) {
            return Err(format!(
                "信封的 branch_id {} 不在模板 {} 中",
                env.branch_id, template.id
            ));
        }
        if by_id.insert(env.branch_id.as_str(), env).is_some() {
            return Err(format!("branch_id {} 重复", env.branch_id));
        }
    }

    let mut branches = Vec::with_capacity(template.branches.len());
    let mut missing_branches = Vec::new();
    for spec in &template.branches {
        let env = match by_id.get(spec.id.as_str()) {
            None => missing_envelope(&spec.id, MISSING_NOT_RETURNED),
            Some(env) => match validate_envelope(spec, env) {
                Ok(()) => (*env).clone(),
                Err(reason) => missing_envelope(&spec.id, &format!("信封校验失败：{reason}")),
            },
        };
        if env.status != STATUS_OK {
            missing_branches.push(spec.id.clone());
        }
        branches.push(env);
    }
    Ok(JoinInput {
        complete: missing_branches.is_empty(),
        branches,
        missing_branches,
    })
}

fn missing_envelope(branch_id: &str, reason: &str) -> BranchEnvelope {
    BranchEnvelope {
        branch_id: branch_id.to_string(),
        status: STATUS_MISSING.into(),
        missing: reason.into(),
        payload: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// 设计文档 9.6 中的 quote 模板原文（JSON）
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

    fn registry() -> TemplateRegistry {
        TemplateRegistry::new(vec![quote()], 4).expect("quote 应通过校验")
    }

    fn slots(template_id: &str, deliverable: &str, slots_ok: &str) -> RouteSlots {
        RouteSlots {
            template_id: template_id.into(),
            deliverable: deliverable.into(),
            slots_ok: slots_ok.into(),
        }
    }

    fn ok_env(branch: &str, payload: Value) -> BranchEnvelope {
        BranchEnvelope {
            branch_id: branch.into(),
            status: "ok".into(),
            missing: String::new(),
            payload: serde_json::from_value(payload).expect("测试数据应为字符串映射"),
        }
    }

    // ── 模板加载 ──

    #[test]
    fn quote_template_from_design_is_valid() {
        assert!(validate_template(&quote(), 4).is_ok());
    }

    #[test]
    fn unknown_field_is_rejected_so_no_hidden_edges() {
        // 任何"边"字段都不能悄悄混进模板
        let bad = QUOTE.replacen(
            r#""id": "parts","#,
            r#""id": "parts", "depends_on": ["damage"],"#,
            1,
        );
        assert!(serde_json::from_str::<Template>(&bad).is_err());
    }

    #[test]
    fn too_many_branches_for_slots_is_rejected() {
        let problems = validate_template(&quote(), 2).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("超过后台槽位数")),
            "{problems:?}"
        );
    }

    #[test]
    fn capability_must_be_a_router_role_name() {
        // capability 与 Router 的执行角色同名：不认识的名字在解析时直接报错
        let bad = QUOTE.replacen(
            r#""capability": "general","#,
            r#""capability": "writer","#,
            1,
        );
        let err = serde_json::from_str::<Template>(&bad)
            .unwrap_err()
            .to_string();
        assert!(err.contains("writer"), "{err}");
        // 设计示例中的 general / 其他两个角色名都应能解析
        for name in ["general", "coding", "reviewer"] {
            let ok = QUOTE.replacen(
                r#""capability": "general","#,
                &format!(r#""capability": "{name}","#),
                1,
            );
            assert!(
                serde_json::from_str::<Template>(&ok).is_ok(),
                "{name} 应可解析"
            );
        }
    }

    #[test]
    fn duplicate_branch_id_and_empty_steps_are_reported() {
        let mut t = quote();
        t.branches[1].id = "damage".into();
        t.branches[2].serial_steps = vec![];
        let problems = validate_template(&t, 4).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("分支 id 重复")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("serial_steps")),
            "{problems:?}"
        );
    }

    #[test]
    fn duplicate_template_id_is_rejected() {
        let err = TemplateRegistry::new(vec![quote(), quote()], 4).unwrap_err();
        assert!(err.iter().any(|p| p.contains("模板 id 重复")));
    }

    // ── Router 拆分决定 ──

    #[test]
    fn matching_slots_split() {
        assert_eq!(
            decide_split(&slots("quote", "报价单", "yes"), &registry()),
            SplitDecision::Split {
                template_id: "quote".into()
            }
        );
    }

    #[test]
    fn slots_ok_no_or_malformed_stays_single() {
        for v in ["no", "maybe", "", "YES"] {
            assert!(
                matches!(
                    decide_split(&slots("quote", "报价单", v), &registry()),
                    SplitDecision::Single { .. }
                ),
                "slots_ok={v:?} 应退回单动作"
            );
        }
    }

    #[test]
    fn empty_template_id_stays_single() {
        assert!(matches!(
            decide_split(&slots("", "", "yes"), &registry()),
            SplitDecision::Single { .. }
        ));
    }

    #[test]
    fn unregistered_template_stays_single() {
        let d = decide_split(&slots("stop_line", "停线清单", "yes"), &registry());
        assert!(
            matches!(d, SplitDecision::Single { ref reason } if reason.contains("未注册")),
            "{d:?}"
        );
    }

    #[test]
    fn deliverable_mismatch_stays_single() {
        // 设计：交付物必须与模板一致，否则拒绝拆分
        let d = decide_split(&slots("quote", "放行单", "yes"), &registry());
        assert!(
            matches!(d, SplitDecision::Single { ref reason } if reason.contains("不一致")),
            "{d:?}"
        );
    }

    // ── 分支信封 ──

    #[test]
    fn non_string_payload_value_is_rejected_at_parse() {
        // 数字、布尔、数组、对象都不允许：调用方解析失败时记为该路 missing
        for bad in [
            json!({"hours": 6}),
            json!({"ok": true}),
            json!({"x": ["a"]}),
            json!({"x": null}),
        ] {
            let raw = json!({"branch_id": "damage", "status": "ok", "missing": "", "payload": bad});
            assert!(
                serde_json::from_value::<BranchEnvelope>(raw).is_err(),
                "{bad} 应被拒绝"
            );
        }
        let good = json!({"branch_id": "damage", "status": "ok", "missing": "", "payload": {"hours": "6"}});
        assert!(serde_json::from_value::<BranchEnvelope>(good).is_ok());
    }

    #[test]
    fn ok_envelope_with_exact_fields_passes() {
        let spec = &quote().branches[0];
        let env = ok_env("damage", json!({"verdict": "可修", "hours": "6"}));
        assert!(validate_envelope(spec, &env).is_ok());
    }

    #[test]
    fn ok_envelope_missing_or_extra_field_fails() {
        let spec = &quote().branches[0];
        let lack = ok_env("damage", json!({"verdict": "可修"}));
        assert!(validate_envelope(spec, &lack)
            .unwrap_err()
            .contains("缺少字段 hours"));
        let extra = ok_env(
            "damage",
            json!({"verdict": "可修", "hours": "6", "note": "x"}),
        );
        assert!(validate_envelope(spec, &extra)
            .unwrap_err()
            .contains("未声明的字段 note"));
    }

    #[test]
    fn ok_envelope_blank_value_fails() {
        let spec = &quote().branches[0];
        let env = ok_env("damage", json!({"verdict": "  ", "hours": "6"}));
        assert!(validate_envelope(spec, &env)
            .unwrap_err()
            .contains("verdict"));
    }

    #[test]
    fn non_ok_envelope_must_have_empty_payload_and_reason() {
        let spec = &quote().branches[0];
        let mut miss = missing_envelope("damage", "缺测量值");
        miss.payload.insert("verdict".into(), "可修".into());
        assert!(validate_envelope(spec, &miss)
            .unwrap_err()
            .contains("payload 必须为空"));
        let no_reason = missing_envelope("damage", " ");
        assert!(validate_envelope(spec, &no_reason)
            .unwrap_err()
            .contains("说明缺的是什么"));
        let failed = BranchEnvelope {
            branch_id: "damage".into(),
            status: "failed".into(),
            missing: String::new(),
            payload: BTreeMap::new(),
        };
        assert!(validate_envelope(spec, &failed).is_ok());
    }

    #[test]
    fn envelope_branch_id_and_status_are_checked() {
        let spec = &quote().branches[0];
        let wrong_id = ok_env("parts", json!({"verdict": "可修", "hours": "6"}));
        assert!(validate_envelope(spec, &wrong_id)
            .unwrap_err()
            .contains("不一致"));
        let mut bad_status = ok_env("damage", json!({"verdict": "可修", "hours": "6"}));
        bad_status.status = "done".into();
        assert!(validate_envelope(spec, &bad_status)
            .unwrap_err()
            .contains("未知的 status"));
    }

    // ── 汇合 ──

    #[test]
    fn join_complete_when_all_branches_ok() {
        let t = quote();
        let envs = vec![
            ok_env("damage", json!({"verdict": "可修", "hours": "6"})),
            ok_env("parts", json!({"shortage_amount": "1200"})),
            ok_env("history", json!({"warranty_risk": "低"})),
        ];
        let j = assemble_join(&t, &envs).unwrap();
        assert!(j.complete);
        assert!(j.missing_branches.is_empty());
        // 输出顺序与模板一致，而不是输入顺序
        let order: Vec<&str> = j.branches.iter().map(|b| b.branch_id.as_str()).collect();
        assert_eq!(order, ["damage", "parts", "history"]);
    }

    #[test]
    fn absent_branch_is_recorded_as_missing_not_filled() {
        let t = quote();
        let envs = vec![
            ok_env("damage", json!({"verdict": "可修", "hours": "6"})),
            ok_env("history", json!({"warranty_risk": "低"})),
        ];
        let j = assemble_join(&t, &envs).unwrap();
        assert!(!j.complete);
        assert_eq!(j.missing_branches, vec!["parts".to_string()]);
        let parts = j.branches.iter().find(|b| b.branch_id == "parts").unwrap();
        assert_eq!(parts.status, "missing");
        assert_eq!(parts.missing, MISSING_NOT_RETURNED);
        assert!(parts.payload.is_empty(), "缺失的分支不得用别的数据填补");
    }

    #[test]
    fn invalid_envelope_is_downgraded_to_missing_with_reason() {
        let t = quote();
        let envs = vec![
            ok_env("damage", json!({"verdict": "可修"})), // 缺 hours
            ok_env("parts", json!({"shortage_amount": "1200"})),
            ok_env("history", json!({"warranty_risk": "低"})),
        ];
        let j = assemble_join(&t, &envs).unwrap();
        assert!(!j.complete);
        assert_eq!(j.missing_branches, vec!["damage".to_string()]);
        assert!(j.branches[0].missing.contains("缺少字段 hours"));
    }

    #[test]
    fn join_rejects_unknown_or_duplicate_branch_ids() {
        let t = quote();
        let unknown = vec![ok_env("ghost", json!({"x": "1"}))];
        assert!(assemble_join(&t, &unknown)
            .unwrap_err()
            .contains("不在模板"));
        let dup = vec![
            ok_env("parts", json!({"shortage_amount": "1"})),
            ok_env("parts", json!({"shortage_amount": "2"})),
        ];
        assert!(assemble_join(&t, &dup).unwrap_err().contains("重复"));
    }
}
