//! 阶段③b：DES 案例的模板选择（单设备层）与产线协同事项的模板（协调层），设计文档 §6.8。
//!
//! 这里只做"根据案例当前观测和成员关系，应该用哪个模板、哪些节点"的纯计算；
//! 事项的建立、成员加入与执行回写在 `engine_link.rs`。只对 DES 案例生效（观测载荷带 `simulation` 字段），
//! 脚本案例返回 None，走原有逻辑。

use crate::mom::{Case, Node};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const TPL_EQUIPMENT: &str = "DAG-TPL-DES-EQUIPMENT@1";
pub(crate) const TPL_QUALITY: &str = "DAG-TPL-DES-QUALITY@1";
pub(crate) const TPL_COMBINED: &str = "DAG-TPL-DES-COMBINED@1";
pub(crate) const TPL_DIAGNOSIS: &str = "DAG-TPL-DES-DIAGNOSIS@1";
pub(crate) const TPL_LINE_INCIDENT: &str = "DAG-TPL-DES-LINE-INCIDENT@1";

/// 案例是否来自 DES 仿真（任一观测载荷带 `simulation` 字段）。
pub(crate) fn is_des_case(c: &Case) -> bool {
    c.observations
        .iter()
        .any(|o| o.event.payload.get("simulation").is_some())
}

/// 案例是否是产线协同事项。
pub(crate) fn is_incident(c: &Case) -> bool {
    c.world.get("des_incident").is_some_and(|v| v.is_object())
}

/// 案例是否已加入某个协同事项。
pub(crate) fn is_member(c: &Case) -> bool {
    c.world.get("des_member_of").is_some_and(|v| v.is_string())
}

fn kinds(c: &Case) -> BTreeSet<&str> {
    c.observations
        .iter()
        .map(|o| o.event.kind.as_str())
        .collect()
}

/// 案例是否有设备或质量报警。
pub(crate) fn has_alarm(c: &Case) -> bool {
    let k = kinds(c);
    k.contains("vibration") || k.contains("quality")
}

/// 节点规格：（节点编号，工具，依赖）。
type NodeSpec<'a> = (&'a str, &'a str, &'a [&'a str]);

fn nodes(list: &[NodeSpec]) -> BTreeMap<String, Node> {
    list.iter()
        .map(|(id, tool, deps)| (id.to_string(), Node::new(tool, deps)))
        .collect()
}

/// DES 报警案例应当使用的模板与节点；不适用时返回 None（脚本案例、只有插单、或三类事件齐全时沿用 S1）。
pub(crate) fn des_template(c: &Case) -> Option<(String, BTreeMap<String, Node>)> {
    if !is_des_case(c) || is_incident(c) {
        return None;
    }
    let k = kinds(c);
    let (v, q, o) = (
        k.contains("vibration"),
        k.contains("quality"),
        k.contains("order"),
    );
    if !v && !q {
        return None;
    }
    if is_member(c) {
        // 成员只做诊断，决策在协同事项上。
        let mut list: Vec<NodeSpec> = vec![];
        if v {
            list.push(("maintenance", "maintenance.assess", &[]));
        }
        if q {
            list.push(("quality", "quality.assess", &[]));
        }
        return Some((TPL_DIAGNOSIS.into(), nodes(&list)));
    }
    if v && q && o {
        return None; // 三类事件齐全：沿用原 S1 模板（P0 行为不变）。
    }
    let (template, list): (&str, Vec<NodeSpec>) = match (v, q) {
        (true, true) => (
            TPL_COMBINED,
            vec![
                ("maintenance", "maintenance.assess", &[]),
                ("quality", "quality.assess", &[]),
                ("resources", "resources.check", &[]),
                (
                    "schedule",
                    "schedule.simulate",
                    &["maintenance", "quality", "resources"],
                ),
            ],
        ),
        (true, false) => (
            TPL_EQUIPMENT,
            vec![
                ("maintenance", "maintenance.assess", &[]),
                ("resources", "resources.check", &[]),
                (
                    "schedule",
                    "schedule.simulate",
                    &["maintenance", "resources"],
                ),
            ],
        ),
        _ => (
            TPL_QUALITY,
            vec![
                ("quality", "quality.assess", &[]),
                ("resources", "resources.check", &[]),
                ("schedule", "schedule.simulate", &["quality", "resources"]),
            ],
        ),
    };
    Some((template.into(), nodes(&list)))
}

/// 协同事项的节点：有报警设备时排产，否则只核查资源（等报警成员加入后再排产）。
pub(crate) fn incident_nodes(has_alarmed_assets: bool) -> BTreeMap<String, Node> {
    if has_alarmed_assets {
        nodes(&[
            ("resources", "resources.check", &[]),
            ("schedule", "schedule.simulate", &["resources"]),
        ])
    } else {
        nodes(&[("resources", "resources.check", &[])])
    }
}
