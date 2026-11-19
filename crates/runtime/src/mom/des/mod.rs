//! 车间离散事件仿真（DES），阶段①：内核与场景生成器（开环）。
//!
//! 设计见 `docs/mom/DES_CLOSED_LOOP.md`（§4 场景生成器、§5 内核、§15 验收用例）。
//!
//! 模块组成（阶段⑤a-1 起，规律模型、随机流、场景生成、可观测量与标准答案在独立 crate `an-des` 中）：
//! - `engine_link`：在 MOM 引擎中创建 DES 仿真、按虚拟时钟投递事件、只向审核层提供标准答案；
//! - `observe`：what-if 计划模型与 DES 案例的工具读取（MOM 业务逻辑）；
//! - `coordination`：按报警触发的模板与产线协同事项；
//! - `review`、`batch`：审核层与批量实验。
//!
//! 验收用例：`crates/runtime/tests/mom_des.rs`（RT-DES-01～05、16、17）。
//!
//! 约定：
//! - 仿真按虚拟分钟推进（与引擎时钟一致），遥测时间戳以秒记录；对外事件沿用 `EventInput.occurred_at`（虚拟分钟）。
//! - 阶段①只产生 `order` / `vibration` / `quality` 三类事件（决策 D7）。
//! - 标准答案（`GroundTruth`）只供审核层读取，不得进入 agent 视图、人类视图、工具返回或仿真列表。
pub mod batch;
mod coordination;
mod engine_link;
mod observe;
pub mod remote;
pub mod review;

pub(crate) use coordination::des_template;
pub use engine_link::DesSimulation;
pub(crate) use observe::read_tool;

use super::EventInput;
/// 仿真产生的事件转成引擎观测后使用同一结构。
pub(crate) type DesEvent = EventInput;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// 阶段⑤a-1：仿真内核迁入独立 crate `an-des`（模拟工厂）。这里重新导出原有名称，MOM 侧代码与测试不变。
pub(crate) use an_des::{observe as kernel_observe, rng, scenario, Intervention};
pub use an_des::{
    AlarmTruth, CoordinationConfig, DesFault, FactoryConfig, GroundTruth, MachineSpec,
    MachineTelemetry, Mechanisms, Pattern, ResourceConfig, ResourceContention, Sample,
    ScenarioConfig, SimulationConfig as DesSimulationConfig, Telemetry, TrueRelation,
    CPK_THRESHOLD, MAX_MACHINES,
};

impl From<an_des::FactoryEvent> for EventInput {
    fn from(e: an_des::FactoryEvent) -> Self {
        Self {
            id: e.id,
            scenario: e.scenario,
            kind: e.kind,
            source: e.source,
            occurred_at: e.occurred_at,
            batch: e.batch,
            customer: e.customer,
            asset: e.asset,
            payload: e.payload,
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

/// 一次开环运行的结果（事件已转为引擎观测结构 `EventInput`）。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DesRun {
    /// 报警发生器与外生事件产生的观测，按时间排序，交给引擎 `ingest`。
    pub events: Vec<EventInput>,
    pub truth: GroundTruth,
    pub telemetry: Telemetry,
}

impl From<an_des::DesRun> for DesRun {
    fn from(r: an_des::DesRun) -> Self {
        Self {
            events: r.events.into_iter().map(EventInput::from).collect(),
            truth: r.truth,
            telemetry: r.telemetry,
        }
    }
}

/// 生成场景并开环运行（见 `an_des::run_open_loop`），事件转为引擎观测结构。
pub fn run_open_loop(factory: &FactoryConfig, scenario: &ScenarioConfig) -> Result<DesRun> {
    an_des::run_open_loop(factory, scenario).map(DesRun::from)
}
