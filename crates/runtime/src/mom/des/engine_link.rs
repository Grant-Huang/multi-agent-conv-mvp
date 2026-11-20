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
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

/// 场景结束后继续推演时，每次多算的分钟数，避免逐分钟重放（R-20261117-M1）。
const EXTENSION_CHUNK_MINUTES: u64 = 720;

/// 同一引擎中 DES 仿真数量上限（与脚本仿真一致）。
pub(super) const MAX_DES_SIMULATIONS: usize = 50;

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
    /// 阶段⑤a-2：在模拟工厂中运行的仿真（远程模式）。与 `replay` 互斥：远程仿真没有重放输入，不会回退到进程内仿真。
    #[serde(default)]
    pub(crate) remote: Option<super::remote::RemoteDes>,
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
///
/// `coordinated` 为真时在载荷中标记来源仿真，启用阶段③b 的报警模板与产线协同事项；
/// P0 固定剧本（`Pattern::Script`）不标记，保持原 S1 行为不变（设计文档 §6.8）。
fn prefixed_queue(
    id: &str,
    events: Vec<an_des::FactoryEvent>,
    coordinated: bool,
) -> Vec<ScheduledEvent> {
    events
        .into_iter()
        .map(|event| {
            let mut event = DesEvent::from(event);
            event.id = format!("{id}:{}", event.id);
            event.batch = format!("{id}:{}", event.batch);
            if coordinated {
                event.payload["simulation"] = json!(id);
            }
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
        let queue = prefixed_queue(
            &id,
            run.events,
            !matches!(scenario.pattern, super::Pattern::Script),
        );
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
            remote: None,
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

    /// 仿真的工厂结构：进程内仿真取重放输入中的配置，远程仿真取登记时的配置。
    pub(in crate::mom) fn des_factory_config(&self, simulation_id: &str) -> Option<FactoryConfig> {
        let sim = self.state.des_simulations.get(simulation_id)?;
        sim.replay
            .as_ref()
            .map(|r| r.factory.clone())
            .or_else(|| sim.remote.as_ref().map(|r| r.factory.clone()))
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
        // 远程仿真（阶段⑤a-3）：用连接器最近交来的快照计算，与写入案例 `world.des` 的是同一份。
        if let Some(remote) = self
            .state
            .des_simulations
            .get(simulation_id)
            .and_then(|s| s.remote.as_ref())
        {
            let o = remote.observation.as_ref().context(
                "no observation from the simulated factory yet; advance the clock first",
            )?;
            return observe::whatif(o, request);
        }
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
            if sim.replay.is_none() && sim.remote.is_none() {
                continue;
            }
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
            let snapshot = match (&sim.replay, &sim.remote) {
                (Some(r), _) => serde_json::to_value(r.observe(&sim.id, now))?,
                // 远程模式：连接器在推进前交来的快照，只在它的观测时刻与当前处理的分钟一致时写入。
                // `advance` 会重新处理当前分钟，那时快照已是下一分钟的，跳过即保留上一分钟的快照，
                // 与进程内模式一致；还没有快照时也不写入（不编造观测）。
                (None, Some(remote)) => match &remote.observation {
                    Some(o) if o.observed_at == now => serde_json::to_value(o)?,
                    _ => continue,
                },
                (None, None) => continue,
            };
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
        // 协同事项作用于报警设备集合；单设备案例作用于本设备。
        let assets: Vec<String> = proposal["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .filter(|a: &Vec<String>| !a.is_empty())
            .unwrap_or_else(|| vec![c.asset.clone()]);
        let revision = c.plan_revision;
        if self
            .state
            .des_simulations
            .get(&sim_id)
            .is_some_and(|s| s.remote.is_some())
        {
            return self
                .des_after_execute_remote(case_id, &sim_id, &plan_id, &plan, &assets, revision);
        }
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
        // 维修班组依次排班（与模拟工厂共用 an_des::schedule；累计此前已接受的维修）。
        let maintenance_end = maintenance_start.map(|start| {
            let prior: Vec<(u64, u64)> = replay
                .interventions
                .iter()
                .filter_map(|iv| match iv {
                    Intervention::Maintenance {
                        start_minute,
                        end_minute,
                        ..
                    } => Some((*start_minute, *end_minute)),
                    _ => None,
                })
                .collect();
            let mut crews =
                an_des::crew_free_after(r.maintenance_crews.max(1) as usize, &prior);
            let mut last_end = start;
            for machine in &assets {
                // 该设备维修尚未结束：不再重复安排。
                let already = replay.interventions.iter().any(|iv| match iv {
                    Intervention::Maintenance {
                        machine: m,
                        end_minute,
                        ..
                    } => m == machine && *end_minute > start,
                    _ => false,
                });
                if already {
                    continue;
                }
                let Ok((begin, end)) =
                    an_des::schedule_next_maintenance(start, r.maintenance_minutes, &mut crews)
                else {
                    continue;
                };
                last_end = last_end.max(end);
                interventions.push(Intervention::Maintenance {
                    machine: machine.clone(),
                    start_minute: begin,
                    end_minute: end,
                });
            }
            last_end
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
        let future: Vec<ScheduledEvent> = prefixed_queue(
            &sim_id,
            run.events,
            !matches!(next.scenario.pattern, super::Pattern::Script),
        )
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
        let Some(factory) = self.des_factory_config(sim_id) else {
            return Ok(());
        };
        let line_of = |asset: &str| {
            factory
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

    /// 产线协同事项（阶段③b，设计文档 §6.8）。某 DES 案例收到观测后：
    /// - 该产线已有未执行的活动事项 → 加入（成员改为只做诊断）；
    /// - 否则，若同产线窗口内有足够多的 DES 案例且至少一个有报警 → 建立事项。
    ///
    /// 已执行的事项不再接收新成员；已执行动作的案例不加入事项。
    fn coordinate_incident(&mut self, sim_id: &str, case_id: &str) -> Result<()> {
        use super::coordination::{has_alarm, is_incident, is_member};
        let Some(factory) = self.des_factory_config(sim_id) else {
            return Ok(());
        };
        let coord = factory.coordination.clone();
        let line_of = |asset: &str| {
            factory
                .machines
                .iter()
                .find(|m| m.id == asset)
                .map(|m| m.line.clone())
        };
        let open = |c: &crate::mom::Case| {
            !c.cancelled && !matches!(c.stage.as_str(), "CLOSED" | "CANCELLED")
        };
        let c = self.case(case_id)?;
        // 已执行动作的案例不再加入事项；已请人决策但未执行的可以加入，其决策请求在加入时撤回。
        if is_incident(c) || !open(c) || !c.actions.is_empty() {
            return Ok(());
        }
        let Some(line) = line_of(&c.asset) else {
            return Ok(());
        };
        let now = self.now();
        let sim_prefix = format!("{sim_id}:");
        let incident_prefix = format!("{sim_id}:INCIDENT:{line}:");
        let active = self
            .state
            .cases
            .values()
            .find(|x| x.batch.starts_with(&incident_prefix) && open(x))
            .map(|x| (x.id.clone(), x.actions.is_empty()));
        let members: Vec<String> = match active {
            Some((_, false)) => return Ok(()),
            Some((ref inc, true)) => {
                let current = &self.case(inc)?.world["des_incident"]["members"];
                let already = current
                    .as_array()
                    .is_some_and(|m| m.iter().any(|v| v == case_id));
                if already && !has_alarm(c) {
                    return Ok(());
                }
                vec![case_id.to_string()]
            }
            None => {
                let since = now.saturating_sub(coord.incident_window_minutes);
                let candidates: Vec<&crate::mom::Case> = self
                    .state
                    .cases
                    .values()
                    .filter(|x| {
                        x.batch.starts_with(&sim_prefix)
                            && !is_incident(x)
                            && !is_member(x)
                            && open(x)
                            && x.actions.is_empty()
                            && line_of(&x.asset).as_deref() == Some(line.as_str())
                            && x.observations.iter().any(|o| o.event.occurred_at >= since)
                    })
                    .collect();
                if candidates.len() < coord.min_cases.max(2)
                    || !candidates.iter().any(|x| has_alarm(x))
                {
                    return Ok(());
                }
                candidates.iter().map(|x| x.id.clone()).collect()
            }
        };
        let existing = active.map(|(id, _)| id);
        self.apply_incident(sim_id, &line, existing, members)
    }

    /// 建立或更新协同事项，并把新成员改为只做诊断（同一事务内完成）。
    fn apply_incident(
        &mut self,
        sim_id: &str,
        line: &str,
        existing: Option<String>,
        new_members: Vec<String>,
    ) -> Result<()> {
        use super::coordination::{
            des_template, has_alarm, incident_nodes, TPL_DIAGNOSIS, TPL_LINE_INCIDENT,
        };
        self.update(|s, events| {
            let now = s.now;
            // 新建事项。
            let incident_id = match existing {
                Some(id) => id,
                None => {
                    let id = format!("CASE-s1-{}", s.cases.len() + 1);
                    let seq = s
                        .cases
                        .values()
                        .filter(|c| c.batch.starts_with(&format!("{sim_id}:INCIDENT:{line}:")))
                        .count()
                        + 1;
                    let mut world = crate::mom::fixture_world("s1");
                    world["des_incident"] = json!({"line": line, "members": [], "assets": [], "orders": []});
                    let policy = crate::mom::fixture_policy("s1");
                    let incident = crate::mom::Case {
                        id: id.clone(),
                        scenario: "s1".into(),
                        batch: format!("{sim_id}:INCIDENT:{line}:{seq}"),
                        customer: "LINE".into(),
                        asset: line.into(),
                        updated_at: now,
                        revision: 0,
                        plan_revision: 1,
                        stage: "ASSESS".into(),
                        template: TPL_LINE_INCIDENT.into(),
                        constraints: vec![
                            "Never bypass safety interlock or quality release".into(),
                            "Approval must match current plan and resource conditions".into(),
                            "Simulated evidence is not real industrial validation".into(),
                        ],
                        observations: vec![],
                        nodes: incident_nodes(false),
                        evidence: BTreeMap::new(),
                        proposal: None,
                        initial_report: None,
                        eight_d: None,
                        approvals: BTreeMap::new(),
                        actions: BTreeMap::new(),
                        children: vec![],
                        outbox: vec![],
                        world,
                        resource_version: 1,
                        resource_observed_at: now,
                        budget_remaining: 40,
                        tool_calls: 0,
                        model_calls: 0,
                        replans: 0,
                        deadlines: BTreeMap::new(),
                        alerts: vec![],
                        breaches: vec![],
                        mock_ids: policy.mock_ids,
                        cancelled: false,
                    };
                    s.cases.insert(id.clone(), incident);
                    let c = s.cases.get_mut(&id).context("incident missing")?;
                    emit(
                        c,
                        now,
                        "LineIncidentOpened",
                        json!({"line": line, "simulation": sim_id, "provenance": "simulation"}),
                        events,
                    );
                    id
                }
            };
            // 成员：标记归属；已有排产计划且未执行的改为诊断模板。
            // 已请人决策（等待决策或受阻）的，撤回决策请求：方案修订号加 1，旧批准作废，
            // 决策此后只在事项上做（设计文档 §6.8，2026-11-17 用户确认方案 A）。
            for m in &new_members {
                let c = s.cases.get_mut(m).context("member missing")?;
                let joined = !c.world.get("des_member_of").is_some_and(Value::is_string);
                c.world["des_member_of"] = json!(incident_id);
                if c.actions.is_empty() && !c.template.is_empty() && c.template != TPL_DIAGNOSIS {
                    if let Some((template, nodes)) = des_template(c) {
                        let pending = matches!(c.stage.as_str(), "WAITING_HUMAN" | "BLOCKED");
                        c.template = template;
                        c.nodes = nodes;
                        c.proposal = None;
                        c.approvals.clear();
                        c.plan_revision += 1;
                        c.stage = "ASSESS".into();
                        emit(
                            c,
                            now,
                            "PlanInvalidated",
                            json!({"reason": "joined line incident", "incident": incident_id}),
                            events,
                        );
                        if pending {
                            emit(
                                c,
                                now,
                                "DecisionWithdrawn",
                                json!({"reason": "joined line incident", "incident": incident_id, "plan_revision": c.plan_revision}),
                                events,
                            );
                        }
                    }
                }
                if joined {
                    emit(c, now, "JoinedLineIncident", json!({"incident": incident_id}), events);
                }
            }
            // 事项：成员、报警设备集合与插单；有变化且未执行时重新规划。
            let prior = s
                .cases
                .get(&incident_id)
                .map(|c| c.world["des_incident"].clone())
                .context("incident missing")?;
            let mut members: BTreeSet<String> = prior["members"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            members.extend(new_members.iter().cloned());
            let mut assets = BTreeSet::new();
            let mut orders = vec![];
            for m in &members {
                if let Some(mc) = s.cases.get(m) {
                    if has_alarm(mc) {
                        assets.insert(mc.asset.clone());
                    }
                    for o in mc.observations.iter().filter(|o| o.event.kind == "order") {
                        orders.push(json!({
                            "order": o.event.payload["order"],
                            "quantity": o.event.payload["quantity"],
                            "due_minute": o.event.payload["due_minute"],
                        }));
                    }
                }
            }
            let next = json!({"line": line, "members": members, "assets": assets, "orders": orders});
            let inc = s.cases.get_mut(&incident_id).context("incident missing")?;
            if inc.world["des_incident"] != next && inc.actions.is_empty() {
                let had_plan = inc.nodes.values().any(|n| n.status != "READY");
                inc.world["des_incident"] = next;
                inc.nodes = incident_nodes(!assets.is_empty());
                inc.proposal = None;
                inc.approvals.clear();
                inc.stage = "ASSESS".into();
                if had_plan {
                    inc.plan_revision += 1;
                    emit(
                        inc,
                        now,
                        "PlanInvalidated",
                        json!({"reason": "incident membership changed"}),
                        events,
                    );
                }
                emit(
                    inc,
                    now,
                    "PlanAccepted",
                    json!({"template": TPL_LINE_INCIDENT, "plan_revision": inc.plan_revision, "members": members, "assets": assets}),
                    events,
                );
            }
            Ok(())
        })
    }

    /// `close` 成功后由引擎调用：关闭协同事项时一并关闭成员案例（阶段③b）；安全联锁中、BLOCKED
    /// 或有未核验动作的成员不关闭，在事项上记 `CaseCloseSkipped`（R-20261118-H2）。其他案例不做任何事。
    pub(in crate::mom) fn des_after_close(&mut self, case_id: &str) -> Result<()> {
        let c = self.case(case_id)?;
        if !super::coordination::is_incident(c) {
            return Ok(());
        }
        let members: Vec<String> = c.world["des_incident"]["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        self.update(|s, events| {
            let now = s.now;
            // 不能随事项关闭的成员：与 `Engine::close` 的安全前提一致（R-20261118-H2，#96 已修复）。
            // 跳过它们，在成员上记 `CaseCloseSkipped`（#96），并在事项上逐个汇总（决策 DES-26）；
            // 其他成员照常关闭；被跳过的成员留在看板上由人单独处理。
            let mut skipped: Vec<(String, &'static str)> = vec![];
            for m in &members {
                let Some(c) = s.cases.get_mut(m) else {
                    continue;
                };
                if matches!(c.stage.as_str(), "CLOSED" | "CANCELLED") {
                    continue;
                }
                let skip_reason = if c.world["safety_trip"] == true {
                    Some("safety_trip")
                } else if c.stage == "BLOCKED" {
                    Some("blocked")
                } else if c.actions.values().any(|action| action.state != "VERIFIED") {
                    Some("verification_incomplete")
                } else {
                    None
                };
                if let Some(reason) = skip_reason {
                    emit(
                        c,
                        now,
                        "CaseCloseSkipped",
                        json!({"incident": case_id, "reason": reason}),
                        events,
                    );
                    // 事项上也逐个记录被跳过的成员（DES-26：便于在事项上看到哪些成员还开着、为什么）。
                    skipped.push((m.clone(), reason));
                    continue;
                }
                c.stage = "CLOSED".into();
                for ch in &mut c.children {
                    if ch.status == "RUNNING" {
                        ch.status = "CANCELLED".into();
                    }
                }
                c.world["reservations"] = json!({});
                emit(
                    c,
                    now,
                    "CaseClosed",
                    json!({"scope": "closed with line incident", "incident": case_id}),
                    events,
                );
            }
            if let Some(incident) = s.cases.get_mut(case_id) {
                for (member, reason) in skipped {
                    emit(
                        incident,
                        now,
                        "CaseCloseSkipped",
                        json!({"member": member, "reason": reason}),
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
                ScheduledPayload::Observation { event } => self.ingest(event).and_then(|case_id| {
                    self.link_related_cases(&sim_id, &case_id)?;
                    self.coordinate_incident(&sim_id, &case_id)
                }),
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
