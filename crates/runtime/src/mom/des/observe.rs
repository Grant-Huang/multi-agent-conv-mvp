//! 阶段②：可观测状态、趋势预测、what-if 计划模型，以及 DES 案例的工具读取。
//!
//! 原则（设计文档 §6.4 阶段②修订说明）：这里只使用"现场看得见"的量——设备运行状态、振动、Cpk、
//! 刀具计数和资源配置——不读取主轴磨损等隐状态，也不复跑带随机数的仿真。
//! 因此 what-if 是计划工具，结果可能与之后仿真世界的真实演化不同，这正是被测系统需要面对的不确定性。

use super::{FactoryConfig, Intervention, ResourceConfig, Telemetry, WhatIfRequest, CPK_THRESHOLD};
use crate::mom::tools::ToolResponse;
use crate::mom::Case;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// ISO 20816-3 第 2 组刚性安装的分区边界（mm/s），与报警发生器一致；待对照标准原文。
const ZONE_BOUNDARIES: [(f64, &str); 3] = [(1.4, "A"), (2.8, "B"), (4.5, "C")];
/// what-if 最长推演时间（分钟）与请求上限。
const WHATIF_HORIZON_MINUTES: u64 = 2880;
const MAX_WHATIF_QUANTITY: u64 = 100_000;
/// 趋势预测使用的观测窗口（分钟）。
const FORECAST_WINDOW_MINUTES: usize = 60;
/// 工具结果的有效期（分钟），与 S1 夹具的快照有效期一致。
const TOOL_VALID_MINUTES: u64 = 5;

/// 一台设备在某一时刻的可观测状态。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct MachineObservation {
    pub id: String,
    pub line: String,
    pub cycle_seconds: u32,
    /// running / stopped（停机时振动读数为 0）。
    pub status: String,
    pub vibration_mm_s: f64,
    pub zone: String,
    pub cpk: f64,
    /// 当前刀具已加工件数。
    pub tool_parts: f64,
    /// 是否装有该产品所需刀具。
    pub tool_present: bool,
    /// 是否获得该产品的工艺认证。
    pub certified: bool,
}

/// 某个 DES 仿真在某一时刻的可观测快照。写入 DES 案例的 `world.des`，供工具读取。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Observation {
    pub simulation_id: String,
    pub observed_at: u64,
    pub machines: Vec<MachineObservation>,
    pub resources: ResourceConfig,
}

impl Observation {
    fn machine(&self, id: &str) -> Option<&MachineObservation> {
        self.machines.iter().find(|m| m.id == id)
    }
}

fn zone(vibration: f64) -> &'static str {
    ZONE_BOUNDARIES
        .iter()
        .find(|(limit, _)| vibration < *limit)
        .map_or("D", |(_, z)| z)
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

/// 遥测中 `now` 对应的样本下标（仿真开始前取起点，结束后取终点）。
fn index_at(telemetry: &Telemetry, start_minute: u64, now: u64) -> usize {
    let len = telemetry
        .machines
        .values()
        .map(|t| t.cpk.len())
        .min()
        .unwrap_or(0);
    (now.saturating_sub(start_minute) as usize).min(len.saturating_sub(1))
}

/// 从遥测中截取 `now` 时刻的可观测状态。
///
/// 阶段③：已到达的刀具调拨使该设备有刀；已发起的调拨占用备件；备件丢失故障后可用备件为 0。
pub(super) fn observe(
    simulation_id: &str,
    factory: &FactoryConfig,
    telemetry: &Telemetry,
    start_minute: u64,
    now: u64,
    interventions: &[Intervention],
    spares_lost: bool,
) -> Observation {
    let i = index_at(telemetry, start_minute, now);
    let transfers: Vec<(&str, u64)> = interventions
        .iter()
        .filter_map(|iv| match iv {
            Intervention::ToolTransfer {
                machine,
                arrive_minute,
            } => Some((machine.as_str(), *arrive_minute)),
            Intervention::Maintenance { .. } => None,
        })
        .collect();
    let arrived = |id: &str| transfers.iter().any(|(m, at)| *m == id && *at <= now);
    let mut resources = factory.resources.clone();
    resources.missing_tool.retain(|m| !arrived(m));
    resources.tool_spares = if spares_lost {
        0
    } else {
        resources.tool_spares.saturating_sub(transfers.len() as u32)
    };
    let machines = factory
        .machines
        .iter()
        .filter_map(|spec| {
            let t = telemetry.machines.get(&spec.id)?;
            let vibration = t.vibration_mm_s.get(i)?.value;
            Some(MachineObservation {
                id: spec.id.clone(),
                line: spec.line.clone(),
                cycle_seconds: spec.cycle_seconds,
                status: if vibration > 0.0 {
                    "running"
                } else {
                    "stopped"
                }
                .into(),
                vibration_mm_s: round(vibration, 2),
                zone: zone(vibration).into(),
                cpk: round(t.cpk.get(i)?.value, 3),
                tool_parts: round(t.tool_parts.get(i).map_or(0.0, |s| s.value), 1),
                tool_present: !resources.missing_tool.contains(&spec.id),
                certified: !factory.resources.uncertified.contains(&spec.id),
            })
        })
        .collect();
    Observation {
        simulation_id: simulation_id.into(),
        observed_at: now,
        machines,
        resources,
    }
}

/// 工厂状态视图（HTTP `GET /api/mom/factory`）。
pub(super) fn factory_view(o: &Observation) -> Value {
    json!({
        "simulation_id": o.simulation_id,
        "observed_at": o.observed_at,
        "machines": o.machines,
        "resources": o.resources,
        "provenance": "simulation",
        "limitations": ["Discrete-event simulation with assumed parameters; not industrial validation"],
    })
}

/// 最小二乘斜率（每分钟）。
fn slope(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if values.len() < 2 {
        return 0.0;
    }
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = values.iter().sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, y) in values.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

/// 按最近一段观测的线性趋势，估计到达阈值还需多少分钟（HTTP `GET /api/mom/factory/forecast`）。
/// 只用振动与 Cpk 的观测值；已越过阈值为 0，趋势不朝阈值方向则为 null。
pub(super) fn forecast(
    simulation_id: &str,
    factory: &FactoryConfig,
    telemetry: &Telemetry,
    start_minute: u64,
    now: u64,
) -> Value {
    let i = index_at(telemetry, start_minute, now);
    let from = (i + 1).saturating_sub(FORECAST_WINDOW_MINUTES);
    let machines: Vec<Value> = factory
        .machines
        .iter()
        .filter_map(|spec| {
            let t = telemetry.machines.get(&spec.id)?;
            let window = |series: &Vec<super::Sample>| -> Vec<f64> {
                series
                    .get(from..=i)
                    .unwrap_or_default()
                    .iter()
                    .map(|s| s.value)
                    .collect()
            };
            let v = window(&t.vibration_mm_s);
            let c = window(&t.cpk);
            let (v_now, c_now) = (*v.last()?, *c.last()?);
            let (v_slope, c_slope) = (slope(&v), slope(&c));
            let to_vibration = if v_now >= 2.8 {
                Some(0.0)
            } else if v_slope > 0.0 {
                Some(round((2.8 - v_now) / v_slope, 0))
            } else {
                None
            };
            let to_cpk = if c_now < CPK_THRESHOLD {
                Some(0.0)
            } else if c_slope < 0.0 {
                Some(round((c_now - CPK_THRESHOLD) / -c_slope, 0))
            } else {
                None
            };
            Some(json!({
                "id": spec.id,
                "vibration_mm_s": round(v_now, 2),
                "vibration_slope_per_hour": round(v_slope * 60.0, 3),
                "minutes_to_vibration_zone_c": to_vibration,
                "cpk": round(c_now, 3),
                "cpk_slope_per_hour": round(c_slope * 60.0, 4),
                "minutes_to_cpk_threshold": to_cpk,
            }))
        })
        .collect();
    json!({
        "simulation_id": simulation_id,
        "observed_at": now,
        "method": "linear_trend",
        "window_minutes": FORECAST_WINDOW_MINUTES,
        "machines": machines,
        "provenance": "simulation",
        "limitations": ["Linear extrapolation of observed readings; an estimate, not a prediction of the simulated truth"],
    })
}

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

fn is_alarmed(m: &MachineObservation) -> bool {
    matches!(m.zone.as_str(), "C" | "D") || m.cpk < CPK_THRESHOLD
}

/// 比较 A/B/C 三个方案。只读，不使用随机数，相同输入结果相同。
pub(crate) fn whatif(o: &Observation, request: &WhatIfRequest) -> Result<Value> {
    let now = o.observed_at;
    ensure!(
        (1..=MAX_WHATIF_QUANTITY).contains(&request.quantity),
        "quantity must be 1..={MAX_WHATIF_QUANTITY}"
    );
    ensure!(
        request.due_minute <= now + WHATIF_HORIZON_MINUTES,
        "due_minute must be within {WHATIF_HORIZON_MINUTES} minutes of the current virtual time"
    );
    let asset = o
        .machine(&request.asset)
        .with_context(|| format!("unknown machine {}", request.asset))?;
    let r = &o.resources;
    let share = r.urgent_capacity_share.clamp(0.0, 1.0);
    let rate = |m: &MachineObservation| 60.0 / f64::from(m.cycle_seconds) * share;
    let alarmed = is_alarmed(asset);

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

        // 报警设备自身。
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
        if spec.needs_maintenance {
            if r.maintenance_crews == 0 {
                violations.push("maintenance_crew_unavailable".into());
            } else {
                actions.push(format!("eam.maintenance:{}", asset.id));
            }
        }
        // 同线其他设备分担。
        for m in o
            .machines
            .iter()
            .filter(|m| m.line == asset.line && m.id != asset.id)
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
            "uses": units.iter().map(|u| u.id.clone()).collect::<Vec<_>>(),
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
