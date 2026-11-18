//! 可观测状态与趋势预测（阶段②；阶段⑤a-1 迁入模拟工厂 `an-des`）。
//!
//! 原则（设计文档 §6.4 阶段②修订说明）：这里只使用"现场看得见"的量——设备运行状态、振动、Cpk、
//! 刀具计数和资源配置——不读取主轴磨损等隐状态，也不复跑带随机数的仿真。

use crate::{FactoryConfig, Intervention, ResourceConfig, Telemetry, CPK_THRESHOLD};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// ISO 20816-3 第 2 组刚性安装的分区边界（mm/s），与报警发生器一致；待对照标准原文。
const ZONE_BOUNDARIES: [(f64, &str); 3] = [(1.4, "A"), (2.8, "B"), (4.5, "C")];
/// 趋势预测使用的观测窗口（分钟）。
const FORECAST_WINDOW_MINUTES: usize = 60;

/// 一台设备在某一时刻的可观测状态。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineObservation {
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
pub struct Observation {
    pub simulation_id: String,
    pub observed_at: u64,
    pub machines: Vec<MachineObservation>,
    pub resources: ResourceConfig,
}

impl Observation {
    pub fn machine(&self, id: &str) -> Option<&MachineObservation> {
        self.machines.iter().find(|m| m.id == id)
    }
}

pub fn zone(vibration: f64) -> &'static str {
    ZONE_BOUNDARIES
        .iter()
        .find(|(limit, _)| vibration < *limit)
        .map_or("D", |(_, z)| z)
}

pub fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

/// 遥测中 `now` 对应的样本下标（仿真开始前取起点，结束后取终点）。
pub fn index_at(telemetry: &Telemetry, start_minute: u64, now: u64) -> usize {
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
pub fn observe(
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
pub fn factory_view(o: &Observation) -> Value {
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
pub fn forecast(
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

pub fn is_alarmed(m: &MachineObservation) -> bool {
    matches!(m.zone.as_str(), "C" | "D") || m.cpk < CPK_THRESHOLD
}
