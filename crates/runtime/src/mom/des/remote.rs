//! 阶段⑤a-2：远程模拟工厂模式（设计文档 §11a，"⑤a-2 设计"）。
//!
//! 仿真在独立的模拟工厂（`an-des-mcp`）中运行，引擎里只登记仿真、不保存重放输入，因此**不会回退到进程内仿真**。
//! 服务层的连接器（`an_server::mom_des_connector`）负责搬运：
//! - 推进前：把模拟工厂已发生的事件与当时的可观测快照交给引擎（`des_ingest_remote`），引擎照常按虚拟时钟投递；
//! - 执行后：引擎把审批后的写操作排进待发清单（`des_pending_writes`），连接器带幂等键发给模拟工厂，回执写回动作；
//! - 审核层的标准答案由连接器从模拟工厂管理入口读取后交给引擎（`des_set_remote_truth`）。
//!
//! 引擎本身保持同步、确定、可重放；所有异步调用都在服务层。
//!
//! 与进程内模式的一致性（RT-DESC-01）：执行后的干预按与进程内相同的规则计算（维修开始时刻、班组排班），
//! 动作的计划完成时刻先按同一参数写入，再以模拟工厂的回执为准；模拟工厂按同样的规则重新推演。

use super::engine_link::{DesSimulation, MAX_DES_SIMULATIONS};
use super::kernel_observe::Observation;
use super::{
    DesSimulationConfig, FactoryConfig, GroundTruth, Intervention, Pattern, ScenarioConfig,
};
use crate::mom::engine::emit;
use crate::mom::{Engine, EventInput, ScheduledEvent, ScheduledPayload};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// 一个待发给模拟工厂的写操作（只在人工审批并 `execute` 之后产生）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteWrite {
    /// 幂等键：`动作键[:设备]`，动作键形如 `案例:方案修订号:production:工具`，重发时不变。
    pub key: String,
    /// 模拟工厂的执行类工具名，例如 `maintenance_order_create`。
    pub tool: String,
    /// 工具参数（已含 `simulation_id`、`idempotency_key`、`approval_ref`）。
    pub args: Value,
}

/// 等待回执的写操作，以及它对应的案例动作。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PendingWrite {
    pub write: RemoteWrite,
    pub case_id: String,
    pub action_key: String,
}

/// 远程仿真在引擎中的记录：工厂结构与场景（不含重放用的尝试序号与机制），当前快照，待发写操作。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RemoteDes {
    pub factory: FactoryConfig,
    pub scenario: ScenarioConfig,
    /// 连接器交来的当前时刻可观测快照（`equipment_status`）。
    #[serde(default)]
    pub observation: Option<Observation>,
    #[serde(default)]
    pub pending: Vec<PendingWrite>,
}

impl Engine {
    /// 下一个仿真编号（与进程内创建的编号规则相同）。连接器把它传给模拟工厂，
    /// 使两边编号一致，并且调用方重启后新仿真不会与数据库里的旧仿真撞号（阶段⑤a-3，DES-28）。
    pub fn des_next_simulation_id(&self) -> String {
        format!("DES-{}", self.state.des_simulations.len() + 1)
    }

    /// 是否为在模拟工厂中运行的远程仿真（未知编号返回 false）。
    pub fn des_is_remote(&self, simulation_id: &str) -> bool {
        self.state
            .des_simulations
            .get(simulation_id)
            .is_some_and(|s| s.remote.is_some())
    }

    /// 登记一个在模拟工厂中运行的仿真（编号由模拟工厂分配）。不保存重放输入。
    pub fn create_remote_des_simulation(
        &mut self,
        simulation_id: &str,
        config: DesSimulationConfig,
    ) -> Result<()> {
        ensure!(
            !config.name.trim().is_empty() && config.name.len() <= 120,
            "name required, max 120 bytes"
        );
        ensure!(
            simulation_id.starts_with("DES-")
                && simulation_id.len() <= 32
                && simulation_id[4..].chars().all(|c| c.is_ascii_digit()),
            "simulation id must look like DES-n"
        );
        ensure!(
            !self.state.des_simulations.contains_key(simulation_id),
            "simulation {simulation_id} already exists"
        );
        ensure!(
            self.state.des_simulations.len() < MAX_DES_SIMULATIONS,
            "DES simulation capacity reached"
        );
        let scenario = config.scenario;
        let sim = DesSimulation {
            id: simulation_id.to_string(),
            name: config.name,
            seed: scenario.seed,
            pattern: scenario.pattern.clone(),
            start_minute: scenario.start_minute,
            horizon_minutes: scenario.horizon_minutes,
            queue: vec![],
            replay: None,
            remote: Some(RemoteDes {
                factory: config.factory.unwrap_or_else(FactoryConfig::line_b_default),
                scenario,
                observation: None,
                pending: vec![],
            }),
        };
        self.update(|s, _| {
            s.des_simulations.insert(simulation_id.to_string(), sim);
            Ok(())
        })
    }

    /// 交给引擎：模拟工厂中已发生的事件（带仿真编号前缀），以及当前时刻的可观测快照
    /// （`equipment_status` 的结果）。之后的 `advance` 按时刻投递这些事件；已收到的事件不重复入队。
    pub fn des_ingest_remote(
        &mut self,
        simulation_id: &str,
        events: Vec<EventInput>,
        observation: Value,
    ) -> Result<()> {
        let sim = self
            .state
            .des_simulations
            .get(simulation_id)
            .context("unknown DES simulation")?;
        let remote = sim.remote.as_ref().context("not a remote DES simulation")?;
        let observation: Observation =
            serde_json::from_value(observation).context("invalid observation from the factory")?;
        ensure!(
            observation.simulation_id == simulation_id,
            "observation belongs to another simulation"
        );
        let prefix = format!("{simulation_id}:");
        ensure!(
            events
                .iter()
                .all(|e| e.id.starts_with(&prefix) && e.batch.starts_with(&prefix)),
            "events must carry the simulation id prefix"
        );
        // 与进程内一致：P0 固定剧本不标记来源仿真（不启用报警模板与协同事项）。
        let coordinated = !matches!(remote.scenario.pattern, Pattern::Script);
        let known: BTreeSet<String> = sim.queue.iter().map(|e| e.id.clone()).collect();
        let fresh: Vec<ScheduledEvent> = events
            .into_iter()
            .filter(|e| !known.contains(&e.id))
            .map(|mut event| {
                if coordinated {
                    event.payload["simulation"] = json!(simulation_id);
                }
                ScheduledEvent {
                    id: event.id.clone(),
                    due_at: event.occurred_at,
                    status: "PENDING".into(),
                    payload: ScheduledPayload::Observation { event },
                    error: None,
                }
            })
            .collect();
        self.update(|s, _| {
            let sim = s
                .des_simulations
                .get_mut(simulation_id)
                .context("unknown DES simulation")?;
            sim.queue.extend(fresh);
            if let Some(r) = sim.remote.as_mut() {
                r.observation = Some(observation);
            }
            Ok(())
        })
    }

    /// 尚未收到回执的写操作。
    pub fn des_pending_writes(&self) -> Vec<RemoteWrite> {
        self.state
            .des_simulations
            .values()
            .filter_map(|s| s.remote.as_ref())
            .flat_map(|r| r.pending.iter().map(|p| p.write.clone()))
            .collect()
    }

    /// 记录模拟工厂对某个写操作的回执，更新对应动作（完成时刻或失败）。
    pub fn des_record_write_receipt(&mut self, key: &str, receipt: Value) -> Result<()> {
        let (sim_id, pending) = self
            .state
            .des_simulations
            .values()
            .find_map(|s| {
                let p = s
                    .remote
                    .as_ref()?
                    .pending
                    .iter()
                    .find(|p| p.write.key == key)?;
                Some((s.id.clone(), p.clone()))
            })
            .with_context(|| format!("no pending write {key}"))?;
        let order_id = receipt["order_id"]
            .as_str()
            .filter(|o| !o.is_empty() && o.len() <= 64)
            .context("receipt has no order id")?
            .to_string();
        let failed = receipt["status"] == "failed";
        let complete = receipt["complete_minute"].as_u64();
        self.update(|s, events| {
            let now = s.now;
            if let Some(r) = s
                .des_simulations
                .get_mut(&sim_id)
                .and_then(|x| x.remote.as_mut())
            {
                r.pending.retain(|p| p.write.key != key);
            }
            let c = s.cases.get_mut(&pending.case_id).context("unknown case")?;
            let a = c
                .actions
                .get_mut(&pending.action_key)
                .context("unknown action")?;
            a.receipt = if a.receipt.starts_with("des-op:") {
                format!("{},{order_id}", a.receipt)
            } else {
                format!("des-op:{order_id}")
            };
            if failed {
                // 调拨失败的原因来自模拟工厂（例如备用刀具已丢失），与进程内的补偿处理一致。
                a.state = "FAILED".into();
                c.stage = "PARTIAL".into();
                c.world["reservations"] = json!({});
                emit(
                    c,
                    now,
                    "CompensationRequired",
                    json!({"released":"reservations","reason": receipt["reason"].as_str().unwrap_or("write failed in simulation"),"irreversible":"accepted maintenance needs operator review","provenance":"simulation"}),
                    events,
                );
            } else if let Some(done) = complete {
                // 维修可能对应多台设备（多张维修单），动作完成时刻取最晚的一张。
                if pending.write.tool == "maintenance_order_create" {
                    a.complete_at = a.complete_at.max(done);
                } else if pending.write.tool == "tooling_transfer_create" {
                    a.complete_at = done;
                }
            }
            Ok(())
        })
    }

    /// 写入从模拟工厂管理入口读到的标准答案，供审核层使用。
    pub fn des_set_remote_truth(&mut self, simulation_id: &str, truth: Value) -> Result<()> {
        ensure!(
            self.state
                .des_simulations
                .get(simulation_id)
                .is_some_and(|s| s.remote.is_some()),
            "not a remote DES simulation"
        );
        let truth: GroundTruth =
            serde_json::from_value(truth).context("invalid ground truth from the factory")?;
        self.update(|s, _| {
            s.des_truth.insert(simulation_id.to_string(), truth);
            Ok(())
        })
    }

    /// 远程模式的执行回调：按与进程内相同的规则算出干预，排成待发写操作，并记录 `DesPlanApplied`。
    pub(super) fn des_after_execute_remote(
        &mut self,
        case_id: &str,
        sim_id: &str,
        plan_id: &str,
        plan: &Value,
        assets: &[String],
        revision: u64,
    ) -> Result<()> {
        let r = self
            .state
            .des_simulations
            .get(sim_id)
            .and_then(|s| s.remote.as_ref())
            .context("not a remote DES simulation")?
            .factory
            .resources
            .clone();
        let now = self.now();
        let c = self.case(case_id)?;
        let action_key = |tool: &str| {
            c.actions
                .iter()
                .find(|(_, a)| a.tool == tool)
                .map(|(k, _)| k.clone())
        };
        let approval_ref = format!("{case_id}:rev{revision}:production");
        let write = |tool: &str, key: String, mut args: Value| {
            args["simulation_id"] = json!(sim_id);
            args["idempotency_key"] = json!(key);
            args["approval_ref"] = json!(approval_ref);
            RemoteWrite {
                key,
                tool: tool.into(),
                args,
            }
        };
        let mut interventions = vec![];
        let mut pending = vec![];
        // 维修：与进程内相同的开始时刻与班组排班。
        let maintenance_start = match plan_id {
            "B" => Some(now + r.finish_batch_minutes),
            "C" => Some(now),
            _ => None,
        };
        let mut maintenance_end = None;
        if let (Some(start), Some(key)) = (maintenance_start, action_key("eam.maintenance")) {
            // 与进程内同一排班函数；本批内依次占班。模拟工厂端还会按已接受维修再排一次。
            let mut crews = an_des::crew_free_after(r.maintenance_crews.max(1) as usize, &[]);
            let mut last_end = start;
            for machine in assets {
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
                pending.push(PendingWrite {
                    write: write(
                        "maintenance_order_create",
                        format!("{key}:{machine}"),
                        json!({"machine": machine, "start_minute": begin}),
                    ),
                    case_id: case_id.into(),
                    action_key: key.clone(),
                });
            }
            maintenance_end = Some(last_end);
        }
        // 刀具调拨：是否成功由模拟工厂决定（回执），这里按计划时刻先写入。
        let mut transfer_arrive = None;
        if let Some(key) = action_key("mro.transfer") {
            for machine in plan["prerequisites"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str()?.strip_prefix("tool_transfer:"))
            {
                let arrive = now + r.tool_transfer_minutes;
                interventions.push(Intervention::ToolTransfer {
                    machine: machine.to_string(),
                    arrive_minute: arrive,
                });
                transfer_arrive = Some(arrive);
                pending.push(PendingWrite {
                    write: write(
                        "tooling_transfer_create",
                        format!("{key}:{machine}"),
                        json!({"machine": machine}),
                    ),
                    case_id: case_id.into(),
                    action_key: key.clone(),
                });
            }
        }
        if let Some(key) = action_key("mes.reschedule") {
            pending.push(PendingWrite {
                write: write(
                    "production_schedule_apply",
                    key.clone(),
                    json!({"plan_id": plan_id}),
                ),
                case_id: case_id.into(),
                action_key: key,
            });
        }
        self.update(|s, events| {
            if let Some(remote) = s
                .des_simulations
                .get_mut(sim_id)
                .and_then(|x| x.remote.as_mut())
            {
                remote.pending.extend(pending);
            }
            let c = s.cases.get_mut(case_id).context("unknown case")?;
            for a in c.actions.values_mut() {
                match a.tool.as_str() {
                    "eam.maintenance" => {
                        if let Some(end) = maintenance_end {
                            a.complete_at = end;
                        }
                    }
                    "mro.transfer" => {
                        if let Some(at) = transfer_arrive {
                            a.complete_at = at;
                        }
                    }
                    _ => {}
                }
            }
            c.world["des_applied"] = json!(revision);
            emit(
                c,
                s.now,
                "DesPlanApplied",
                json!({"plan": plan_id, "interventions": interventions, "provenance": "simulation"}),
                events,
            );
            Ok(())
        })
    }
}
