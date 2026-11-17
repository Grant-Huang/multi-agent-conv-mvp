//! 把 DES 仿真接入 MOM 引擎（阶段①：开环）。
//!
//! 创建时一次性生成场景（开环，agent 的处置不影响后续事件），把事件放入持久队列，
//! 由现有虚拟时钟逐分钟投递到 `ingest`；标准答案单独保存，只能经 `des_review_truth` 读取。
//! 阶段②：每个虚拟分钟把可观测快照写入 DES 案例的 `world.des`，供工具读取；
//! 提供只读的工厂状态、趋势预测和 what-if（只用可观测状态，见 `observe.rs`）。
//! 阶段③（闭环）：选定方案并执行后，把维修、刀具调拨记为仿真干预，从执行时刻起重新推演，
//! 替换尚未投递的事件与标准答案；动作完成时刻由干预决定；同线报警案例互相关联。

use super::observe::{self, Observation};
use super::scenario::{rerun, run_with_attempt};
use super::{
    DesEvent, DesFault, DesSimulationConfig, FactoryConfig, GroundTruth, Intervention, Pattern,
    ScenarioConfig, WhatIfRequest,
};
use crate::mom::engine::emit;
use crate::mom::{Engine, ScheduledEvent, ScheduledPayload};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// 场景结束后继续推演时，每次多算的分钟数，避免逐分钟重放（R-20261117-M1）。
const EXTENSION_CHUNK_MINUTES: u64 = 720;

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
    /// 已执行动作对仿真世界的干预（阶段③）。
    #[serde(default)]
    pub interventions: Vec<Intervention>,
    /// 是否已注入"备件丢失"故障（阶段③）。
    #[serde(default)]
    pub spares_lost: bool,
    /// 重放遥测的内存缓存（R-20261117-M1）：不序列化，状态克隆之间共享。
    /// 以"已记录的干预数 + 推演到的时刻"为键；干预增加或查询超出已推演范围时才重新推演。
    #[serde(skip)]
    cache: Arc<Mutex<Option<TelemetryCache>>>,
}

/// 一次重新推演得到的遥测及其适用范围。
#[derive(Debug)]
struct TelemetryCache {
    interventions: usize,
    until: u64,
    telemetry: Arc<super::Telemetry>,
}

impl DesReplay {
    fn horizon_end(&self) -> u64 {
        self.scenario.start_minute + self.scenario.horizon_minutes
    }

    /// 把一次推演得到的遥测放进缓存（创建时与执行回调时复用，避免再推演一次）。
    fn seed_cache(&self, until: u64, telemetry: super::Telemetry) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(TelemetryCache {
                interventions: self.interventions.len(),
                until: until.max(self.horizon_end()),
                telemetry: Arc::new(telemetry),
            });
        }
    }

    /// 至少覆盖到 `now` 的遥测（执行干预后可能超出原场景窗口）。
    ///
    /// 命中缓存时不重新推演；场景结束后需要延伸时，一次多推演 `EXTENSION_CHUNK_MINUTES` 分钟。
    fn telemetry(&self, now: u64) -> Arc<super::Telemetry> {
        let horizon_end = self.horizon_end();
        let needed = now.max(horizon_end);
        let mut cache = match self.cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(c) = cache.as_ref() {
            if c.interventions == self.interventions.len() && c.until >= needed {
                return c.telemetry.clone();
            }
        }
        let until = if needed > horizon_end {
            needed + EXTENSION_CHUNK_MINUTES
        } else {
            horizon_end
        };
        let telemetry = Arc::new(
            rerun(
                &self.factory,
                &self.scenario,
                self.attempt,
                &self.interventions,
                until,
            )
            .telemetry,
        );
        *cache = Some(TelemetryCache {
            interventions: self.interventions.len(),
            until,
            telemetry: telemetry.clone(),
        });
        telemetry
    }

    /// 当前时刻的可观测快照（确定性重新推演后截取）。
    fn observe(&self, simulation_id: &str, now: u64) -> Observation {
        observe::observe(
            simulation_id,
            &self.factory,
            &self.telemetry(now),
            self.scenario.start_minute,
            now,
            &self.interventions,
            self.spares_lost,
        )
    }

    /// 已发起的刀具调拨占用的备件数量。
    fn transfers_used(&self) -> u32 {
        self.interventions
            .iter()
            .filter(|iv| matches!(iv, Intervention::ToolTransfer { .. }))
            .count() as u32
    }
}

/// 把仿真事件转成引擎事件队列项：事件编号与批次号加上仿真编号前缀。
fn prefixed_queue(id: &str, events: Vec<DesEvent>) -> Vec<ScheduledEvent> {
    events
        .into_iter()
        .map(|mut event| {
            event.id = format!("{id}:{}", event.id);
            event.batch = format!("{id}:{}", event.batch);
            ScheduledEvent {
                id: event.id.clone(),
                due_at: event.occurred_at,
                status: "PENDING".into(),
                payload: ScheduledPayload::Observation { event },
                error: None,
            }
        })
        .collect()
}

/// 标准答案中的事件编号加上仿真编号前缀，与队列一致。
fn prefixed_truth(id: &str, mut truth: GroundTruth) -> GroundTruth {
    let prefix = |s: &str| format!("{id}:{s}");
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
    truth
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
        let queue = prefixed_queue(&id, run.events);
        let truth = prefixed_truth(&id, run.truth);
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
                interventions: vec![],
                spares_lost: false,
                cache: Arc::default(),
            }),
        };
        // 创建时已经推演过一次，直接放进缓存。
        if let Some(r) = &sim.replay {
            r.seed_cache(
                scenario.start_minute + scenario.horizon_minutes,
                run.telemetry,
            );
        }
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
        let telemetry = r.telemetry(self.now());
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
    pub fn select_des_plan(&mut self, case_id: &str, plan_id: &str) -> Result<()> {
        self.update(|s, events| {
            let now = s.now;
            let c = s.cases.get_mut(case_id).context("unknown case")?;
            ensure!(
                c.world.get("des").is_some_and(Value::is_object),
                "plan selection applies to DES cases only"
            );
            ensure!(
                !c.cancelled && !matches!(c.stage.as_str(), "CLOSED" | "CANCELLED" | "BLOCKED"),
                "case is not open for a decision"
            );
            ensure!(c.actions.is_empty(), "a plan was already executed");
            let proposal = c.proposal.as_mut().context("no schedule proposal yet")?;
            let plan = proposal["plans"]
                .as_array()
                .and_then(|plans| plans.iter().find(|p| p["id"] == plan_id))
                .with_context(|| format!("unknown plan {plan_id}"))?;
            ensure!(
                plan["feasible"] == true,
                "plan {plan_id} is not feasible: {}",
                plan["violations"]
            );
            proposal["selected"] = json!(plan_id);
            c.approvals.clear();
            let revision = c.plan_revision;
            emit(
                c,
                now,
                "PlanSelected",
                json!({"plan": plan_id, "plan_revision": revision}),
                events,
            );
            Ok(())
        })
    }

    /// 向 DES 仿真注入故障（阶段③）。
    pub fn des_inject_fault(&mut self, simulation_id: &str, fault: DesFault) -> Result<()> {
        self.des_replay(simulation_id)?;
        self.update(|s, _| {
            let r = s
                .des_simulations
                .get_mut(simulation_id)
                .and_then(|sim| sim.replay.as_mut())
                .context("unknown DES simulation")?;
            match fault {
                DesFault::ToolSparesLost => r.spares_lost = true,
            }
            Ok(())
        })
    }

    /// `execute` 成功后由引擎调用：DES 案例按选定（未选定时按推荐）方案生成仿真干预，
    /// 从当前时刻起重新推演，并用干预时刻设置动作的完成时刻。脚本案例、非生产范围、重复执行时不做任何事。
    pub(in crate::mom) fn des_after_execute(&mut self, case_id: &str, scope: &str) -> Result<()> {
        if scope != "production" {
            return Ok(());
        }
        let c = self.case(case_id)?;
        let Some(sim_id) = c
            .world
            .get("des")
            .and_then(|d| d["simulation_id"].as_str())
            .map(String::from)
        else {
            return Ok(());
        };
        if c.actions.is_empty()
            || c.stage == "PARTIAL"
            || c.world["des_applied"] == json!(c.plan_revision)
        {
            return Ok(());
        }
        let proposal = c.proposal.clone().context("no schedule proposal")?;
        let plan_id = proposal["selected"]
            .as_str()
            .or(proposal["recommended"].as_str())
            .context("no plan selected or recommended")?
            .to_string();
        let plan = proposal["plans"]
            .as_array()
            .and_then(|plans| plans.iter().find(|p| p["id"] == plan_id.as_str()))
            .cloned()
            .context("selected plan missing")?;
        let asset = c.asset.clone();
        let revision = c.plan_revision;
        let replay = self.des_replay(&sim_id)?.clone();
        let now = self.now();
        let r = replay.factory.resources.clone();

        // 方案 → 干预。
        let mut interventions = vec![];
        let maintenance_start = match plan_id.as_str() {
            "B" => Some(now + r.finish_batch_minutes),
            "C" => Some(now),
            _ => None,
        };
        let maintenance_end = maintenance_start.map(|start| {
            let end = start + r.maintenance_minutes;
            interventions.push(Intervention::Maintenance {
                machine: asset.clone(),
                start_minute: start,
                end_minute: end,
            });
            end
        });
        let mut spares = if replay.spares_lost {
            0
        } else {
            r.tool_spares.saturating_sub(replay.transfers_used())
        };
        let (mut transfer_arrive, mut transfer_failed) = (None, false);
        for machine in plan["prerequisites"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str()?.strip_prefix("tool_transfer:"))
        {
            if spares == 0 {
                transfer_failed = true;
                continue;
            }
            spares -= 1;
            let arrive = now + r.tool_transfer_minutes;
            interventions.push(Intervention::ToolTransfer {
                machine: machine.to_string(),
                arrive_minute: arrive,
            });
            transfer_arrive = Some(arrive);
        }

        // 重新推演：执行时刻之前的事件不变，之后尚未投递的事件与标准答案被替换。
        let mut next = replay;
        next.interventions.extend(interventions.iter().cloned());
        let run = rerun(
            &next.factory,
            &next.scenario,
            next.attempt,
            &next.interventions,
            now,
        );
        // 新干预需要新的缓存（不与旧状态共享），并复用这次推演的遥测。
        next.cache = Arc::default();
        next.seed_cache(now, run.telemetry);
        let future: Vec<ScheduledEvent> = prefixed_queue(&sim_id, run.events)
            .into_iter()
            .filter(|e| e.due_at > now)
            .collect();
        let truth = prefixed_truth(&sim_id, run.truth);

        self.update(|s, events| {
            let sim = s
                .des_simulations
                .get_mut(&sim_id)
                .context("DES simulation missing")?;
            sim.replay = Some(next);
            sim.queue.retain(|e| e.status != "PENDING" || e.due_at <= now);
            let existing: BTreeSet<String> = sim.queue.iter().map(|e| e.id.clone()).collect();
            sim.queue
                .extend(future.into_iter().filter(|e| !existing.contains(&e.id)));
            sim.queue.sort_by(|a, b| (a.due_at, &a.id).cmp(&(b.due_at, &b.id)));
            s.des_truth.insert(sim_id.clone(), truth);

            let c = s.cases.get_mut(case_id).context("unknown case")?;
            for a in c.actions.values_mut() {
                match a.tool.as_str() {
                    "eam.maintenance" => {
                        if let Some(end) = maintenance_end {
                            a.complete_at = end;
                        }
                    }
                    "mro.transfer" if transfer_failed => {
                        a.state = "FAILED".into();
                    }
                    "mro.transfer" => {
                        if let Some(at) = transfer_arrive {
                            a.complete_at = at;
                        }
                    }
                    _ => {}
                }
            }
            if transfer_failed {
                c.stage = "PARTIAL".into();
                c.world["reservations"] = json!({});
                emit(
                    c,
                    now,
                    "CompensationRequired",
                    json!({"released":"reservations","reason":"tool spares unavailable in simulation","irreversible":"accepted maintenance needs operator review","provenance":"simulation"}),
                    events,
                );
            }
            c.world["des_applied"] = json!(revision);
            emit(
                c,
                now,
                "DesPlanApplied",
                json!({"plan": plan_id, "interventions": interventions, "provenance": "simulation"}),
                events,
            );
            Ok(())
        })
    }

    /// 同一 DES 仿真中、同一产线（共用维修班组）的报警案例互相关联（D6），每对只记录一次。
    fn link_related_cases(&mut self, sim_id: &str, case_id: &str) -> Result<()> {
        let Some(replay) = self
            .state
            .des_simulations
            .get(sim_id)
            .and_then(|s| s.replay.as_ref())
        else {
            return Ok(());
        };
        let line_of = |asset: &str| {
            replay
                .factory
                .machines
                .iter()
                .find(|m| m.id == asset)
                .map(|m| m.line.clone())
        };
        let has_alarm = |c: &crate::mom::Case| {
            c.observations
                .iter()
                .any(|o| matches!(o.event.kind.as_str(), "vibration" | "quality"))
        };
        let prefix = format!("{sim_id}:");
        let c = self.case(case_id)?;
        let Some(line) = line_of(&c.asset) else {
            return Ok(());
        };
        if !has_alarm(c) {
            return Ok(());
        }
        let linked = |c: &crate::mom::Case, other: &str| {
            c.world["des_related"]
                .as_array()
                .is_some_and(|l| l.iter().any(|v| v == other))
        };
        let others: Vec<String> = self
            .state
            .cases
            .values()
            .filter(|o| {
                o.id != case_id
                    && o.batch.starts_with(&prefix)
                    && has_alarm(o)
                    && line_of(&o.asset).as_deref() == Some(line.as_str())
                    && !linked(c, &o.id)
            })
            .map(|o| o.id.clone())
            .collect();
        if others.is_empty() {
            return Ok(());
        }
        let reason = format!("shared_resource:maintenance_crew:{line}");
        self.update(|s, events| {
            let now = s.now;
            for other in &others {
                for (a, b) in [(case_id, other.as_str()), (other.as_str(), case_id)] {
                    let c = s.cases.get_mut(a).context("unknown case")?;
                    if !c.world["des_related"].is_array() {
                        c.world["des_related"] = json!([]);
                    }
                    if let Some(list) = c.world["des_related"].as_array_mut() {
                        list.push(json!(b));
                    }
                    emit(
                        c,
                        now,
                        "RelatedCaseLinked",
                        json!({"related_case": b, "reason": reason, "source": "DES topology"}),
                        events,
                    );
                }
            }
            Ok(())
        })
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
                ScheduledPayload::Observation { event } => self
                    .ingest(event)
                    .and_then(|case_id| self.link_related_cases(&sim_id, &case_id)),
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

/// R-20261117-M1 回归（移植自 PR #84，并按阶段③的干预与执行后延伸调整）。
/// 是 `engine_link` 内部的单元测试：重放次数计数器只在单元测试中可见，不计入 runtime 集成测试映射。
#[cfg(test)]
mod review_regressions {
    use super::super::scenario::REVIEW_REPLAY_COUNT;
    use super::*;

    fn large_engine() -> (Engine, String, super::super::Telemetry, FactoryConfig) {
        let mut factory = FactoryConfig::line_b_default();
        while factory.machines.len() < 32 {
            let i = factory.machines.len();
            factory.machines.push(super::super::MachineSpec {
                id: format!("CNC-{i:02}"),
                line: "Line-B".into(),
                cycle_seconds: 90,
            });
        }
        let scenario = ScenarioConfig {
            seed: 1,
            pattern: Pattern::Script,
            start_minute: 497,
            horizon_minutes: 2880,
            mechanisms: None,
        };
        let expected = rerun(&factory, &scenario, 0, &[], 0).telemetry;
        let mut engine = Engine::memory().unwrap();
        let id = engine
            .create_des_simulation(DesSimulationConfig {
                name: "cache regression".into(),
                factory: Some(factory.clone()),
                scenario,
            })
            .unwrap();
        engine.advance(497).unwrap();
        (engine, id, expected, factory)
    }

    fn replays() -> usize {
        REVIEW_REPLAY_COUNT.with(|c| c.get())
    }

    #[test]
    fn rt_des_22_advance_2880_reuses_replay_and_preserves_observations() {
        let (mut e, id, telemetry, factory) = large_engine();
        REVIEW_REPLAY_COUNT.with(|c| c.set(0));
        for step in 1..=2880 {
            e.advance(497 + step).unwrap();
            let count = replays();
            assert!(
                count <= 1,
                "推进2880分钟至多重放一次，step={step}, count={count}"
            );
            if [1, 14, 80, 2880].contains(&step) {
                let expected = serde_json::to_value(observe::observe(
                    &id,
                    &factory,
                    &telemetry,
                    497,
                    e.now(),
                    &[],
                    false,
                ))
                .unwrap();
                let case = e
                    .state
                    .cases
                    .values()
                    .find(|c| c.batch.starts_with(&format!("{id}:")))
                    .unwrap();
                assert_eq!(
                    case.world["des"], expected,
                    "缓存快照必须与直接重放逐值一致"
                );
                assert_eq!(
                    e.des_factory_view(&id).unwrap(),
                    observe::factory_view(&serde_json::from_value(expected).unwrap())
                );
            }
        }
    }

    #[test]
    fn rt_des_23_recovered_cache_is_shared_read_only_and_not_persisted() {
        let (mut e, id, _, _) = large_engine();
        let before = serde_json::to_value(&e.state).unwrap();
        e.state = serde_json::from_value(before.clone()).unwrap();
        REVIEW_REPLAY_COUNT.with(|c| c.set(0));
        let view = e.des_factory_view(&id).unwrap();
        e.des_forecast(&id).unwrap();
        e.des_whatif(
            &id,
            &WhatIfRequest {
                asset: "CNC-XX".into(),
                quantity: 500,
                due_minute: 960,
            },
        )
        .unwrap();
        assert!(replays() <= 1, "恢复后各读取入口应共用一次重放");
        assert_eq!(
            serde_json::to_value(&e.state).unwrap(),
            before,
            "只读缓存不得持久化或改变状态"
        );
        e.advance(498).unwrap();
        e.des_forecast(&id).unwrap();
        assert!(replays() <= 1, "状态clone后不得丢失缓存");
        let again = e.des_factory_view(&id).unwrap();
        assert_eq!(again["simulation_id"], view["simulation_id"]);
        // 阶段③后重放输入有 5 个持久字段（工厂、场景、尝试序号、干预、备件丢失）；
        // 原 #84 断言"只有 3 个字段"的意图是"遥测不持久化"，这里直接检查。
        let replay =
            serde_json::to_value(&e.state).unwrap()["des_simulations"][&id]["replay"].clone();
        let keys: Vec<&String> = replay.as_object().unwrap().keys().collect();
        assert!(
            keys.iter()
                .all(|k| !k.contains("telemetry") && !k.contains("cache")),
            "不持久化遥测或缓存: {keys:?}"
        );
    }

    /// 阶段③补充：执行干预后缓存按新干预失效并只重算一次；场景结束后的继续推演也不逐分钟重放。
    #[test]
    fn rt_des_22b_interventions_and_post_horizon_reuse_cache() {
        let mut e = Engine::memory().unwrap();
        let id = e
            .create_des_simulation(DesSimulationConfig {
                name: "p0".into(),
                factory: None,
                scenario: ScenarioConfig {
                    seed: 1,
                    pattern: Pattern::Script,
                    start_minute: 497,
                    horizon_minutes: 60,
                    mechanisms: None,
                },
            })
            .unwrap();
        let mut case = None;
        for t in 497..=530 {
            e.advance(t).unwrap();
            case = e
                .state
                .cases
                .values()
                .find(|c| c.stage == "WAITING_HUMAN")
                .map(|c| c.id.clone());
            if case.is_some() {
                break;
            }
        }
        let case = case.expect("P0 case waits for a decision");
        e.select_des_plan(&case, "C").unwrap();
        let rev = e.case(&case).unwrap().plan_revision;
        e.approve(&case, rev, "production", "supervisor").unwrap();
        e.execute(&case, "production").unwrap();
        let t0 = e.now();
        REVIEW_REPLAY_COUNT.with(|c| c.set(0));
        // 推进到场景结束后 300 分钟（维修完成之后）。
        for t in t0 + 1..=497 + 60 + 300 {
            e.advance(t).unwrap();
        }
        assert!(
            replays() <= 2,
            "干预后与场景结束后的推进不应逐分钟重放，实际 {} 次",
            replays()
        );
        let view = e.des_factory_view(&id).unwrap();
        let xx = view["machines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "CNC-XX")
            .unwrap()
            .clone();
        assert_eq!(xx["status"], "running", "维修完成后恢复运行: {xx}");
    }
}
