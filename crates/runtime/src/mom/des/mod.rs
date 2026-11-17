//! 车间离散事件仿真（DES），阶段①：内核与场景生成器（开环）。
//!
//! 设计见 `docs/mom/DES_CLOSED_LOOP.md`（§4 场景生成器、§5 内核、§15 验收用例）。
//!
//! 模块组成：
//! - `rng`：可复现的独立随机流；
//! - `physics`：规律模型（磨损、振动、刀具、尺寸与 Cpk），常数均为建模假设；
//! - `scenario`：场景模式、逐分钟开环仿真、报警发生器、标准答案导出；
//! - `engine_link`：在 MOM 引擎中创建 DES 仿真、按虚拟时钟投递事件、只向审核层提供标准答案。
//!
//! 验收用例：`crates/runtime/tests/mom_des.rs`（RT-DES-01～05、16、17）。
//!
//! 约定：
//! - 仿真按虚拟分钟推进（与引擎时钟一致），遥测时间戳以秒记录；对外事件沿用 `EventInput.occurred_at`（虚拟分钟）。
//! - 阶段①只产生 `order` / `vibration` / `quality` 三类事件（决策 D7）。
//! - 标准答案（`GroundTruth`）只供审核层读取，不得进入 agent 视图、人类视图、工具返回或仿真列表。
mod engine_link;
mod observe;
mod physics;
mod rng;
mod scenario;

pub use engine_link::DesSimulation;
pub(crate) use observe::read_tool;

use super::EventInput;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 过程能力警戒门槛（与现有 S1 夹具一致；是行业惯例而非普适标准，见设计文档 §4.2）。
pub const CPK_THRESHOLD: f64 = 1.33;

/// 一台设备的静态配置（L0 结构层）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineSpec {
    /// 设备编号，例如 `CNC-XX`。
    pub id: String,
    /// 所在产线，例如 `Line-B`。同一产线共享订单与来料批次。
    pub line: String,
    /// 单件加工节拍（秒）。
    pub cycle_seconds: u32,
}

/// 工厂结构配置。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactoryConfig {
    pub machines: Vec<MachineSpec>,
    /// 资源与工艺约束（阶段②）。旧配置没有此字段时取默认值。
    #[serde(default)]
    pub resources: ResourceConfig,
}

/// 工厂设备数上限（阶段②，RT-DES-21）。
pub const MAX_MACHINES: usize = 32;

/// 资源与工艺约束，供 what-if 计划模型和资源查询使用。默认值取自 S1 夹具，均为建模假设。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceConfig {
    /// 未获该产品工艺认证的设备。
    pub uncertified: Vec<String>,
    /// 当前缺少指定刀具（φ32 铣刀）的设备。
    pub missing_tool: Vec<String>,
    /// 仓库可调拨的合格备用刀具数量。
    pub tool_spares: u32,
    /// 刀具调拨加装配耗时（分钟）。
    pub tool_transfer_minutes: u64,
    /// 主轴轴承更换等维修耗时（分钟）。
    pub maintenance_minutes: u64,
    /// 可用维修班组数量。
    pub maintenance_crews: u32,
    /// "完成当前批次后再停机"所需时间（分钟）。
    pub finish_batch_minutes: u64,
    /// 成品检验放行耗时（分钟）。
    pub inspection_minutes: u64,
    /// 插单可占用的设备产能比例（0～1）。
    pub urgent_capacity_share: f64,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            uncertified: vec![],
            missing_tool: vec!["CNC-XZ".into()],
            tool_spares: 1,
            tool_transfer_minutes: 45,
            maintenance_minutes: 240,
            maintenance_crews: 1,
            finish_batch_minutes: 30,
            inspection_minutes: 20,
            urgent_capacity_share: 1.0,
        }
    }
}

/// what-if 请求：为某个报警设备上的订单比较候选方案。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhatIfRequest {
    /// 报警设备（方案 A/B/C 针对它继续用、批后停、立即停）。
    pub asset: String,
    /// 需要完成的件数。
    pub quantity: u64,
    /// 交期（虚拟分钟）。
    pub due_minute: u64,
}

impl FactoryConfig {
    /// 默认工厂：Line-B 上的 CNC-XX、CNC-XY、CNC-XZ，节拍 90 秒（建模假设，见设计文档 §16）。
    pub fn line_b_default() -> Self {
        let machine = |id: &str| MachineSpec {
            id: id.into(),
            line: "Line-B".into(),
            cycle_seconds: 90,
        };
        Self {
            machines: vec![machine("CNC-XX"), machine("CNC-XY"), machine("CNC-XZ")],
            resources: ResourceConfig::default(),
        }
    }
}

/// 因果机制开关（设计文档 §4.3）。"主轴磨损→振动""插单→负荷→磨损速度"总是开启，不在此列。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mechanisms {
    /// 主轴磨损影响尺寸均值与方差。
    pub spindle_wear_to_dimension: bool,
    /// 刀具后刀面磨损影响尺寸均值。
    pub tool_wear_to_dimension: bool,
    /// 来料批次特性影响尺寸均值（混杂因素）。
    pub material_lot_to_dimension: bool,
}

/// 场景模式：对抽样施加的约束（设计文档 §4.4）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pattern {
    /// P0 固定剧本：复现 S1 的插单、振动、质量报警时序。
    Script,
    /// P1 并发簇：任一 `window_minutes` 窗口内至少 `min_disruptions` 个扰动，且至少一对真实关联。
    ConcurrentCluster {
        window_minutes: u64,
        min_disruptions: u32,
    },
    /// P2 诱饵：同一设备振动与质量报警接近出现，但质量报警的真实原因不是主轴磨损。
    Decoy,
    /// P3 级联：插单加大负荷，加速磨损，进而出现振动与质量报警。
    Cascade,
    /// P4 资源竞争：至少一种资源被两个以上事件的处置共同需要。
    ResourceContention,
    /// P5 安静对照：无扰动，Cpk 保持在门槛以上。
    Quiet,
    /// 不施加模式约束，只按规律模型抽样（用于机制与随机流测试）。
    Unconstrained,
}

/// 一次场景的输入。相同输入必须得到逐字节相同的输出（设计文档 §4.5）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    pub seed: u64,
    pub pattern: Pattern,
    /// 仿真起点（虚拟分钟，与引擎时钟一致；S1 为 497，即 08:17）。
    pub start_minute: u64,
    /// 仿真时长（虚拟分钟）。
    pub horizon_minutes: u64,
    /// 指定因果机制；`None` 表示由场景生成器按 seed 与模式抽样。
    pub mechanisms: Option<Mechanisms>,
}

/// 单条报警的标准答案。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AlarmTruth {
    pub event_id: String,
    pub kind: String,
    pub asset: String,
    /// 真实根因，例如 `spindle_wear:CNC-XX`、`tool_wear:CNC-XY`、`material_lot:LOT-3`、`customer_order:ORD-2`。
    pub root_causes: Vec<String>,
}

/// 事件之间的真实关联，例如 `load_accelerates_wear`、`common_cause:spindle_wear`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrueRelation {
    pub from_event: String,
    pub to_event: String,
    pub relation: String,
}

/// 多个事件的处置共同需要的资源（刀具、维修班组等）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceContention {
    pub resource: String,
    pub event_ids: Vec<String>,
}

/// 标准答案：由因果结构和运行轨迹导出，只供审核层读取。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroundTruth {
    pub mechanisms: Mechanisms,
    pub alarms: Vec<AlarmTruth>,
    pub relations: Vec<TrueRelation>,
    pub contentions: Vec<ResourceContention>,
}

/// 时间序列的一个采样点。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub at_seconds: u64,
    pub value: f64,
}

/// 单台设备的遥测（含隐状态，仅测试与审核层使用）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineTelemetry {
    /// 主轴磨损量（隐状态，无量纲）。
    pub spindle_wear: Vec<Sample>,
    /// 振动有效值（mm/s）。
    pub vibration_mm_s: Vec<Sample>,
    /// 过程能力 Cpk（阶段①为模型值，见设计文档 §4.2 修订说明）。
    pub cpk: Vec<Sample>,
    /// 当前刀具已加工件数（MES 刀具计数器，可观测）。
    pub tool_parts: Vec<Sample>,
}

/// 全部设备的遥测，按设备编号索引。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub machines: BTreeMap<String, MachineTelemetry>,
}

/// 一次开环运行的结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DesRun {
    /// 报警发生器与外生事件产生的观测，按时间排序，交给引擎 `ingest`。
    pub events: Vec<EventInput>,
    pub truth: GroundTruth,
    pub telemetry: Telemetry,
}

/// 生成场景并开环运行到 `start_minute + horizon_minutes`。
///
/// - 相同的工厂与场景配置，输出逐字节相同（RT-DES-01）。
/// - 场景模式不满足时换下一个尝试序号重新抽样，最多 256 次；仍不满足或事先就能判定不可能时，
///   返回含 "constraint" 的错误，不放宽约束（RT-DES-16）。
/// - `Pattern::Script` 是固定剧本夹具：事件按 S1 时序给出，不由仿真产生。
pub fn run_open_loop(factory: &FactoryConfig, scenario: &ScenarioConfig) -> Result<DesRun> {
    scenario::run(factory, scenario)
}

/// 在引擎中创建 DES 仿真的配置。与脚本模式的 `SimulationConfig` 分开，
/// 以保证脚本模式的接口与现有测试不变（设计文档 §6.1）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesSimulationConfig {
    pub name: String,
    /// `None` 使用 `FactoryConfig::line_b_default()`。
    pub factory: Option<FactoryConfig>,
    pub scenario: ScenarioConfig,
}
