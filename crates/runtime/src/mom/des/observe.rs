//! what-if 计划模型与 DES 案例的工具读取（MOM 业务逻辑）。
//!
//! 原则（设计文档 §6.4 阶段②修订说明）：这里只使用"现场看得见"的量——设备运行状态、振动、Cpk、
//! 刀具计数和资源配置——不读取主轴磨损等隐状态，也不复跑带随机数的仿真。
//! 因此 what-if 是计划工具，结果可能与之后仿真世界的真实演化不同，这正是被测系统需要面对的不确定性。
//! 可观测状态本身由模拟工厂 `an_des::observe` 给出（阶段⑤a-1）。

pub(crate) use super::kernel_observe::{
    factory_view, forecast, is_alarmed, observe, round, MachineObservation, Observation,
};
use super::{WhatIfRequest, CPK_THRESHOLD};
use crate::mom::tools::ToolResponse;
use crate::mom::Case;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

/// what-if 最长推演时间（分钟）与请求上限。
const WHATIF_HORIZON_MINUTES: u64 = 2880;
const MAX_WHATIF_QUANTITY: u64 = 100_000;
/// 工具结果的有效期（分钟），与 S1 夹具的快照有效期一致。
const TOOL_VALID_MINUTES: u64 = 5;

// ───────────────────────── what-if 计划模型 ─────────────────────────

/// 方案中参与生产的一台设备：从 `from` 分钟起、到 `until` 分钟前（相对当前时刻）按 `rate` 件/分钟生产。
struct Unit {
    id: String,
    rate: f64,
    from: u64,
    until: Option<u64>,
    /// 在报警设备上加工的件计为风险件。
    risky: bool,
}

struct PlanSpec {
    id: &'static str,
    label: &'static str,
    /// 报警设备继续生产的时长：None 一直生产，Some(n) 生产 n 分钟后停机维修。
    asset_runs_for: Option<u64>,
    needs_maintenance: bool,
}

/// 比较 A/B/C 三个方案。只读，不使用随机数，相同输入结果相同。
pub(crate) fn whatif(o: &Observation, request: &WhatIfRequest) -> Result<Value> {
    whatif_assets(
        o,
        std::slice::from_ref(&request.asset),
        request.quantity,
        request.due_minute,
        false,
    )
}

/// 对一组报警设备比较 A/B/C 三个方案（阶段③b 的产线协同事项）。方案作用于集合中所有设备，
/// 同产线其余设备分担生产；单台设备时与 `whatif` 相同。
pub(crate) fn whatif_assets(
    o: &Observation,
    assets: &[String],
    quantity: u64,
    due_minute: u64,
    reported_alarms: bool,
) -> Result<Value> {
    let request = WhatIfRequest {
        asset: assets.first().cloned().unwrap_or_default(),
        quantity,
        due_minute,
    };
    let request = &request;
    let now = o.observed_at;
    ensure!(!assets.is_empty(), "at least one machine is required");
    ensure!(
        (1..=MAX_WHATIF_QUANTITY).contains(&request.quantity),
        "quantity must be 1..={MAX_WHATIF_QUANTITY}"
    );
    ensure!(
        request.due_minute <= now + WHATIF_HORIZON_MINUTES,
        "due_minute must be within {WHATIF_HORIZON_MINUTES} minutes of the current virtual time"
    );
    let targets: Vec<&MachineObservation> = assets
        .iter()
        .map(|a| o.machine(a).with_context(|| format!("unknown machine {a}")))
        .collect::<Result<_>>()?;
    let line = targets[0].line.clone();
    let r = &o.resources;
    let share = r.urgent_capacity_share.clamp(0.0, 1.0);
    let rate = |m: &MachineObservation| 60.0 / f64::from(m.cycle_seconds) * share;

    let specs = [
        PlanSpec {
            id: "A",
            label: "continue on the alarmed machine",
            asset_runs_for: None,
            needs_maintenance: false,
        },
        PlanSpec {
            id: "B",
            label: "finish the current batch, then stop for maintenance",
            asset_runs_for: Some(r.finish_batch_minutes),
            needs_maintenance: true,
        },
        PlanSpec {
            id: "C",
            label: "stop now for maintenance",
            asset_runs_for: Some(0),
            needs_maintenance: true,
        },
    ];

    let mut plans = vec![];
    for spec in &specs {
        let mut violations: Vec<String> = vec![];
        let mut prerequisites: Vec<String> = vec![];
        let mut actions: Vec<String> = vec![];
        let mut units: Vec<Unit> = vec![];
        let mut spares = r.tool_spares;
        // 维修班组空闲时刻（相对现在）：与执行时 `des_after_execute` 的排班一致，
        // 每台设备依次分给最早空闲的班组；修好后按正常节拍恢复生产（阶段③b 修订）。
        let maintenance_from = spec.asset_runs_for.unwrap_or(0);
        let mut crews = vec![maintenance_from; r.maintenance_crews as usize];

        // 报警设备集合。
        for asset in &targets {
            // 协同事项的设备集合来自已报告的报警：报警在维修或检验处置前保持有效，
            // 即使观测值暂时回落也按报警设备处理（`reported_alarms`）。
            let alarmed = reported_alarms || is_alarmed(asset);
            if spec.asset_runs_for != Some(0) {
                if asset.status != "running" {
                    violations.push(format!("machine_down:{}", asset.id));
                } else {
                    if alarmed && spec.asset_runs_for.is_none() {
                        violations.push(format!("quality_risk:{}", asset.id));
                    }
                    units.push(Unit {
                        id: asset.id.clone(),
                        rate: rate(asset),
                        from: 0,
                        until: spec.asset_runs_for,
                        risky: alarmed,
                    });
                }
            }
            if spec.needs_maintenance && r.maintenance_crews > 0 {
                actions.push(format!("eam.maintenance:{}", asset.id));
                let crew = (0..crews.len()).min_by_key(|&i| crews[i]).unwrap_or(0);
                crews[crew] += r.maintenance_minutes;
                units.push(Unit {
                    id: asset.id.clone(),
                    rate: rate(asset),
                    from: crews[crew],
                    until: None,
                    risky: false,
                });
            }
        }
        if spec.needs_maintenance && r.maintenance_crews == 0 {
            violations.push("maintenance_crew_unavailable".into());
        }
        // 同线其他设备分担。
        for m in o
            .machines
            .iter()
            .filter(|m| m.line == line && !assets.contains(&m.id))
        {
            if m.status != "running" {
                violations.push(format!("machine_down:{}", m.id));
                continue;
            }
            if !m.certified {
                violations.push(format!("uncertified:{}", m.id));
                continue;
            }
            let mut from = 0;
            if !m.tool_present {
                if spares == 0 {
                    violations.push(format!("tool_unavailable:{}", m.id));
                    continue;
                }
                spares -= 1;
                prerequisites.push(format!("tool_transfer:{}", m.id));
                actions.push(format!("mro.transfer:{}", m.id));
                from = r.tool_transfer_minutes;
            }
            units.push(Unit {
                id: m.id.clone(),
                rate: rate(m),
                from,
                until: None,
                risky: false,
            });
        }

        // 逐分钟累计产量，直到完成或超出推演范围。
        let (mut produced, mut risk) = (0.0, 0.0);
        let mut done_at = None;
        for t in 0..WHATIF_HORIZON_MINUTES {
            for u in &units {
                if t >= u.from && u.until.is_none_or(|end| t < end) {
                    produced += u.rate;
                    if u.risky {
                        risk += u.rate;
                    }
                }
            }
            if produced >= request.quantity as f64 {
                done_at = Some(t + 1);
                break;
            }
        }
        let completion = done_at.map(|t| now + t + r.inspection_minutes);
        let on_time = completion.is_some_and(|c| c <= request.due_minute);
        if !on_time {
            violations.push("deadline_miss".into());
        }
        if !actions.iter().any(|a| a.starts_with("mes.reschedule")) {
            actions.push("mes.reschedule".into());
        }
        plans.push(json!({
            "id": spec.id,
            "label": spec.label,
            "feasible": violations.is_empty(),
            "completion_minute": completion,
            "on_time": on_time,
            "risk_parts": round(risk, 1),
            "violations": violations,
            // 同一设备修前、修后各一个产能单元，这里只列一次。
            "uses": units.iter().fold(Vec::<String>::new(), |mut v, u| {
                if !v.contains(&u.id) {
                    v.push(u.id.clone());
                }
                v
            }),
            "prerequisites": prerequisites,
            "actions": actions,
        }));
    }

    // 推荐：可行方案中风险件最少，其次完成最早。
    let recommended = plans
        .iter()
        .filter(|p| p["feasible"] == true)
        .min_by(|a, b| {
            let key = |p: &Value| {
                (
                    p["risk_parts"].as_f64().unwrap_or(f64::MAX),
                    p["completion_minute"].as_u64().unwrap_or(u64::MAX),
                )
            };
            let (ra, ca) = key(a);
            let (rb, cb) = key(b);
            ra.total_cmp(&rb).then(ca.cmp(&cb))
        })
        .map(|p| p["id"].clone())
        .unwrap_or(Value::Null);

    Ok(json!({
        "simulation_id": o.simulation_id,
        "observed_at": now,
        "request": request,
        "assets": assets,
        "plans": plans,
        "recommended": recommended,
        "basis": "observable_state",
        "assumptions": r,
        "provenance": "simulation",
        "limitations": ["Planning model on observed state and assumed resource parameters; not a forecast of the simulated truth"],
    }))
}

// ───────────────────────── DES 案例的工具读取 ─────────────────────────

/// DES 案例（`world.des` 中有可观测快照）的维护、质量、资源、排产工具改读仿真状态；其他工具返回 None，
/// 由 `tools::read` 继续使用原有夹具。
pub(crate) fn read_tool(c: &Case, tool: &str, now: u64) -> Option<Result<ToolResponse>> {
    let o: Observation = serde_json::from_value(c.world.get("des")?.clone()).ok()?;
    let asset = o.machine(&c.asset).cloned();
    let (data, mock_ids): (Result<Value>, [&str; 2]) = match tool {
        "maintenance.assess" => {
            let m = asset?;
            let warning = matches!(m.zone.as_str(), "C" | "D");
            (
                Ok(json!({
                    "asset": m.id,
                    "status": m.status,
                    "vibration_mm_s": m.vibration_mm_s,
                    "zone": m.zone,
                    "condition": if warning { "WARNING" } else { "NORMAL" },
                    "cause_status": "HYPOTHESIS",
                    "continue_using": !warning,
                    "investigation_needed": warning,
                })),
                ["M27", "M03"],
            )
        }
        "quality.assess" => {
            let m = asset?;
            let low = m.cpk < CPK_THRESHOLD;
            (
                Ok(json!({
                    "asset": m.id,
                    "cpk": m.cpk,
                    "threshold": CPK_THRESHOLD,
                    "risk": if low { "HOLD_PENDING_VALIDATION" } else { "IN_CONTROL" },
                    "root_cause_status": "UNKNOWN",
                    "measurement_status": "simulated_model_value",
                })),
                ["M27", "M04"],
            )
        }
        "resources.check" => (
            Ok(json!({
                "machines": o.machines.iter().map(|m| json!({
                    "id": m.id, "status": m.status, "certified": m.certified, "tool_present": m.tool_present,
                })).collect::<Vec<_>>(),
                "tool_spares": o.resources.tool_spares,
                "tool_transfer_minutes": o.resources.tool_transfer_minutes,
                "maintenance_crews": o.resources.maintenance_crews,
                "resource_version": c.resource_version,
            })),
            ["M27", "M05"],
        ),
        "schedule.simulate" if super::coordination::is_incident(c) => {
            // 协同事项（阶段③b）：对报警设备集合排产；订单取成员插单中交期未到、交期最早的一张。
            // 没有这样的插单时与单设备案例一致，按 500 件、8 小时估算。
            let incident = &c.world["des_incident"];
            let assets: Vec<String> = incident["assets"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let order = incident["orders"].as_array().and_then(|orders| {
                orders
                    .iter()
                    .filter(|p| p["quantity"].is_u64())
                    .filter(|p| p["due_minute"].as_u64().is_some_and(|d| d > o.observed_at))
                    .min_by_key(|o| o["due_minute"].as_u64())
                    .cloned()
            });
            let quantity = order
                .as_ref()
                .and_then(|p| p["quantity"].as_u64())
                .unwrap_or(500);
            let due = order
                .as_ref()
                .and_then(|p| p["due_minute"].as_u64())
                .unwrap_or(o.observed_at + 480);
            let data = whatif_assets(&o, &assets, quantity, due, true).map(|mut v| {
                v["feasible"] = json!(v["recommended"].is_string());
                v["quantity"] = json!(quantity);
                v
            });
            (data, ["M27", "M06"])
        }
        "schedule.simulate" => {
            // 订单数量与交期取自案例中的插单观测；没有插单时按 500 件、8 小时估算。
            let order = c
                .observations
                .iter()
                .rev()
                .find(|ob| ob.event.kind == "order")
                .map(|ob| ob.event.payload.clone());
            let quantity = order
                .as_ref()
                .and_then(|p| p["quantity"].as_u64())
                .unwrap_or(500);
            let due = order
                .as_ref()
                .and_then(|p| p["due_minute"].as_u64())
                .unwrap_or(o.observed_at + 480);
            let request = WhatIfRequest {
                asset: c.asset.clone(),
                quantity,
                due_minute: due,
            };
            let data = whatif(&o, &request).map(|mut v| {
                // 引擎按 `feasible` 决定是否进入人工决策：有可行的推荐方案即为可行。
                v["feasible"] = json!(v["recommended"].is_string());
                v["quantity"] = json!(quantity);
                v
            });
            (data, ["M27", "M06"])
        }
        _ => return None,
    };
    Some(data.map(|data| ToolResponse {
        status: "ok".into(),
        data,
        evidence_refs: vec![],
        source: "DES".into(),
        observed_at: o.observed_at,
        valid_until: now + TOOL_VALID_MINUTES,
        provenance: "simulation".into(),
        mock_ids: mock_ids.iter().map(|s| s.to_string()).collect(),
        limitations: vec![
            "Discrete-event simulation with assumed parameters; not industrial validation".into(),
        ],
    }))
}
