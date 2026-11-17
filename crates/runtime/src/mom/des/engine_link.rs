//! 把 DES 仿真接入 MOM 引擎（阶段①：开环）。
//!
//! 创建时一次性生成场景（开环，agent 的处置不影响后续事件），把事件放入持久队列，
//! 由现有虚拟时钟逐分钟投递到 `ingest`；标准答案单独保存，只能经 `des_review_truth` 读取。
//! 闭环（执行回写仿真世界）在阶段③实现。

use super::{run_open_loop, DesSimulationConfig, FactoryConfig, GroundTruth, Pattern};
use crate::mom::{Engine, ScheduledEvent, ScheduledPayload};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 同一引擎中 DES 仿真数量上限（与脚本仿真一致）。
const MAX_DES_SIMULATIONS: usize = 50;

/// 引擎中的一个 DES 仿真。不含标准答案和机制开关。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesSimulation {
    pub id: String,
    pub name: String,
    pub seed: u64,
    pub pattern: Pattern,
    pub start_minute: u64,
    pub horizon_minutes: u64,
    pub queue: Vec<ScheduledEvent>,
}

impl Engine {
    /// 创建 DES 仿真，返回仿真编号（`DES-n`）。
    ///
    /// 事件编号和批次号都加上仿真编号前缀，避免与脚本仿真或其他 DES 仿真的事件合并或冲突。
    /// 起点取配置起点与当前虚拟时钟中较晚者。
    pub fn create_des_simulation(&mut self, config: DesSimulationConfig) -> Result<String> {
        ensure!(
            !config.name.trim().is_empty() && config.name.len() <= 120,
            "name required, max 120 bytes"
        );
        ensure!(
            self.state.des_simulations.len() < MAX_DES_SIMULATIONS,
            "DES simulation capacity reached"
        );
        let mut scenario = config.scenario.clone();
        scenario.start_minute = scenario.start_minute.max(self.now());
        let factory = config
            .factory
            .clone()
            .unwrap_or_else(FactoryConfig::line_b_default);
        let run = run_open_loop(&factory, &scenario)?;

        let id = format!("DES-{}", self.state.des_simulations.len() + 1);
        let prefix = |s: &str| format!("{id}:{s}");
        let queue = run
            .events
            .into_iter()
            .map(|mut event| {
                event.id = prefix(&event.id);
                event.batch = prefix(&event.batch);
                ScheduledEvent {
                    id: event.id.clone(),
                    due_at: event.occurred_at,
                    status: "PENDING".into(),
                    payload: ScheduledPayload::Observation { event },
                    error: None,
                }
            })
            .collect();
        let mut truth: GroundTruth = run.truth;
        for a in &mut truth.alarms {
            a.event_id = prefix(&a.event_id);
        }
        for r in &mut truth.relations {
            r.from_event = prefix(&r.from_event);
            r.to_event = prefix(&r.to_event);
        }
        for c in &mut truth.contentions {
            for e in &mut c.event_ids {
                *e = prefix(e);
            }
        }
        let sim = DesSimulation {
            id: id.clone(),
            name: config.name,
            seed: scenario.seed,
            pattern: scenario.pattern,
            start_minute: scenario.start_minute,
            horizon_minutes: scenario.horizon_minutes,
            queue,
        };
        self.update(|s, _| {
            s.des_simulations.insert(id.clone(), sim);
            s.des_truth.insert(id.clone(), truth);
            Ok(())
        })?;
        Ok(id)
    }

    /// 读取某个 DES 仿真的标准答案。只供审核层使用，不进入 agent 视图、人类视图或仿真列表。
    pub fn des_review_truth(&self, simulation_id: &str) -> Result<Value> {
        let truth = self
            .state
            .des_truth
            .get(simulation_id)
            .context("unknown DES simulation")?;
        Ok(serde_json::to_value(truth)?)
    }

    /// 比较候选方案（A 继续用、B 批后停机维修、C 立即停机维修），只读，只用可观测状态。
    ///
    /// 骨架：阶段②尚未实现。
    pub fn des_whatif(
        &self,
        _simulation_id: &str,
        _request: &super::WhatIfRequest,
    ) -> Result<Value> {
        anyhow::bail!("DES what-if is not implemented yet")
    }

    /// 工厂当前可观测状态（不含隐状态与标准答案）。
    ///
    /// 骨架：阶段②尚未实现。
    pub fn des_factory_view(&self, _simulation_id: &str) -> Result<Value> {
        anyhow::bail!("DES factory view is not implemented yet")
    }

    /// 按观测趋势外推到阈值的估计。
    ///
    /// 骨架：阶段②尚未实现。
    pub fn des_forecast(&self, _simulation_id: &str) -> Result<Value> {
        anyhow::bail!("DES forecast is not implemented yet")
    }

    /// 投递到期的 DES 事件。由 `deliver_scheduled` 在每个虚拟分钟调用。
    pub(in crate::mom) fn deliver_des(&mut self, now: u64) -> Result<()> {
        let due: Vec<(String, ScheduledEvent)> = self
            .state
            .des_simulations
            .values()
            .flat_map(|sim| {
                sim.queue
                    .iter()
                    .filter(|e| e.status == "PENDING" && e.due_at <= now)
                    .map(|e| (sim.id.clone(), e.clone()))
            })
            .collect();
        for (sim_id, item) in due {
            let result = match item.payload {
                ScheduledPayload::Observation { event } => self.ingest(event).map(|_| ()),
                ScheduledPayload::WorldChange { .. } => Ok(()),
            };
            let (status, error) = match result {
                Ok(()) => ("APPLIED", None),
                Err(e) => ("FAILED", Some(e.to_string())),
            };
            self.update(|s, _| {
                let sim = s
                    .des_simulations
                    .get_mut(&sim_id)
                    .context("DES simulation missing")?;
                let entry = sim
                    .queue
                    .iter_mut()
                    .find(|e| e.id == item.id)
                    .context("DES event missing")?;
                entry.status = status.into();
                entry.error = error.clone();
                Ok(())
            })?;
        }
        Ok(())
    }

    /// 仿真列表中的 DES 部分：只列出配置摘要和事件队列，不含标准答案与机制开关。
    pub(in crate::mom) fn des_simulations_view(&self) -> Value {
        json!(self
            .state
            .des_simulations
            .values()
            .map(|sim| json!({
                "simulation_id": sim.id,
                "name": sim.name,
                "seed": sim.seed,
                "pattern": sim.pattern,
                "start_minute": sim.start_minute,
                "horizon_minutes": sim.horizon_minutes,
                "pending": sim.queue.iter().filter(|e| e.status == "PENDING").count(),
                "queue": sim.queue,
                "provenance": "simulation",
            }))
            .collect::<Vec<_>>())
    }
}
