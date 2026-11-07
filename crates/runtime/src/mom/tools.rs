//! Stateful, deterministic domain simulators. No network, shell or physical actions.
use super::model::Case;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResponse {
    pub status: String,
    pub data: Value,
    pub evidence_refs: Vec<String>,
    pub source: String,
    pub observed_at: u64,
    pub valid_until: u64,
    pub provenance: String,
    pub mock_ids: Vec<String>,
    pub limitations: Vec<String>,
}
pub fn known(tool: &str) -> bool {
    matches!(
        tool,
        "topology.resolve"
            | "maintenance.assess"
            | "quality.assess"
            | "resources.check"
            | "schedule.simulate"
            | "genealogy.trace"
            | "production.reconstruct"
            | "equipment.history"
            | "inventory.hold"
            | "resources.reserve"
            | "execution.dispatch"
            | "action.status"
            | "customer.send"
            | "verification.collect"
    )
}
/// E1-C：基于服务层记录的 RelOS 上下文（world.relos）给出诊断输出。
/// RelOS 只提供候选根因与证据关系，根因仍是假设；RelOS 不可用时返回 unknown，交由人工决策。
fn relos_diagnosis(relos: &Value, candidates_key: &str, base: Value) -> (Value, &'static str) {
    let status = relos["status"].as_str().unwrap_or("unavailable");
    if status != "ok" {
        return (
            json!({"relos_status":status,"relos_reason":relos.get("reason").cloned().unwrap_or(Value::Null),"cause_status":"UNKNOWN","continue_using":false,"investigation_needed":true}),
            "unknown",
        );
    }
    let mut data = base;
    data["candidate_causes"] = relos[candidates_key].clone();
    data["context_relation_ids"] = relos["selected_ids"].clone();
    data["relos_status"] = json!("ok");
    (data, "ok")
}
pub fn read(c: &Case, tool: &str, now: u64) -> Result<ToolResponse> {
    let w = &c.world;
    let relos = w.get("relos").filter(|v| v.is_object());
    let (source, ids, data, status) = match tool {
        "maintenance.assess" if relos.is_some() => {
            let (data, status) = relos_diagnosis(
                relos.unwrap(),
                "maintenance_candidates",
                json!({"cause_status":"HYPOTHESIS","continue_using":false,"investigation_needed":true}),
            );
            ("RelOS", vec!["M24"], data, status)
        }
        "quality.assess" if relos.is_some() => {
            // 测量值仍来自 QMS 替代件（M04），候选根因来自 RelOS（M24）
            let (data, status) = relos_diagnosis(
                relos.unwrap(),
                "quality_candidates",
                json!({"risk":"HOLD_PENDING_VALIDATION","root_cause_status":"UNKNOWN","cpk":w.get("cpk").cloned().unwrap_or(json!(1.1)),"threshold":1.33,"measurement_status":"fixture_valid"}),
            );
            ("QMS+RelOS", vec!["M04", "M24"], data, status)
        }
        "topology.resolve" => (
            "MES",
            vec!["M02"],
            json!({"asset":c.asset,"batch":c.batch,"line":"Line-B"}),
            "ok",
        ),
        "maintenance.assess" => {
            let timeout = w["maintenance_timeout"] == true;
            (
                "EAM",
                vec!["M03"],
                json!({"condition":if timeout {"UNKNOWN"}else{"WARNING"},"cause_status":"HYPOTHESIS","continue_using":false,"investigation_needed":true}),
                if timeout { "unknown" } else { "ok" },
            )
        }
        "quality.assess" => {
            let timeout = w["quality_timeout"] == true;
            (
                "QMS",
                vec!["M04"],
                json!({"risk":"HOLD_PENDING_VALIDATION","root_cause_status":"UNKNOWN","cpk":w.get("cpk").cloned().unwrap_or(json!(1.1)),"threshold":1.33,"measurement_status":if timeout {"UNKNOWN"}else{"fixture_valid"}}),
                if timeout { "unknown" } else { "ok" },
            )
        }
        "resources.check" => (
            "MES/WMS",
            vec!["M05"],
            json!({"available":w["resources_available"],"certified":w["certified"],"tool_available":w["tool_available"],"tool_transfer_minutes":w["tool_transfer_minutes"],"resource_version":c.resource_version}),
            "ok",
        ),
        "schedule.simulate" => {
            let qty = w["quantity"].as_u64().unwrap_or(500);
            let cap =
                w["capacity_xy"].as_u64().unwrap_or(0) + w["capacity_xz"].as_u64().unwrap_or(0);
            let feasible = w["resources_available"] == true
                && w["certified"] == true
                && w["tool_available"] == true
                && w["resource_conflict"] != true
                && cap >= qty;
            (
                "simulation",
                vec!["M06"],
                json!({"feasible":feasible,"quantity":qty,"capacity":cap,"exclude_assets":["CNC-XX"],"inspection_finish":w["inspection_finish"],"deadline":w["deadline"],"other_order_impact":w["other_order_impact"],"tool_transfer_minutes":w["tool_transfer_minutes"],"rejected_alternatives":[{"name":"continue XX","reason":"quality and equipment constraints"}],"limitations":["Fixture capacities, not a real solver"]}),
                "ok",
            )
        }
        "genealogy.trace" => {
            let missing = w["missing_genealogy"] == true;
            let conflict = w["conflicting_records"] == true;
            (
                "MES/WMS",
                vec!["M02"],
                json!({"batch":c.batch,"in_stock":if missing {Value::Null}else{w["inventory"].clone()},"delivered":w["delivered"],"missing":missing,"conflict":conflict}),
                if missing || conflict { "partial" } else { "ok" },
            )
        }
        "production.reconstruct" => (
            "MES",
            vec!["M13"],
            json!({"batch":c.batch,"shifts":["day","night"],"asset":c.asset,"record_gaps":[]}),
            "ok",
        ),
        "equipment.history" => (
            "EAM",
            vec!["M14"],
            json!({"asset":c.asset,"history":w["history"],"root_cause_status":"UNKNOWN"}),
            "ok",
        ),
        "verification.collect" => (
            "QMS/EAM",
            vec!["M12"],
            json!({"verified":c.actions.values().all(|a|a.state=="VERIFIED"),"scope":"mock operation receipts"}),
            "ok",
        ),
        "action.status" => (
            "MockWorld",
            vec!["M12"],
            serde_json::to_value(&c.actions)?,
            "ok",
        ),
        _ => bail!("tool not permitted in read context: {tool}"),
    };
    Ok(ToolResponse {
        status: status.into(),
        data,
        evidence_refs: vec![],
        source: source.into(),
        observed_at: now,
        valid_until: now + super::fixture_policy(&c.scenario).snapshot_ttl_minutes,
        provenance: if source.contains("RelOS") {
            "relos-context".into()
        } else {
            "mock".into()
        },
        mock_ids: ids.into_iter().map(String::from).collect(),
        limitations: if source.contains("RelOS") {
            vec![
                "RelOS relation knowledge is simulated; candidates are hypotheses, not verified root causes".into(),
            ]
        } else {
            vec!["Simulated source; not industrial validation".into()]
        },
    })
}
