//! 把 DES 仿真接入 MOM 引擎（阶段①：开环）。
//!
//! 创建时一次性生成场景（开环，agent 的处置不影响后续事件），把事件放入持久队列，
//! 由现有虚拟时钟逐分钟投递到 `ingest`；标准答案单独保存，只能经 `des_review_truth` 读取。
//! 阶段②：每个虚拟分钟把可观测快照写入 DES 案例的 `world.des`，供工具读取；
//! 提供只读的工厂状态、趋势预测和 what-if（只用可观测状态，见 `observe.rs`）。
//! 闭环（执行回写仿真世界）在阶段③实现。

use super::observe::{self, Observation};
use super::scenario::{replay, run_with_attempt};
use super::{
    DesSimulationConfig, FactoryConfig, GroundTruth, Pattern, ScenarioConfig, WhatIfRequest,
};
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
    /// 重放所需的输入（工厂、场景、选定的尝试序号）。含机制开关，只在快照中保存，不进入任何视图。
    /// 阶段①创建的仿真没有此字段，阶段②的查询对它们返回错误。
    #[serde(default)]
    pub(crate) replay: Option<DesReplay>,
}

/// 重放一个 DES 仿真所需的全部输入。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DesReplay {
    pub factory: FactoryConfig,
    pub scenario: ScenarioConfig,
    pub attempt: u32,
}

impl DesReplay {
    /// 当前时刻的可观测快照（确定性重放后截取）。
    fn observe(&self, simulation_id: &str, now: u64) -> Observation {
        let telemetry = replay(&self.factory, &self.scenario, self.attempt);
        observe::observe(
            simulation_id,
            &self.factory,
            &telemetry,
            self.scenario.start_minute,
            now,
        )
    }
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
        let (run, attempt) = run_with_attempt(&factory, &scenario)?;

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
            pattern: scenario.pattern.clone(),
            start_minute: scenario.start_minute,
            horizon_minutes: scenario.horizon_minutes,
            queue,
            replay: Some(DesReplay {
                factory,
                scenario: scenario.clone(),
                attempt,
            }),
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

    fn des_replay(&self, simulation_id: &str) -> Result<&DesReplay> {
        self.state
            .des_simulations
            .get(simulation_id)
            .context("unknown DES simulation")?
            .replay
            .as_ref()
            .context("DES simulation was created before replay support; create a new one")
    }

    /// 比较候选方案（A 继续用、B 批后停机维修、C 立即停机维修）。只读，只用可观测状态。
    pub fn des_whatif(&self, simulation_id: &str, request: &WhatIfRequest) -> Result<Value> {
        let o = self
            .des_replay(simulation_id)?
            .observe(simulation_id, self.now());
        observe::whatif(&o, request)
    }

    /// 工厂当前可观测状态（不含隐状态与标准答案）。
    pub fn des_factory_view(&self, simulation_id: &str) -> Result<Value> {
        let o = self
            .des_replay(simulation_id)?
            .observe(simulation_id, self.now());
        Ok(observe::factory_view(&o))
    }

    /// 按观测线性趋势外推到阈值的估计。
    pub fn des_forecast(&self, simulation_id: &str) -> Result<Value> {
        let r = self.des_replay(simulation_id)?;
        let telemetry = replay(&r.factory, &r.scenario, r.attempt);
        Ok(observe::forecast(
            simulation_id,
            &r.factory,
            &telemetry,
            r.scenario.start_minute,
            self.now(),
        ))
    }

    /// 把当前可观测快照写入各 DES 仿真关联的活动案例（`world.des`），供工具读取。
    /// 案例通过批次号前缀 `DES-n:` 关联；没有关联案例的仿真不重放。
    fn refresh_des_observations(&mut self, now: u64) -> Result<()> {
        let mut updates: Vec<(String, Value)> = vec![];
        for sim in self.state.des_simulations.values() {
            let Some(r) = &sim.replay else { continue };
            let prefix = format!("{}:", sim.id);
            let linked: Vec<String> = self
                .state
                .cases
                .values()
                .filter(|c| {
                    c.batch.starts_with(&prefix)
                        && !c.cancelled
                        && !matches!(c.stage.as_str(), "CLOSED" | "CANCELLED")
                })
                .map(|c| c.id.clone())
                .collect();
            if linked.is_empty() {
                continue;
            }
            let snapshot = serde_json::to_value(r.observe(&sim.id, now))?;
            updates.extend(linked.into_iter().map(|id| (id, snapshot.clone())));
        }
        if updates.is_empty() {
            return Ok(());
        }
        self.update(|s, _| {
            for (id, snapshot) in updates {
                if let Some(c) = s.cases.get_mut(&id) {
                    c.world["des"] = snapshot;
                }
            }
            Ok(())
        })
    }

    /// 从 DES 案例的排产方案中选定一个可行方案；选定后原有批准作废，需要重新批准（阶段③）。
    ///
    /// 骨架：阶段③尚未实现。
    pub fn select_des_plan(&mut self, _case_id: &str, _plan_id: &str) -> Result<()> {
        anyhow::bail!("DES plan selection is not implemented yet")
    }

    /// 向 DES 仿真注入故障（阶段③）。
    ///
    /// 骨架：阶段③尚未实现。
    pub fn des_inject_fault(
        &mut self,
        _simulation_id: &str,
        _fault: super::DesFault,
    ) -> Result<()> {
        anyhow::bail!("DES fault injection is not implemented yet")
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
        self.refresh_des_observations(now)
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
