//! 场景生成器与开环仿真（设计文档 §4、§5）。
//!
//! 做法：先按场景模式给出抽样"倾向"（哪些设备接近报警、插单多快、来料波动多大），
//! 再按规律模型逐分钟推进工厂状态；事件由状态越限产生，标准答案由机制开关和运行轨迹导出。
//! 若某次抽样不满足模式约束，换下一个尝试序号重新抽样（拒绝抽样，次数有上限），
//! 超过上限就报错，不放宽约束。

use super::physics::{self, Contributions};
use super::rng::Rng;
use super::{
    AlarmTruth, DesRun, FactoryConfig, GroundTruth, Intervention, MachineTelemetry, Mechanisms,
    Pattern, ResourceContention, Sample, ScenarioConfig, Telemetry, TrueRelation, CPK_THRESHOLD,
};
use crate::mom::EventInput;
use anyhow::{bail, ensure, Result};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// 拒绝抽样的最大尝试次数。
const MAX_ATTEMPTS: u32 = 256;
/// 每次创建的确定性工作量预算（含全部拒绝抽样尝试），单位：设备·分钟（R-20261117-L1，移植自 PR #85）。
/// 一条最大规模轨迹（32 × 2881）在预算内；多次完整轨迹则不在。
const MAX_MACHINE_MINUTES: u64 = 100_000;
/// 仿真最长时长（虚拟分钟），与脚本仿真扰动窗口一致（48 小时）。
const MAX_HORIZON_MINUTES: u64 = 2880;
/// 两个报警相隔不超过此值且需要同一资源时，记为资源竞争。
const CONTENTION_WINDOW_MINUTES: u64 = 120;
/// 插单在此时间内都视为可能加快了同线设备的磨损。
const LOAD_RELATION_WINDOW_MINUTES: u64 = 720;
/// 诱饵模式中振动与质量报警的最大间隔。
const DECOY_WINDOW_MINUTES: u64 = 60;
/// 插单的交期（下单后多少分钟）。
const ORDER_DUE_MINUTES: u64 = 480;
const CUSTOMERS: [&str; 3] = ["A", "B", "C"];

/// 一次抽样的"倾向"：由场景模式决定，不改变规律模型本身。
struct Knobs {
    /// 接近报警阈值的设备数量（"热点设备"）。
    hot_count: usize,
    /// 热点设备的初始主轴磨损区间。
    hot_wear: (f64, f64),
    /// 其他设备的初始主轴磨损区间。
    base_wear: (f64, f64),
    /// 插单平均到达间隔（分钟）；`None` 表示不插单。
    order_mean_minutes: Option<f64>,
    /// 是否在起点就插入第一张订单。
    first_order_at_start: bool,
    /// 来料批次偏移的标准差（mm）与平均批次时长（分钟）。
    lot_sigma_mm: f64,
    lot_mean_minutes: f64,
    /// 刀具初始已用量，占计划换刀间隔的比例区间。
    tool_used: (f64, f64),
    /// 热点设备当前刀具的实际寿命（名义寿命的倍数）区间，用于构造"磨损偏快的刀"。
    hot_tool_life: Option<(f64, f64)>,
    hot_tool_used: Option<(f64, f64)>,
    /// 固定某台设备的初始主轴磨损（只用于 P0 固定剧本，使背景遥测与剧本事件一致）。
    forced_wear: Option<(&'static str, f64)>,
}

fn knobs(pattern: &Pattern) -> Knobs {
    let base = Knobs {
        hot_count: 0,
        hot_wear: (1.10, 1.16),
        base_wear: (0.2, 1.15),
        order_mean_minutes: Some(240.0),
        first_order_at_start: false,
        lot_sigma_mm: 0.002,
        lot_mean_minutes: 180.0,
        tool_used: (0.0, 1.0),
        hot_tool_life: None,
        hot_tool_used: None,
        forced_wear: None,
    };
    match pattern {
        Pattern::Unconstrained | Pattern::Script => base,
        Pattern::ConcurrentCluster { .. } => Knobs {
            hot_count: usize::MAX,
            order_mean_minutes: Some(90.0),
            first_order_at_start: true,
            lot_sigma_mm: 0.003,
            ..base
        },
        Pattern::Decoy => Knobs {
            hot_count: 1,
            base_wear: (0.2, 0.9),
            lot_sigma_mm: 0.007,
            lot_mean_minutes: 90.0,
            hot_tool_life: Some((0.6, 0.75)),
            hot_tool_used: Some((0.6, 0.9)),
            ..base
        },
        Pattern::Cascade => Knobs {
            hot_count: 1,
            hot_wear: (1.05, 1.17),
            base_wear: (0.2, 0.9),
            order_mean_minutes: Some(360.0),
            first_order_at_start: true,
            ..base
        },
        Pattern::ResourceContention => Knobs {
            hot_count: 2,
            base_wear: (0.2, 0.9),
            ..base
        },
        Pattern::Quiet => Knobs {
            base_wear: (0.1, 0.5),
            order_mean_minutes: None,
            lot_sigma_mm: 0.001,
            tool_used: (0.0, 0.2),
            ..base
        },
    }
}

fn spindle_only() -> Mechanisms {
    Mechanisms {
        spindle_wear_to_dimension: true,
        tool_wear_to_dimension: false,
        material_lot_to_dimension: false,
    }
}

/// 机制开关：指定时直接使用；未指定时按模式抽样。
fn mechanisms(cfg: &ScenarioConfig, attempt: u32) -> Mechanisms {
    if let Some(m) = cfg.mechanisms {
        return m;
    }
    let mut r = Rng::stream(cfg.seed, attempt, "mechanisms");
    match cfg.pattern {
        Pattern::Script | Pattern::Cascade => spindle_only(),
        Pattern::Decoy => {
            // 诱饵：主轴机制关闭，质量问题另有原因（来料、刀具或两者）。
            let u = r.unit();
            Mechanisms {
                spindle_wear_to_dimension: false,
                tool_wear_to_dimension: u >= 1.0 / 3.0,
                material_lot_to_dimension: u < 2.0 / 3.0,
            }
        }
        _ => Mechanisms {
            spindle_wear_to_dimension: r.unit() < 0.5,
            tool_wear_to_dimension: r.unit() < 0.5,
            material_lot_to_dimension: r.unit() < 0.5,
        },
    }
}

/// 在开始抽样前就能判定"不可能满足"的约束，直接报错。
fn precheck(factory: &FactoryConfig, cfg: &ScenarioConfig) -> Result<()> {
    ensure!(
        !factory.machines.is_empty(),
        "factory needs at least one machine"
    );
    let mut ids = BTreeSet::new();
    for m in &factory.machines {
        ensure!(
            !m.id.trim().is_empty() && !m.line.trim().is_empty(),
            "machine id and line are required"
        );
        ensure!(
            m.cycle_seconds > 0,
            "machine {} cycle must be positive",
            m.id
        );
        ensure!(ids.insert(m.id.as_str()), "duplicate machine {}", m.id);
    }
    ensure!(
        factory.machines.len() <= super::MAX_MACHINES,
        "factory has {} machines; at most {} machines are supported",
        factory.machines.len(),
        super::MAX_MACHINES
    );
    ensure!(
        (1..=MAX_HORIZON_MINUTES).contains(&cfg.horizon_minutes),
        "horizon must be 1..={MAX_HORIZON_MINUTES} minutes"
    );
    let unsatisfiable = |why: &str| -> Result<()> {
        bail!(
            "scenario constraint not satisfiable: {:?}: {why}",
            cfg.pattern
        )
    };
    match (&cfg.pattern, cfg.mechanisms) {
        (
            Pattern::ConcurrentCluster {
                window_minutes,
                min_disruptions,
            },
            _,
        ) if *window_minutes == 0 || *min_disruptions == 0 => {
            unsatisfiable("window and minimum disruptions must be positive")
        }
        (Pattern::Decoy, Some(m))
            if m.spindle_wear_to_dimension
                || !(m.tool_wear_to_dimension || m.material_lot_to_dimension) =>
        {
            unsatisfiable("decoy needs the spindle mechanism off and another dimension cause on")
        }
        (Pattern::Cascade | Pattern::Script, Some(m)) if !m.spindle_wear_to_dimension => {
            unsatisfiable("this pattern needs the spindle wear mechanism")
        }
        (Pattern::Script, _) if !ids.contains("CNC-XX") => {
            unsatisfiable("script pattern needs machine CNC-XX")
        }
        (Pattern::Script, _) if cfg.horizon_minutes < 14 => {
            unsatisfiable("script pattern needs at least 14 minutes")
        }
        _ => Ok(()),
    }
}

/// 生成场景并开环运行。见 `super::run_open_loop` 的说明。
pub(super) fn run(factory: &FactoryConfig, cfg: &ScenarioConfig) -> Result<DesRun> {
    run_with_attempt(factory, cfg).map(|(run, _)| run)
}

/// 同 `run`，另返回满足约束的尝试序号，供引擎按需回放同一条轨迹（阶段②）。
pub(super) fn run_with_attempt(
    factory: &FactoryConfig,
    cfg: &ScenarioConfig,
) -> Result<(DesRun, u32)> {
    precheck(factory, cfg)?;
    if cfg.pattern == Pattern::Script {
        return Ok((script(factory, cfg, &[], 0), 0));
    }
    let attempts = if cfg.pattern == Pattern::Unconstrained {
        1
    } else {
        MAX_ATTEMPTS
    };
    let per_attempt = factory.machines.len() as u64 * (cfg.horizon_minutes + 1);
    let attempts = attempts.min((MAX_MACHINE_MINUTES / per_attempt.max(1)).max(1) as u32);
    for attempt in 0..attempts {
        let mech = mechanisms(cfg, attempt);
        let sim = simulate(factory, cfg, &knobs(&cfg.pattern), mech, attempt, &[], 0);
        let truth = derive_truth(&sim, mech);
        if satisfies(&cfg.pattern, &sim, &truth) {
            let run = DesRun {
                events: sim.emitted.into_iter().map(|e| e.event).collect(),
                truth,
                telemetry: sim.telemetry,
            };
            return Ok((run, attempt));
        }
    }
    bail!(
        "scenario constraint not satisfiable: {:?} within {attempts} attempts and {MAX_MACHINE_MINUTES} machine-minutes work budget (seed {})",
        cfg.pattern,
        cfg.seed
    )
}

/// 按已选定的尝试序号和已记录的干预重新推演（确定性）。
///
/// 干预只影响其开始时刻之后；随机数的消耗与干预无关，所以干预之前的事件、遥测与标准答案保持不变。
/// 引擎只从遥测中截取可观测的量（振动、Cpk、刀具计数），不对外暴露主轴磨损。
///
/// `until_minute`（绝对虚拟分钟）晚于场景结束时，世界状态继续推演到该时刻，供执行后的查询使用；
/// 报警事件仍只在场景窗口内产生。
pub(super) fn rerun(
    factory: &FactoryConfig,
    cfg: &ScenarioConfig,
    attempt: u32,
    interventions: &[Intervention],
    until_minute: u64,
) -> DesRun {
    #[cfg(test)]
    REVIEW_REPLAY_COUNT.with(|count| count.set(count.get() + 1));
    if cfg.pattern == Pattern::Script {
        return script(factory, cfg, interventions, until_minute);
    }
    let mech = mechanisms(cfg, attempt);
    let sim = simulate(
        factory,
        cfg,
        &knobs(&cfg.pattern),
        mech,
        attempt,
        interventions,
        until_minute,
    );
    let truth = derive_truth(&sim, mech);
    DesRun {
        events: sim.emitted.into_iter().map(|e| e.event).collect(),
        truth,
        telemetry: sim.telemetry,
    }
}

// 测试用：统计重新推演次数（R-20261117-M1 的可计数验收，RT-DES-22/23）。
#[cfg(test)]
thread_local! {
    pub(super) static REVIEW_REPLAY_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// ───────────────────────── 逐分钟仿真 ─────────────────────────

/// 仿真产生的一条事件及其内部记录（根因、所需资源、关联用的上下文）。
struct Emitted {
    event: EventInput,
    line: String,
    machine: Option<String>,
    causes: Vec<String>,
    resources: Vec<String>,
}

struct Simulated {
    emitted: Vec<Emitted>,
    telemetry: Telemetry,
}

struct PlannedOrder {
    at: u64,
    id: String,
    quantity: u64,
    customer: String,
}

struct Line {
    name: String,
    /// 新订单的产品挂在第一台设备上（与 S1 夹具一致）。
    lead_machine: String,
    orders: Vec<PlannedOrder>,
    next_order: usize,
    /// 插单积压件数；大于 0 时同线设备超负荷运行。
    backlog: f64,
    /// 当前在制订单（批次号、客户），没有插单时为常规库存生产。
    current: Option<(String, String)>,
    /// 来料批次：（开始分钟偏移，尺寸偏移 mm）。
    lots: Vec<(u64, f64)>,
}

impl Line {
    fn lot_at(&self, minute: u64) -> (usize, f64) {
        let idx = self
            .lots
            .iter()
            .rposition(|(at, _)| *at <= minute)
            .unwrap_or(0);
        (idx, self.lots.get(idx).map_or(0.0, |l| l.1))
    }
    fn batch_and_customer(&self) -> (String, String) {
        self.current
            .clone()
            .unwrap_or_else(|| (format!("{}-STOCK", self.name), "STOCK".into()))
    }
}

struct Machine {
    id: String,
    line: usize,
    parts_per_minute: f64,
    wear: f64,
    /// 当前刀具已加工件数与实际寿命（件）。
    tool_parts: f64,
    tool_life: f64,
    running: bool,
    vibration_over: u32,
    vibration_armed: bool,
    cpk_armed: bool,
    cpk_recovered_minutes: u32,
    wear_rng: Rng,
    vibration_rng: Rng,
    tool_rng: Rng,
    telemetry: MachineTelemetry,
}

/// 新刀的实际寿命（件）：名义寿命 × 对数正态离散。
fn sample_tool_life(r: &mut Rng) -> f64 {
    physics::TOOL_NOMINAL_LIFE_PARTS * (physics::TOOL_LIFE_LOG_SIGMA * r.normal()).exp()
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

fn simulate(
    factory: &FactoryConfig,
    cfg: &ScenarioConfig,
    k: &Knobs,
    mech: Mechanisms,
    attempt: u32,
    interventions: &[Intervention],
    until_minute: u64,
) -> Simulated {
    let horizon = cfg.horizon_minutes;
    // 执行后世界继续推演到查询时刻（最多再推演一个最大时长），事件只保留场景窗口内的。
    let total = horizon.max(
        until_minute
            .saturating_sub(cfg.start_minute)
            .min(2 * MAX_HORIZON_MINUTES),
    );
    // 产线按设备在配置中首次出现的顺序排列。
    let mut lines: Vec<Line> = vec![];
    for m in &factory.machines {
        if lines.iter().all(|l| l.name != m.line) {
            lines.push(plan_line(&m.line, &m.id, cfg, k, attempt));
        }
    }
    // 热点设备：从全部设备中按 seed 选出，不影响其他随机流。
    let n = factory.machines.len();
    let mut order: Vec<usize> = (0..n).collect();
    let mut pick = Rng::stream(cfg.seed, attempt, "hot-machines");
    for i in 0..n {
        let j = i + (pick.next_u64() % (n - i) as u64) as usize;
        order.swap(i, j);
    }
    let hot: BTreeSet<usize> = order.into_iter().take(k.hot_count.min(n)).collect();

    let mut machines: Vec<Machine> = factory
        .machines
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let mut init = Rng::stream(cfg.seed, attempt, &format!("machine:{}:init", spec.id));
            let is_hot = hot.contains(&i);
            let (lo, hi) = if is_hot { k.hot_wear } else { k.base_wear };
            let sampled = init.uniform(lo, hi);
            let wear = match k.forced_wear {
                Some((id, w)) if id == spec.id => w,
                _ => sampled,
            };
            let (tlo, thi) = match (is_hot, k.hot_tool_used) {
                (true, Some(range)) => range,
                _ => k.tool_used,
            };
            let used = init.uniform(tlo, thi);
            let mut tool_rng = Rng::stream(cfg.seed, attempt, &format!("machine:{}:tool", spec.id));
            let tool_life = match (is_hot, k.hot_tool_life) {
                (true, Some((lo, hi))) => physics::TOOL_NOMINAL_LIFE_PARTS * init.uniform(lo, hi),
                _ => sample_tool_life(&mut tool_rng),
            };
            let planned = physics::TOOL_NOMINAL_LIFE_PARTS * physics::TOOL_PLANNED_CHANGE_FRACTION;
            // 起点处于受控状态：当前刀具不超过实际寿命的一定比例。
            let tool_parts =
                (used * planned).min(physics::TOOL_START_MAX_LIFE_FRACTION * tool_life);
            Machine {
                id: spec.id.clone(),
                line: lines.iter().position(|l| l.name == spec.line).unwrap_or(0),
                parts_per_minute: 60.0 / f64::from(spec.cycle_seconds),
                wear,
                tool_parts,
                tool_life,
                running: true,
                vibration_over: 0,
                vibration_armed: true,
                cpk_armed: true,
                cpk_recovered_minutes: 0,
                wear_rng: Rng::stream(cfg.seed, attempt, &format!("machine:{}:wear", spec.id)),
                vibration_rng: Rng::stream(
                    cfg.seed,
                    attempt,
                    &format!("machine:{}:vibration", spec.id),
                ),
                tool_rng,
                telemetry: MachineTelemetry::default(),
            }
        })
        .collect();

    let mut emitted: Vec<Emitted> = vec![];
    let push = |emitted: &mut Vec<Emitted>,
                kind: &str,
                at: u64,
                asset: &str,
                line: &Line,
                payload: serde_json::Value,
                machine: Option<String>,
                causes: Vec<String>,
                resources: Vec<String>| {
        let (batch, customer) = line.batch_and_customer();
        let source = match kind {
            "order" => "MES",
            "vibration" => "EAM",
            _ => "QMS",
        };
        emitted.push(Emitted {
            event: EventInput {
                id: format!("E{:04}", emitted.len() + 1),
                scenario: "s1".into(),
                kind: kind.into(),
                source: source.into(),
                occurred_at: at,
                batch,
                customer,
                asset: asset.into(),
                payload,
            },
            line: line.name.clone(),
            machine,
            causes,
            resources,
        });
    };

    for minute in 0..=total {
        let t = cfg.start_minute + minute;
        // 1) 外生事件：插单到达。
        for line in &mut lines {
            while line
                .orders
                .get(line.next_order)
                .is_some_and(|o| o.at == minute)
            {
                let (id, quantity, customer) = {
                    let o = &line.orders[line.next_order];
                    (o.id.clone(), o.quantity, o.customer.clone())
                };
                line.backlog += quantity as f64;
                let payload = json!({"line":line.name,"order":id,"quantity":quantity,"due_minute":t + ORDER_DUE_MINUTES});
                line.current = Some((id, customer));
                let lead = line.lead_machine.clone();
                line.next_order += 1;
                push(
                    &mut emitted,
                    "order",
                    t,
                    &lead,
                    line,
                    payload,
                    None,
                    vec![],
                    vec![],
                );
            }
        }
        // 2) 设备状态推进与报警发生器。
        let mut produced = vec![0.0; lines.len()];
        for m in &mut machines {
            // 无论设备状态如何都先抽样，保证随机流的消耗与负荷、机制无关。
            let unit_increment = m.wear_rng.gamma(physics::WEAR_SHAPE_PER_MIN);
            let noise = m.vibration_rng.normal();
            let line = &lines[m.line];
            // 干预（阶段③）：维修期间停机；维修结束的那一分钟起主轴磨损复位、恢复运行、报警锁存解除。
            let mut in_maintenance = false;
            for iv in interventions {
                if let Intervention::Maintenance {
                    machine,
                    start_minute,
                    end_minute,
                } = iv
                {
                    if machine == &m.id && (*start_minute..*end_minute).contains(&t) {
                        in_maintenance = true;
                    }
                    if machine == &m.id && *end_minute == t {
                        m.wear = physics::POST_REPAIR_WEAR;
                        m.running = true;
                        m.vibration_armed = true;
                        m.vibration_over = 0;
                        m.cpk_armed = true;
                        m.cpk_recovered_minutes = 0;
                    }
                }
            }
            let active = m.running && !in_maintenance;
            if active && minute > 0 {
                let load = if line.backlog > 0.0 {
                    physics::OVERLOAD_FACTOR
                } else {
                    1.0
                };
                m.wear += unit_increment * physics::WEAR_RATE_PER_MIN * load
                    / physics::WEAR_SHAPE_PER_MIN;
                m.tool_parts += m.parts_per_minute;
                // 到计划换刀点（预防性换刀）或实际寿命（磨损偏快的刀被迫更换）时换新刀。
                let planned =
                    physics::TOOL_NOMINAL_LIFE_PARTS * physics::TOOL_PLANNED_CHANGE_FRACTION;
                if m.tool_parts >= planned.min(m.tool_life) {
                    m.tool_parts = 0.0;
                    m.tool_life = sample_tool_life(&mut m.tool_rng);
                }
                produced[m.line] += m.parts_per_minute;
            }
            if m.wear >= physics::FUNCTIONAL_FAILURE_WEAR {
                m.running = false;
            }
            let active = active && m.running;
            let vibration = if active {
                physics::vibration_mean(m.wear) + physics::VIBRATION_NOISE_MM_S * noise
            } else {
                0.0
            };
            let (lot_idx, lot_offset) = line.lot_at(minute);
            let c = physics::contributions(&mech, m.wear, m.tool_parts / m.tool_life, lot_offset);
            let cpk = physics::cpk(&c);
            let at_seconds = t * 60;
            m.telemetry.spindle_wear.push(Sample {
                at_seconds,
                value: m.wear,
            });
            m.telemetry.vibration_mm_s.push(Sample {
                at_seconds,
                value: vibration,
            });
            m.telemetry.cpk.push(Sample {
                at_seconds,
                value: cpk,
            });
            m.telemetry.tool_parts.push(Sample {
                at_seconds,
                value: m.tool_parts,
            });

            // 振动：进入 C 区并持续若干分钟才报警；报警后锁存，回到 B 区以下才解除。
            if active && vibration >= physics::VIBRATION_ZONE_C_MM_S {
                m.vibration_over += 1;
            } else {
                m.vibration_over = 0;
            }
            if m.vibration_armed && m.vibration_over >= physics::VIBRATION_SUSTAIN_MINUTES {
                m.vibration_armed = false;
                let payload =
                    json!({"line":line.name,"vibration_mm_s":round(vibration, 2),"zone":"C"});
                let crew = format!("maintenance_crew:{}", line.name);
                push(
                    &mut emitted,
                    "vibration",
                    t,
                    &m.id,
                    line,
                    payload,
                    Some(m.id.clone()),
                    vec![format!("spindle_wear:{}", m.id)],
                    vec![crew],
                );
            } else if !m.vibration_armed && active && vibration < physics::VIBRATION_REARM_MM_S {
                m.vibration_armed = true;
            }

            // 质量：Cpk 从门槛以上跌到以下时报警一次；回升并保持一段时间才解除锁存。
            // 停机期间没有加工件，不做质量判定。
            if active && cpk < CPK_THRESHOLD {
                m.cpk_recovered_minutes = 0;
                if m.cpk_armed {
                    m.cpk_armed = false;
                    let lot_id = format!("{}-LOT-{}", line.name, lot_idx + 1);
                    let (causes, resources) = quality_causes(&mech, &c, &m.id, &line.name, &lot_id);
                    let payload = json!({"line":line.name,"cpk":round(cpk.clamp(0.0, 5.0), 3),"threshold":CPK_THRESHOLD});
                    push(
                        &mut emitted,
                        "quality",
                        t,
                        &m.id,
                        line,
                        payload,
                        Some(m.id.clone()),
                        causes,
                        resources,
                    );
                }
            } else if active && !m.cpk_armed {
                if cpk >= CPK_THRESHOLD + physics::CPK_REARM_MARGIN {
                    m.cpk_recovered_minutes += 1;
                    if m.cpk_recovered_minutes >= physics::CPK_REARM_MINUTES {
                        m.cpk_armed = true;
                        m.cpk_recovered_minutes = 0;
                    }
                } else {
                    m.cpk_recovered_minutes = 0;
                }
            }
        }
        // 3) 插单积压按实际产量消化。
        for (line, p) in lines.iter_mut().zip(produced) {
            if line.backlog > 0.0 {
                line.backlog = (line.backlog - p).max(0.0);
                if line.backlog == 0.0 {
                    line.current = None;
                }
            }
        }
    }

    emitted.retain(|e| e.event.occurred_at <= cfg.start_minute + horizon);
    Simulated {
        emitted,
        telemetry: Telemetry {
            machines: machines.into_iter().map(|m| (m.id, m.telemetry)).collect(),
        },
    }
}

/// 预先抽样一条产线的插单到达（泊松过程）和来料批次。
fn plan_line(name: &str, lead: &str, cfg: &ScenarioConfig, k: &Knobs, attempt: u32) -> Line {
    let horizon = cfg.horizon_minutes as f64;
    let mut orders = vec![];
    if let Some(mean) = k.order_mean_minutes {
        let mut r = Rng::stream(cfg.seed, attempt, &format!("line:{name}:orders"));
        let mut t = if k.first_order_at_start {
            0.0
        } else {
            r.exponential(mean)
        };
        while t <= horizon {
            let n = orders.len() + 1;
            orders.push(PlannedOrder {
                at: t.floor() as u64,
                id: format!("ORD-{name}-{n}"),
                quantity: 100 + r.next_u64() % 501,
                customer: CUSTOMERS[(r.next_u64() % 3) as usize].into(),
            });
            t += r.exponential(mean);
        }
    }
    let mut r = Rng::stream(cfg.seed, attempt, &format!("line:{name}:lots"));
    // 起点处于受控状态：当前在用的来料批次已检验合格，偏移为 0；之后到达的批次才有偏移。
    let mut lots = vec![(0, 0.0)];
    let mut t = r.exponential(k.lot_mean_minutes);
    while t <= horizon {
        lots.push((t.floor() as u64, k.lot_sigma_mm * r.normal()));
        t += r.exponential(k.lot_mean_minutes);
    }
    Line {
        name: name.into(),
        lead_machine: lead.into(),
        orders,
        next_order: 0,
        backlog: 0.0,
        current: None,
        lots,
    }
}

/// 质量报警的根因与处置所需资源：贡献达到下限的机制才计入。
fn quality_causes(
    m: &Mechanisms,
    c: &Contributions,
    machine: &str,
    line: &str,
    lot_id: &str,
) -> (Vec<String>, Vec<String>) {
    let candidates = [
        (
            m.spindle_wear_to_dimension,
            c.spindle_shift,
            format!("spindle_wear:{machine}"),
            format!("maintenance_crew:{line}"),
        ),
        (
            m.tool_wear_to_dimension,
            c.tool_shift,
            format!("tool_wear:{machine}"),
            "tool:phi32-mill".to_string(),
        ),
        (
            m.material_lot_to_dimension,
            c.lot_shift,
            format!("material_lot:{lot_id}"),
            format!("inspection:{line}"),
        ),
    ];
    let mut causes = vec![];
    let mut resources = vec![];
    for (on, shift, cause, resource) in &candidates {
        if *on && shift.abs() >= physics::CAUSE_MIN_SHIFT_MM {
            causes.push(cause.clone());
            resources.push(resource.clone());
        }
    }
    if causes.is_empty() {
        // 偏移都很小（例如只有方差增大），取贡献最大的已开启机制。
        if let Some((_, _, cause, resource)) = candidates
            .iter()
            .filter(|x| x.0)
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        {
            causes.push(cause.clone());
            resources.push(resource.clone());
        }
    }
    (causes, resources)
}

// ───────────────────────── 标准答案 ─────────────────────────

fn derive_truth(sim: &Simulated, mech: Mechanisms) -> GroundTruth {
    let ev = &sim.emitted;
    let alarms = ev
        .iter()
        .filter(|e| e.event.kind != "order")
        .map(|e| AlarmTruth {
            event_id: e.event.id.clone(),
            kind: e.event.kind.clone(),
            asset: e.event.asset.clone(),
            root_causes: e.causes.clone(),
        })
        .collect();

    let mut relations = vec![];
    for v in ev.iter().filter(|e| e.event.kind == "vibration") {
        // 插单加大同线负荷，加快磨损。
        for o in ev.iter().filter(|o| {
            o.event.kind == "order"
                && o.line == v.line
                && o.event.occurred_at < v.event.occurred_at
                && v.event.occurred_at - o.event.occurred_at <= LOAD_RELATION_WINDOW_MINUTES
        }) {
            relations.push(TrueRelation {
                from_event: o.event.id.clone(),
                to_event: v.event.id.clone(),
                relation: "load_accelerates_wear".into(),
            });
        }
    }
    for q in ev.iter().filter(|e| e.event.kind == "quality") {
        let machine = q.machine.as_deref().unwrap_or_default();
        // 振动与质量报警的共同原因：同一主轴磨损。
        if q.causes.contains(&format!("spindle_wear:{machine}")) {
            // 取该设备在质量报警之前最近的一次振动报警。
            if let Some(v) = ev.iter().rev().find(|v| {
                v.event.kind == "vibration"
                    && v.machine.as_deref() == Some(machine)
                    && v.event.occurred_at < q.event.occurred_at
            }) {
                relations.push(TrueRelation {
                    from_event: v.event.id.clone(),
                    to_event: q.event.id.clone(),
                    relation: "common_cause:spindle_wear".into(),
                });
            }
        }
    }
    // 同一来料批次造成的多个质量报警。
    let mut by_lot: BTreeMap<&str, Vec<&Emitted>> = BTreeMap::new();
    for q in ev.iter().filter(|e| e.event.kind == "quality") {
        for c in q.causes.iter().filter(|c| c.starts_with("material_lot:")) {
            by_lot.entry(c.as_str()).or_default().push(q);
        }
    }
    for qs in by_lot.values() {
        for pair in qs.windows(2) {
            relations.push(TrueRelation {
                from_event: pair[0].event.id.clone(),
                to_event: pair[1].event.id.clone(),
                relation: "common_cause:material_lot".into(),
            });
        }
    }

    // 资源竞争：不同设备的报警在相近时间需要同一资源。
    let mut by_resource: BTreeMap<&str, Vec<&Emitted>> = BTreeMap::new();
    for e in ev.iter().filter(|e| e.event.kind != "order") {
        for r in &e.resources {
            by_resource.entry(r.as_str()).or_default().push(e);
        }
    }
    let mut contentions = vec![];
    for (resource, list) in by_resource {
        let mut group: Vec<&Emitted> = vec![];
        let mut flush = |group: &mut Vec<&Emitted>| {
            let machines: BTreeSet<_> = group.iter().filter_map(|e| e.machine.as_deref()).collect();
            if machines.len() >= 2 {
                contentions.push(ResourceContention {
                    resource: resource.to_string(),
                    event_ids: group.iter().map(|e| e.event.id.clone()).collect(),
                });
            }
            group.clear();
        };
        for e in list {
            if group.last().is_some_and(|p| {
                e.event.occurred_at - p.event.occurred_at > CONTENTION_WINDOW_MINUTES
            }) {
                flush(&mut group);
            }
            group.push(e);
        }
        flush(&mut group);
    }

    GroundTruth {
        mechanisms: mech,
        alarms,
        relations,
        contentions,
    }
}

// ───────────────────────── 模式约束 ─────────────────────────

fn satisfies(pattern: &Pattern, sim: &Simulated, truth: &GroundTruth) -> bool {
    let ev: Vec<&EventInput> = sim.emitted.iter().map(|e| &e.event).collect();
    let first = |kind: &str, machine: &str| {
        ev.iter()
            .find(|e| e.kind == kind && e.asset == machine)
            .copied()
    };
    let cause_of = |id: &str| {
        truth
            .alarms
            .iter()
            .find(|a| a.event_id == id)
            .map(|a| a.root_causes.clone())
            .unwrap_or_default()
    };
    match pattern {
        Pattern::Unconstrained | Pattern::Script => true,
        Pattern::ConcurrentCluster {
            window_minutes,
            min_disruptions,
        } => {
            let densest = ev
                .iter()
                .map(|a| {
                    ev.iter()
                        .filter(|b| {
                            (a.occurred_at..a.occurred_at + window_minutes).contains(&b.occurred_at)
                        })
                        .count()
                })
                .max()
                .unwrap_or(0);
            densest >= *min_disruptions as usize && !truth.relations.is_empty()
        }
        Pattern::Decoy => {
            !truth.mechanisms.spindle_wear_to_dimension
                && sim.telemetry.machines.keys().any(|m| {
                    match (first("vibration", m), first("quality", m)) {
                        (Some(v), Some(q)) => {
                            let causes = cause_of(&q.id);
                            q.occurred_at.abs_diff(v.occurred_at) <= DECOY_WINDOW_MINUTES
                                && !causes.is_empty()
                                && !causes.contains(&format!("spindle_wear:{m}"))
                        }
                        _ => false,
                    }
                })
        }
        Pattern::Cascade => {
            let at = |id: &str| {
                ev.iter()
                    .find(|e| e.id == id)
                    .map(|e| (e.kind.as_str(), e.occurred_at))
            };
            truth
                .relations
                .iter()
                .filter(|l| l.relation == "load_accelerates_wear")
                .any(|l| {
                    truth
                        .relations
                        .iter()
                        .filter(|c| {
                            c.relation == "common_cause:spindle_wear" && c.from_event == l.to_event
                        })
                        .any(
                            |c| match (at(&l.from_event), at(&l.to_event), at(&c.to_event)) {
                                (
                                    Some(("order", o)),
                                    Some(("vibration", v)),
                                    Some(("quality", q)),
                                ) => o < v && v < q,
                                _ => false,
                            },
                        )
                })
        }
        Pattern::ResourceContention => !truth.contentions.is_empty(),
        Pattern::Quiet => {
            ev.is_empty()
                && sim
                    .telemetry
                    .machines
                    .values()
                    .all(|t| t.cpk.iter().all(|s| s.value >= CPK_THRESHOLD))
        }
    }
}

// ───────────────────────── P0 固定剧本 ─────────────────────────

/// P0：复现 S1 夹具的时序（插单 08:17、振动 +6、质量 +14），批次 B77、客户 A。
/// 这是固定剧本夹具，事件时刻不由仿真产生；背景遥测用安静运行，但 CNC-XX 的初始主轴磨损固定为 1.62，
/// 使它在剧本期间振动处于 C 区、Cpk 低于门槛，与剧本事件一致；事件中的读数取自该遥测。
fn script(
    factory: &FactoryConfig,
    cfg: &ScenarioConfig,
    interventions: &[Intervention],
    until_minute: u64,
) -> DesRun {
    let k = Knobs {
        forced_wear: Some(("CNC-XX", 1.62)),
        ..knobs(&Pattern::Quiet)
    };
    let quiet = simulate(
        factory,
        cfg,
        &k,
        spindle_only(),
        0,
        interventions,
        until_minute,
    );
    let start = cfg.start_minute;
    let xx = quiet.telemetry.machines.get("CNC-XX");
    let reading = |series: fn(&MachineTelemetry) -> &Vec<Sample>, minute: usize, digits: i32| {
        xx.and_then(|t| series(t).get(minute))
            .map_or(0.0, |s| round(s.value.clamp(0.0, 5.0), digits))
    };
    let vibration = reading(|t| &t.vibration_mm_s, 6, 2);
    let cpk = reading(|t| &t.cpk, 14, 3);
    let event =
        |id: &str, kind: &str, source: &str, at: u64, payload: serde_json::Value| EventInput {
            id: id.into(),
            scenario: "s1".into(),
            kind: kind.into(),
            source: source.into(),
            occurred_at: at,
            batch: "B77".into(),
            customer: "A".into(),
            asset: "CNC-XX".into(),
            payload,
        };
    let events = vec![
        event(
            "P0-order",
            "order",
            "MES",
            start,
            json!({"line":"Line-B","order":"ORD-A-URGENT","quantity":500,"due_minute":start + 463}),
        ),
        event(
            "P0-vibration",
            "vibration",
            "EAM",
            start + 6,
            json!({"line":"Line-B","vibration_mm_s":vibration,"zone":"C"}),
        ),
        event(
            "P0-quality",
            "quality",
            "QMS",
            start + 14,
            json!({"line":"Line-B","cpk":cpk,"threshold":CPK_THRESHOLD}),
        ),
    ];
    let cause = vec!["spindle_wear:CNC-XX".to_string()];
    let alarm = |id: &str, kind: &str| AlarmTruth {
        event_id: id.into(),
        kind: kind.into(),
        asset: "CNC-XX".into(),
        root_causes: cause.clone(),
    };
    let relation = |from: &str, to: &str, relation: &str| TrueRelation {
        from_event: from.into(),
        to_event: to.into(),
        relation: relation.into(),
    };
    DesRun {
        events,
        truth: GroundTruth {
            mechanisms: spindle_only(),
            alarms: vec![
                alarm("P0-vibration", "vibration"),
                alarm("P0-quality", "quality"),
            ],
            relations: vec![
                relation("P0-order", "P0-vibration", "load_accelerates_wear"),
                relation("P0-vibration", "P0-quality", "common_cause:spindle_wear"),
            ],
            contentions: vec![],
        },
        telemetry: quiet.telemetry,
    }
}
