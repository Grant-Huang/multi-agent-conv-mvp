//! Configurable deterministic simulation. Persistent queue, explicit shared clock.
use super::{engine::emit, Engine, EventInput, Snapshot};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimulationConfig {
    pub name: String,
    pub scenario: String,
    pub seed: u64,
    pub world: BTreeMap<String, Value>,
    pub event_count: u32,
    pub event_interval_minutes: u64,
    pub disturbances: Vec<Disturbance>,
}
impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            name: "新仿真".into(),
            scenario: "s1".into(),
            seed: 1,
            world: BTreeMap::new(),
            event_count: 4,
            event_interval_minutes: 5,
            disturbances: vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disturbance {
    pub after_minutes: u64,
    pub key: String,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledPayload {
    Observation { event: EventInput },
    WorldChange { key: String, value: Value },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub id: String,
    pub due_at: u64,
    pub status: String,
    pub payload: ScheduledPayload,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Simulation {
    pub case_id: String,
    pub config: SimulationConfig,
    pub started_at: u64,
    pub queue: Vec<ScheduledEvent>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Playback {
    pub playing: bool,
    pub remaining_minutes: u64,
}

pub(super) fn validate_change(key: &str, value: &Value) -> Result<()> {
    match key {
        "quantity" => ensure!(
            value.as_u64().is_some_and(|v| v > 0 && v <= 1_000_000),
            "quantity must be integer1..1000000"
        ),
        "capacity_xy" | "capacity_xz" => ensure!(
            value.as_u64().is_some_and(|v| v <= 1_000_000),
            "capacity must be integer0..1000000"
        ),
        "cpk" => ensure!(
            value.as_f64().is_some_and(|v| (0.0..=5.0).contains(&v)),
            "cpk must be 0..5"
        ),
        "history" => ensure!(
            value.as_str().is_some_and(|s| s.len() <= 16000),
            "history must be bounded text"
        ),
        // E1-C：服务层从 RelOS 取回的关系上下文，作为一条观测写入（大小受限，status 取值受限）
        "relos" => {
            let status = value.get("status").and_then(Value::as_str);
            ensure!(
                value.is_object()
                    && matches!(status, Some("ok" | "unavailable" | "timeout" | "empty")),
                "relos must be an object with status ok|unavailable|timeout|empty"
            );
            ensure!(
                value.to_string().len() <= 65_536,
                "relos context must be at most 64KB"
            );
        }
        "resources_available"
        | "certified"
        | "tool_available"
        | "resource_conflict"
        | "maintenance_timeout"
        | "quality_timeout"
        | "dispatch_failure"
        | "lost_receipt"
        | "child_timeout"
        | "missing_genealogy"
        | "conflicting_records"
        | "root_cause_verified"
        | "effectiveness_verified" => ensure!(value.is_boolean(), "fault flags must be boolean"),
        _ => anyhow::bail!("unsupported world parameter {key}"),
    }
    Ok(())
}
fn random(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut x = *seed;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
impl Engine {
    pub fn create_simulation(&mut self, config: SimulationConfig) -> Result<String> {
        ensure!(
            matches!(config.scenario.as_str(), "s1" | "s2"),
            "unknown scenario"
        );
        ensure!(
            !config.name.trim().is_empty() && config.name.len() <= 120,
            "name required, max 120 bytes"
        );
        ensure!(
            config.event_count <= 32 && (1..=120).contains(&config.event_interval_minutes),
            "event count max32; interval1..120 minutes"
        );
        ensure!(
            config.disturbances.len() <= 32 && config.world.len() <= 24,
            "configuration too large"
        );
        ensure!(
            self.state.simulations.len() < 50,
            "simulation capacity reached"
        );
        for (key, value) in &config.world {
            validate_change(key, value)?;
        }
        for d in &config.disturbances {
            ensure!(
                (1..=2880).contains(&d.after_minutes),
                "disturbance must be within 48h"
            );
            validate_change(&d.key, &d.value)?;
        }
        let start = self
            .now()
            .max(if config.scenario == "s1" { 497 } else { 540 });
        let batch = format!("SIM-{}", self.state.cases.len() + 1);
        let mut first = if config.scenario == "s1" {
            EventInput::s1("order", start)
        } else {
            EventInput::complaint(&batch, start)
        };
        first.id = format!("{batch}-initial");
        first.batch = batch.clone();
        if let Some(quantity) = config.world.get("quantity") {
            first.payload["quantity"] = quantity.clone();
        }
        let id = self.ingest(first)?;
        self.update(|s,events|{
            let c=s.cases.get_mut(&id).context("case missing")?;
            for (key,value) in &config.world {c.world[key]=value.clone();}
            c.resource_version+=1;
            let mut queue=vec![];
            let mut make_observation=|kind:&str,at:u64,cpk:Option<f64>| {
                let mut event=EventInput::s1(kind,at);event.scenario=config.scenario.clone();event.id=format!("{batch}-event-{}",queue.len());event.batch=batch.clone();
                if let Some(value)=cpk{event.payload["cpk"]=json!(value);}
                queue.push(ScheduledEvent{id:event.id.clone(),due_at:at,status:"PENDING".into(),payload:ScheduledPayload::Observation{event},error:None});
            };
            if config.scenario=="s1"{make_observation("vibration",start+6,None);make_observation("quality",start+14,Some(config.world.get("cpk").and_then(Value::as_f64).unwrap_or(1.1)));}
            let mut seed=config.seed;
            for i in 0..config.event_count {
                let cpk=(85+random(&mut seed)%51) as f64/100.0;
                make_observation("quality",start+14+(u64::from(i)+1)*config.event_interval_minutes,Some(cpk));
            }
            for (index,d) in config.disturbances.iter().enumerate(){queue.push(ScheduledEvent{id:format!("{batch}-fault-{index}"),due_at:start+d.after_minutes,status:"PENDING".into(),payload:ScheduledPayload::WorldChange{key:d.key.clone(),value:d.value.clone()},error:None});}
            queue.sort_by(|a,b|(a.due_at,&a.id).cmp(&(b.due_at,&b.id)));
            emit(c,s.now,"SimulationConfigured",json!({"seed":config.seed,"name":config.name,"queued":queue.len(),"provenance":"mock"}),events);
            s.simulations.insert(id.clone(),Simulation{case_id:id.clone(),config,started_at:start,queue});Ok(())
        })?;
        Ok(id)
    }
    pub(super) fn deliver_scheduled(&mut self, now: u64) -> Result<()> {
        let due: Vec<_> = self
            .state
            .simulations
            .values()
            .flat_map(|sim| {
                sim.queue
                    .iter()
                    .filter(|e| e.status == "PENDING" && e.due_at <= now)
                    .map(|e| (sim.case_id.clone(), e.clone()))
            })
            .collect();
        for (id, event) in due {
            if self.case(&id)?.cancelled || self.case(&id)?.stage == "CLOSED" {
                self.mark_delivery(&id, &event.id, "CANCELLED", None)?;
                continue;
            }
            let result = match event.payload {
                ScheduledPayload::Observation { event } => self.ingest(event).map(|_| ()),
                ScheduledPayload::WorldChange { key, value } => self.change_world(&id, &key, value),
            };
            match result {
                Ok(()) => self.mark_delivery(&id, &event.id, "APPLIED", None)?,
                Err(e) => self.mark_delivery(&id, &event.id, "FAILED", Some(e.to_string()))?,
            }
        }
        Ok(())
    }
    fn mark_delivery(
        &mut self,
        id: &str,
        event_id: &str,
        status: &str,
        error: Option<String>,
    ) -> Result<()> {
        self.update(|s, events| {
            let sim = s.simulations.get_mut(id).context("simulation missing")?;
            let item = sim
                .queue
                .iter_mut()
                .find(|e| e.id == event_id)
                .context("scheduled event missing")?;
            item.status = status.into();
            item.error = error.clone();
            let c = s.cases.get_mut(id).context("case missing")?;
            emit(
                c,
                s.now,
                "SimulationEventDelivered",
                json!({"event_id":event_id,"status":status,"error":error}),
                events,
            );
            Ok(())
        })
    }
    pub fn simulations_view(&self) -> Value {
        json!({"now":self.now(),"playback":self.state.playback,"simulation_rate":"1 wall second = 1 virtual minute","simulations":self.state.simulations.values().map(|sim|json!({"case_id":sim.case_id,"config":sim.config,"started_at":sim.started_at,"pending":sim.queue.iter().filter(|e|e.status=="PENDING").count(),"queue":sim.queue})).collect::<Vec<_>>()})
    }
    pub fn set_playback(&mut self, playing: bool, minutes: u64) -> Result<()> {
        ensure!(
            !playing || (1..=360).contains(&minutes),
            "autoplay horizon must be1..360 minutes"
        );
        self.update(|s, _| {
            s.playback = Playback {
                playing,
                remaining_minutes: if playing { minutes } else { 0 },
            };
            Ok(())
        })
    }
    pub fn playback_tick(&mut self) -> Result<()> {
        if !self.state.playback.playing {
            return Ok(());
        }
        self.advance(self.now() + 1)?;
        self.run_children()?;
        self.update(|s, _| {
            s.playback.remaining_minutes = s.playback.remaining_minutes.saturating_sub(1);
            if s.playback.remaining_minutes == 0 {
                s.playback.playing = false;
            }
            Ok(())
        })
    }
}

pub fn gap_register() -> Value {
    let text = include_str!("../../../../docs/mom/MOCK_REGISTER.md");
    let items:Vec<_>=text.lines().filter_map(|line|{
        let parts:Vec<_>=line.split('|').map(str::trim).collect();
        if parts.len()<7||!parts[2].starts_with('M'){return None;}
        let (id,title)=parts[2].split_once(' ')?;
        if id.len()!=3||!id[1..].bytes().all(|b|b.is_ascii_digit()){return None;}
        Some(json!({"id":id,"title":title,"implementation":parts[3],"acceptance":parts[4],"status":parts[5],"verified":parts[1]=="[x]"}))
    }).collect();
    json!({"items":items,"source":"docs/mom/MOCK_REGISTER.md","provenance":"register, not automatically inferred from simulation results"})
}

pub(super) fn pause_on_recovery(s: &mut Snapshot) {
    s.playback.playing = false;
    s.playback.remaining_minutes = 0;
}
