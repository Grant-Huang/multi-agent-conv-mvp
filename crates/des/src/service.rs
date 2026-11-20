//! 模拟工厂的工具目录与调用入口（阶段⑤a-1，设计文档 §11a）。
//!
//! 工具名只用小写字母、数字和下划线，长度不超过 64：MCP 规范允许点号，但 Anthropic 与 OpenAI 的
//! 工具名规则是 `^[a-zA-Z0-9_-]{1,64}$`，用点号会让挂载到这些模型的系统出错（设计文档 §11a 修订）。
//!
//! 行为约定：
//! - 同步、确定，不读墙钟：时钟只由 `simulation_advance` 推进；相同的调用序列得到逐字节相同的结果。
//! - 观测工具只读；被拒绝的调用不改变任何状态（先校验，再修改）。
//! - 写操作（执行类）必须带幂等键与审批引用。模拟工厂无法核验审批本身——审批由接入方（AgentNexus）强制，
//!   这里只要求携带并记录；同键同参数返回原回执、只生效一次，同键不同参数拒绝。
//! - 维修与刀具调拨作为干预改变仿真世界：已发生的事件不变，当前时刻之后的事件与标准答案按新干预重新推演，
//!   与 MOM 进程内仿真的做法相同（`engine_link::des_after_execute`）。
//! - 标准答案只经管理工具提供，需要服务配置的管理口令；观测输出不含标准答案与隐状态。

use crate::observe::{self, Observation};
use crate::scenario::{rerun, run_with_attempt};
use crate::{
    FactoryConfig, FactoryEvent, GroundTruth, Intervention, ScenarioConfig, SimulationConfig,
    Telemetry, CPK_THRESHOLD,
};
use anyhow::{bail, ensure, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// 一个服务最多持有的仿真数。
const MAX_SIMULATIONS: usize = 50;
/// 一次推进的最大分钟数（与 MOM 引擎一致）。
const MAX_ADVANCE_MINUTES: u64 = 100_000;
/// 每次拉取事件的上限。
const MAX_POLL_LIMIT: u64 = 1000;
/// 场景结束后需要延伸遥测时，一次多推演的分钟数（与 MOM 进程内仿真一致）。
const EXTENSION_CHUNK_MINUTES: u64 = 720;
/// 维修单最晚可排到当前时刻之后多少分钟。
const MAX_SCHEDULE_AHEAD_MINUTES: u64 = 7 * 24 * 60;
/// 每个仿真最多的工单数（含失败回执），防止无界增长。
const MAX_ORDERS: usize = 10_000;
/// 文本字段长度上限（字节）。
const MAX_TEXT: usize = 200;

/// 工具类别：决定谁可以调用（设计文档 §11a 工具表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// 只读观测：agent、桌面、连接器。
    Observe,
    /// 外部写（幂等键、审批引用、回执）：只由 AgentNexus 审批后的执行流程调用。
    Execute,
    /// 仿真控制：测试调度器、实验脚本。
    Control,
    /// 管理（敏感）：只供审核层，需要管理口令，不进任何 agent 的允许清单。
    Admin,
}

/// 一个工具的说明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub category: ToolCategory,
    pub description: String,
    /// JSON Schema（object，拒绝未知字段）。
    pub input_schema: Value,
}

/// 服务配置。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// 管理口令；为 None 时管理工具一律拒绝。
    pub admin_token: Option<String>,
}

// ───────────────────────── 工具目录 ─────────────────────────

fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc, "minLength": 1, "maxLength": MAX_TEXT})
}

fn integer(desc: &str) -> Value {
    json!({"type": "integer", "description": desc, "minimum": 0})
}

/// object schema：`fields` 为（字段名，schema，是否必填）。
fn schema(fields: &[(&str, Value, bool)]) -> Value {
    let properties: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(k, v, _)| (k.to_string(), v.clone()))
        .collect();
    let required: Vec<&str> = fields.iter().filter(|f| f.2).map(|f| f.0).collect();
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn sim_field() -> (&'static str, Value, bool) {
    ("simulation_id", string("Simulation id, e.g. DES-1"), true)
}

fn write_fields() -> [(&'static str, Value, bool); 2] {
    [
        (
            "idempotency_key",
            string("Caller-chosen key; a retry with the same key and arguments returns the original receipt"),
            true,
        ),
        (
            "approval_ref",
            string("Reference to the human approval that authorised this write (recorded on the receipt)"),
            true,
        ),
    ]
}

/// `simulation_create.factory` 的 schema（与 `FactoryConfig` 的反序列化规则一致；null 使用默认 Line-B）。
fn factory_schema() -> Value {
    let duration = |desc: &str| {
        json!({"type": "integer", "minimum": 0, "maximum": crate::MAX_DURATION_MINUTES, "description": desc})
    };
    let count = |desc: &str| json!({"type": "integer", "minimum": 0, "description": desc});
    json!({
        "type": ["object", "null"],
        "description": "Factory configuration; null uses the default Line-B (CNC-XX, CNC-XY, CNC-XZ)",
        "properties": {
            "machines": {
                "type": "array",
                "minItems": 1,
                "maxItems": crate::MAX_MACHINES,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": string("Machine id, e.g. CNC-XX"),
                        "line": string("Production line, e.g. Line-B"),
                        "cycle_seconds": {"type": "integer", "minimum": 1, "description": "Cycle time per part (seconds)"}
                    },
                    "required": ["id", "line", "cycle_seconds"],
                    "additionalProperties": false
                }
            },
            "resources": {
                "type": "object",
                "description": "Resource and process assumptions; omitted fields take defaults",
                "properties": {
                    "uncertified": {"type": "array", "items": {"type": "string"}},
                    "missing_tool": {"type": "array", "items": {"type": "string"}},
                    "tool_spares": count("Spare tools in the store"),
                    "tool_transfer_minutes": duration("Tool transfer lead time (minutes)"),
                    "maintenance_minutes": duration("Repair time (minutes)"),
                    "maintenance_crews": count("Maintenance crews"),
                    "finish_batch_minutes": duration("Time to finish the current batch (minutes)"),
                    "inspection_minutes": duration("Release inspection time (minutes)"),
                    "urgent_capacity_share": {"type": "number", "minimum": 0, "maximum": 1}
                },
                "additionalProperties": false
            },
            "coordination": {
                "type": "object",
                "description": "Caller-side coordination parameters carried with the factory; the simulation ignores them",
                "properties": {
                    "incident_window_minutes": count("Window (minutes)"),
                    "min_cases": count("Minimum cases")
                },
                "additionalProperties": false
            }
        },
        "required": ["machines"],
        "additionalProperties": false
    })
}

/// `simulation_create.scenario` 的 schema（与 `ScenarioConfig` 的反序列化规则一致）。
fn scenario_schema() -> Value {
    let simple = |kind: &str| json!({"type": "object", "properties": {"kind": {"const": kind}}, "required": ["kind"], "additionalProperties": false});
    json!({
        "type": "object",
        "properties": {
            "seed": {"type": "integer", "minimum": 0, "description": "Random seed; same inputs give byte-identical results"},
            "pattern": {
                "description": "Scenario pattern constraint",
                "oneOf": [
                    simple("script"),
                    {
                        "type": "object",
                        "properties": {
                            "kind": {"const": "concurrent_cluster"},
                            "window_minutes": {"type": "integer", "minimum": 1},
                            "min_disruptions": {"type": "integer", "minimum": 1}
                        },
                        "required": ["kind", "window_minutes", "min_disruptions"],
                        "additionalProperties": false
                    },
                    simple("decoy"),
                    simple("cascade"),
                    simple("resource_contention"),
                    simple("quiet"),
                    simple("unconstrained")
                ]
            },
            "start_minute": {"type": "integer", "minimum": 0, "maximum": crate::MAX_START_MINUTE, "description": "Virtual minute the simulation starts"},
            "horizon_minutes": {"type": "integer", "minimum": 1, "maximum": crate::scenario::MAX_HORIZON_MINUTES, "description": "Simulated duration (minutes)"},
            "mechanisms": {
                "type": ["object", "null"],
                "description": "Causal mechanism switches; null lets the generator sample them",
                "properties": {
                    "spindle_wear_to_dimension": {"type": "boolean"},
                    "tool_wear_to_dimension": {"type": "boolean"},
                    "material_lot_to_dimension": {"type": "boolean"}
                },
                "required": ["spindle_wear_to_dimension", "tool_wear_to_dimension", "material_lot_to_dimension"],
                "additionalProperties": false
            }
        },
        "required": ["seed", "pattern", "start_minute", "horizon_minutes"],
        "additionalProperties": false
    })
}

fn tool(name: &str, category: ToolCategory, description: &str, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        category,
        description: description.into(),
        input_schema,
    }
}

/// 全部工具（13 个，见设计文档 §11a 工具表）。
pub fn tool_catalog() -> Vec<ToolSpec> {
    use ToolCategory::*;
    let asset = ("asset", string("Optional machine id filter"), false);
    let [key, approval] = write_fields();
    vec![
        tool(
            "equipment_status",
            Observe,
            "Current observable state of machines: running status, vibration (mm/s) and ISO 20816 zone, model Cpk, tool presence and counter. Read-only.",
            schema(&[sim_field(), asset.clone()]),
        ),
        tool(
            "equipment_trend",
            Observe,
            "Linear trend over the last hour of vibration and Cpk, with the estimated minutes until each threshold. Read-only; a planning estimate, not a prediction of the simulated truth.",
            schema(&[sim_field(), asset]),
        ),
        tool(
            "quality_capability",
            Observe,
            "Process capability (model Cpk) per machine against the 1.33 threshold. Read-only.",
            schema(&[sim_field(), ("line", string("Optional line filter"), false)]),
        ),
        tool(
            "inventory_tooling",
            Observe,
            "Tooling inventory: spare tools in the store, transfer lead time, and which machines have the required tool. Read-only.",
            schema(&[sim_field()]),
        ),
        tool(
            "events_poll",
            Observe,
            "Events that have occurred (orders, vibration and quality alarms) after a cursor, in occurrence order. Returns next_cursor; polling the same cursor again returns the same page. Read-only.",
            schema(&[
                sim_field(),
                ("cursor", integer("Position after the last event already received (0 to start)"), true),
                ("limit", json!({"type": "integer", "minimum": 1, "maximum": MAX_POLL_LIMIT, "description": "Page size"}), true),
            ]),
        ),
        tool(
            "order_status",
            Observe,
            "Status of a work order created by an execution tool: accepted, completed or failed. Read-only.",
            schema(&[sim_field(), ("order_id", string("Work order id from a receipt"), true)]),
        ),
        tool(
            "maintenance_order_create",
            Execute,
            "Create a maintenance work order that stops a machine from start_minute for the configured repair time. External write: requires an idempotency key and an approval reference; returns a receipt with the completion minute.",
            schema(&[
                sim_field(),
                ("machine", string("Machine id"), true),
                ("start_minute", integer("Virtual minute the stop begins (not earlier than now)"), true),
                key.clone(),
                approval.clone(),
            ]),
        ),
        tool(
            "tooling_transfer_create",
            Execute,
            "Transfer a spare tool from the store to a machine. External write: requires an idempotency key and an approval reference; the receipt is failed when no spare is available.",
            schema(&[sim_field(), ("machine", string("Machine id"), true), key.clone(), approval.clone()]),
        ),
        tool(
            "production_schedule_apply",
            Execute,
            "Record that a production schedule plan was applied. External write: requires an idempotency key and an approval reference.",
            schema(&[
                sim_field(),
                ("plan_id", string("Plan identifier"), true),
                ("order", string("Optional customer order id"), false),
                key,
                approval,
            ]),
        ),
        tool(
            "simulation_create",
            Control,
            "Create a simulation from a factory and scenario configuration. Simulation control: for test schedulers and experiment scripts only.",
            json!({
                "type": "object",
                "properties": {
                    "name": string("Display name"),
                    "simulation_id": string("Optional id chosen by the caller, DES-<positive integer>; rejected if already used. Omit to let the factory number it"),
                    "factory": factory_schema(),
                    "scenario": scenario_schema()
                },
                "required": ["name", "factory", "scenario"],
                "additionalProperties": false
            }),
        ),
        tool(
            "simulation_advance",
            Control,
            "Advance the simulation clock to to_minute. Simulation control: for test schedulers and experiment scripts only.",
            schema(&[sim_field(), ("to_minute", integer("Target virtual minute (not earlier than now)"), true)]),
        ),
        tool(
            "simulation_fault_inject",
            Control,
            "Inject a fault into the simulated world, e.g. {\"kind\":\"tool_spares_lost\"}. Simulation control: for tests only.",
            schema(&[
                sim_field(),
                ("fault", json!({"type": "object", "properties": {"kind": {"type": "string", "enum": ["tool_spares_lost"]}}, "required": ["kind"], "additionalProperties": false}), true),
            ]),
        ),
        tool(
            "simulation_ground_truth",
            Admin,
            "Ground truth (root causes, true relations, resource contentions) for review. Administrative and sensitive: requires the admin token; never expose to agents.",
            schema(&[sim_field(), ("admin_token", string("Admin token configured on the service"), true)]),
        ),
    ]
}

// ───────────────────────── 仿真状态 ─────────────────────────

/// 一张工单的记录。
#[derive(Clone, Debug)]
struct Order {
    receipt: Value,
    /// 完成时刻；失败的工单为 None。
    complete_minute: Option<u64>,
}

/// 一个仿真的全部状态。
struct Sim {
    id: String,
    factory: FactoryConfig,
    scenario: ScenarioConfig,
    attempt: u32,
    interventions: Vec<Intervention>,
    spares_lost: bool,
    now: u64,
    /// 全部事件（含未来事件），按发生时刻排序；`occurred_at <= now` 的是已投递前缀，永不改变。
    events: Vec<FactoryEvent>,
    truth: GroundTruth,
    /// 遥测缓存：（干预数，推演到的时刻，遥测）。
    telemetry: Option<(usize, u64, Arc<Telemetry>)>,
    orders: BTreeMap<String, Order>,
    /// 幂等键 → （参数，单号）。
    keys: BTreeMap<String, (Value, String)>,
}

impl Sim {
    fn horizon_end(&self) -> u64 {
        self.scenario.start_minute + self.scenario.horizon_minutes
    }

    /// 至少覆盖到 `now` 的遥测（命中缓存时不重新推演）。
    fn telemetry(&mut self) -> Arc<Telemetry> {
        let horizon_end = self.horizon_end();
        let needed = self.now.max(horizon_end);
        if let Some((n, until, t)) = &self.telemetry {
            if *n == self.interventions.len() && *until >= needed {
                return t.clone();
            }
        }
        let until = if needed > horizon_end {
            needed + EXTENSION_CHUNK_MINUTES
        } else {
            horizon_end
        };
        let t = Arc::new(
            rerun(
                &self.factory,
                &self.scenario,
                self.attempt,
                &self.interventions,
                until,
            )
            .telemetry,
        );
        self.telemetry = Some((self.interventions.len(), until, t.clone()));
        t
    }

    fn observation(&mut self) -> Observation {
        let telemetry = self.telemetry();
        observe::observe(
            &self.id,
            &self.factory,
            &telemetry,
            self.scenario.start_minute,
            self.now,
            &self.interventions,
            self.spares_lost,
        )
    }

    /// 已投递事件数（事件按时刻排序，已投递的是前缀）。
    fn delivered(&self) -> usize {
        self.events.partition_point(|e| e.occurred_at <= self.now)
    }

    fn spares_left(&self) -> u32 {
        if self.spares_lost {
            return 0;
        }
        let used = self
            .interventions
            .iter()
            .filter(|iv| matches!(iv, Intervention::ToolTransfer { .. }))
            .count() as u32;
        self.factory.resources.tool_spares.saturating_sub(used)
    }

    fn machine_exists(&self, id: &str) -> bool {
        self.factory.machines.iter().any(|m| m.id == id)
    }

    /// 加入一项干预并重新推演：已发生的事件不变，之后的事件与标准答案按新干预替换。
    fn intervene(&mut self, iv: Intervention) {
        self.interventions.push(iv);
        let run = rerun(
            &self.factory,
            &self.scenario,
            self.attempt,
            &self.interventions,
            self.now,
        );
        let delivered = self.delivered();
        self.events.truncate(delivered);
        let known: std::collections::BTreeSet<String> =
            self.events.iter().map(|e| e.id.clone()).collect();
        let mut future: Vec<FactoryEvent> = run
            .events
            .into_iter()
            .map(|e| prefixed_event(&self.id, e))
            .filter(|e| e.occurred_at > self.now && !known.contains(&e.id))
            .collect();
        future.sort_by(|a, b| (a.occurred_at, &a.id).cmp(&(b.occurred_at, &b.id)));
        self.events.extend(future);
        self.truth = prefixed_truth(&self.id, run.truth);
        self.telemetry = Some((
            self.interventions.len(),
            self.now.max(self.horizon_end()),
            Arc::new(run.telemetry),
        ));
    }
}

/// 事件编号与批次号加上仿真编号前缀（与 MOM 进程内仿真一致）。
fn prefixed_event(id: &str, mut e: FactoryEvent) -> FactoryEvent {
    e.id = format!("{id}:{}", e.id);
    e.batch = format!("{id}:{}", e.batch);
    e
}

/// 标准答案中的事件编号加上仿真编号前缀，与事件一致。
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

// ───────────────────────── 参数 ─────────────────────────

fn parse<T: DeserializeOwned>(tool: &str, args: Value) -> Result<T> {
    serde_json::from_value(args).with_context(|| format!("invalid arguments for {tool}"))
}

/// 非空、不超长的文本。
fn text(field: &str, v: &str) -> Result<String> {
    let t = v.trim();
    ensure!(!t.is_empty(), "{field} is required");
    ensure!(t.len() <= MAX_TEXT, "{field} is too long");
    Ok(t.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimArgs {
    simulation_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetArgs {
    simulation_id: String,
    #[serde(default)]
    asset: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineArgs {
    simulation_id: String,
    #[serde(default)]
    line: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PollArgs {
    simulation_id: String,
    cursor: u64,
    limit: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderArgs {
    simulation_id: String,
    order_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvanceArgs {
    simulation_id: String,
    to_minute: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultArgs {
    simulation_id: String,
    fault: FaultKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultKind {
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TruthArgs {
    simulation_id: String,
    #[serde(default)]
    admin_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaintenanceArgs {
    simulation_id: String,
    machine: String,
    start_minute: u64,
    idempotency_key: String,
    approval_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferArgs {
    simulation_id: String,
    machine: String,
    idempotency_key: String,
    approval_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleArgs {
    simulation_id: String,
    plan_id: String,
    #[serde(default)]
    order: Option<String>,
    idempotency_key: String,
    approval_ref: String,
}

// ───────────────────────── 服务 ─────────────────────────

/// 模拟工厂服务：持有多个仿真的状态，按工具名分派调用。同步、确定，不读墙钟。
#[derive(Default)]
pub struct FactoryService {
    config: ServiceConfig,
    sims: BTreeMap<String, Sim>,
}

impl std::fmt::Debug for FactoryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryService")
            .field("simulations", &self.sims.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl FactoryService {
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            sims: BTreeMap::new(),
        }
    }

    /// 按工具名调用；参数与返回值都是 JSON，与 MCP 的 `tools/call` 一一对应。
    /// 返回错误时状态不变。
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value> {
        ensure!(args.is_object(), "arguments must be a JSON object");
        match tool {
            "simulation_create" => {
                // 可选的 `simulation_id` 不属于仿真配置，先取出再按配置严格解析（阶段⑤a-3，DES-28）。
                let mut args = args;
                let requested = match args.as_object_mut().and_then(|m| m.remove("simulation_id")) {
                    None | Some(Value::Null) => None,
                    Some(Value::String(id)) => Some(id),
                    Some(other) => bail!("simulation_id must be a string, got {other}"),
                };
                self.create(parse(tool, args)?, requested)
            }
            "simulation_advance" => self.advance(parse(tool, args)?),
            "simulation_fault_inject" => self.fault(parse(tool, args)?),
            "simulation_ground_truth" => self.ground_truth(parse(tool, args)?),
            "equipment_status" => {
                let a: AssetArgs = parse(tool, args)?;
                let sim = self.sim(&a.simulation_id)?;
                let o = sim.observation();
                let mut view = observe::factory_view(&o);
                filter_machines(&mut view, a.asset.as_deref(), sim)?;
                Ok(view)
            }
            "equipment_trend" => {
                let a: AssetArgs = parse(tool, args)?;
                let sim = self.sim(&a.simulation_id)?;
                let telemetry = sim.telemetry();
                let mut view = observe::forecast(
                    &sim.id,
                    &sim.factory,
                    &telemetry,
                    sim.scenario.start_minute,
                    sim.now,
                );
                filter_machines(&mut view, a.asset.as_deref(), sim)?;
                Ok(view)
            }
            "quality_capability" => {
                let a: LineArgs = parse(tool, args)?;
                let sim = self.sim(&a.simulation_id)?;
                if let Some(line) = &a.line {
                    ensure!(
                        sim.factory.machines.iter().any(|m| &m.line == line),
                        "unknown line {line}"
                    );
                }
                let o = sim.observation();
                let machines: Vec<Value> = o
                    .machines
                    .iter()
                    .filter(|m| a.line.as_ref().is_none_or(|l| &m.line == l))
                    .map(|m| {
                        json!({"id": m.id, "line": m.line, "cpk": m.cpk, "threshold": CPK_THRESHOLD, "below_threshold": m.cpk < CPK_THRESHOLD})
                    })
                    .collect();
                Ok(
                    json!({"simulation_id": o.simulation_id, "observed_at": o.observed_at, "machines": machines, "provenance": "simulation"}),
                )
            }
            "inventory_tooling" => {
                let a: SimArgs = parse(tool, args)?;
                let sim = self.sim(&a.simulation_id)?;
                let o = sim.observation();
                let machines: Vec<Value> = o
                    .machines
                    .iter()
                    .map(|m| json!({"id": m.id, "tool_present": m.tool_present, "tool_parts": m.tool_parts}))
                    .collect();
                Ok(json!({
                    "simulation_id": o.simulation_id,
                    "observed_at": o.observed_at,
                    "tool_spares": sim.spares_left(),
                    "transfer_minutes": sim.factory.resources.tool_transfer_minutes,
                    "machines": machines,
                    "provenance": "simulation",
                }))
            }
            "events_poll" => {
                let a: PollArgs = parse(tool, args)?;
                ensure!(
                    (1..=MAX_POLL_LIMIT).contains(&a.limit),
                    "limit must be 1..={MAX_POLL_LIMIT}"
                );
                let sim = self.sim(&a.simulation_id)?;
                let delivered = sim.delivered();
                ensure!(
                    a.cursor as usize <= delivered,
                    "cursor {} is beyond the {delivered} events that have occurred",
                    a.cursor
                );
                let start = a.cursor as usize;
                let end = (start + a.limit as usize).min(delivered);
                Ok(json!({
                    "simulation_id": sim.id,
                    "now": sim.now,
                    "events": &sim.events[start..end],
                    "next_cursor": end,
                    "provenance": "simulation",
                }))
            }
            "order_status" => {
                let a: OrderArgs = parse(tool, args)?;
                let sim = self.sim(&a.simulation_id)?;
                let order = sim.orders.get(&a.order_id).context("unknown order")?;
                let mut out = order.receipt.clone();
                out["status"] = match order.complete_minute {
                    None => json!("failed"),
                    Some(done) if sim.now >= done => json!("completed"),
                    Some(_) => json!("accepted"),
                };
                out["now"] = json!(sim.now);
                Ok(out)
            }
            "maintenance_order_create" => {
                let a: MaintenanceArgs = parse(tool, args)?;
                let key = text("idempotency_key", &a.idempotency_key)?;
                let approval = text("approval_ref", &a.approval_ref)?;
                let canonical = json!({"kind": "maintenance", "machine": a.machine, "start_minute": a.start_minute, "approval_ref": approval});
                let sim = self.sim(&a.simulation_id)?;
                if let Some(receipt) = replay(sim, &key, &canonical)? {
                    return Ok(receipt);
                }
                ensure!(
                    sim.machine_exists(&a.machine),
                    "unknown machine {}",
                    a.machine
                );
                let ahead = sim.now.saturating_add(MAX_SCHEDULE_AHEAD_MINUTES);
                ensure!(
                    a.start_minute >= sim.now && a.start_minute <= ahead,
                    "start_minute must be between now ({}) and {} minutes ahead",
                    sim.now,
                    MAX_SCHEDULE_AHEAD_MINUTES
                );
                ensure!(sim.orders.len() < MAX_ORDERS, "work order capacity reached");
                // 同一设备维修未结束时拒绝再下单（R-20261119-M13）。
                let busy = sim.interventions.iter().any(|iv| match iv {
                    Intervention::Maintenance {
                        machine,
                        end_minute,
                        ..
                    } => machine == &a.machine && *end_minute > a.start_minute,
                    _ => false,
                });
                ensure!(
                    !busy,
                    "machine {} already has an overlapping maintenance order",
                    a.machine
                );
                let prior: Vec<(u64, u64)> = sim
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
                let crews = sim.factory.resources.maintenance_crews.max(1) as usize;
                let mut free = crate::crew_free_after(crews, &prior);
                let (begin, end) = crate::schedule_next_maintenance(
                    a.start_minute,
                    sim.factory.resources.maintenance_minutes,
                    &mut free,
                )?;
                sim.intervene(Intervention::Maintenance {
                    machine: a.machine.clone(),
                    start_minute: begin,
                    end_minute: end,
                });
                Ok(record(
                    sim,
                    key,
                    canonical,
                    json!({
                        "kind": "maintenance",
                        "machine": a.machine,
                        "start_minute": begin,
                        "complete_minute": end,
                        "status": "accepted",
                    }),
                    Some(end),
                ))
            }
            "tooling_transfer_create" => {
                let a: TransferArgs = parse(tool, args)?;
                let key = text("idempotency_key", &a.idempotency_key)?;
                let approval = text("approval_ref", &a.approval_ref)?;
                let canonical = json!({"kind": "tool_transfer", "machine": a.machine, "approval_ref": approval});
                let sim = self.sim(&a.simulation_id)?;
                if let Some(receipt) = replay(sim, &key, &canonical)? {
                    return Ok(receipt);
                }
                ensure!(
                    sim.machine_exists(&a.machine),
                    "unknown machine {}",
                    a.machine
                );
                ensure!(sim.orders.len() < MAX_ORDERS, "work order capacity reached");
                if sim.spares_left() == 0 {
                    // 失败也是一个回执：原因来自仿真世界（备用刀具不足或已丢失）。
                    let reason = if sim.spares_lost {
                        "no tool spares available: store spares were lost"
                    } else {
                        "no tool spares available"
                    };
                    return Ok(record(
                        sim,
                        key,
                        canonical,
                        json!({
                            "kind": "tool_transfer",
                            "machine": a.machine,
                            "status": "failed",
                            "reason": reason,
                        }),
                        None,
                    ));
                }
                let arrive = sim.now + sim.factory.resources.tool_transfer_minutes;
                sim.intervene(Intervention::ToolTransfer {
                    machine: a.machine.clone(),
                    arrive_minute: arrive,
                });
                Ok(record(
                    sim,
                    key,
                    canonical,
                    json!({
                        "kind": "tool_transfer",
                        "machine": a.machine,
                        "complete_minute": arrive,
                        "status": "accepted",
                    }),
                    Some(arrive),
                ))
            }
            "production_schedule_apply" => {
                let a: ScheduleArgs = parse(tool, args)?;
                let key = text("idempotency_key", &a.idempotency_key)?;
                let approval = text("approval_ref", &a.approval_ref)?;
                let plan = text("plan_id", &a.plan_id)?;
                let order = a.order.as_deref().map(|o| text("order", o)).transpose()?;
                let canonical = json!({"kind": "schedule", "plan_id": plan, "order": order, "approval_ref": approval});
                let sim = self.sim(&a.simulation_id)?;
                if let Some(receipt) = replay(sim, &key, &canonical)? {
                    return Ok(receipt);
                }
                ensure!(sim.orders.len() < MAX_ORDERS, "work order capacity reached");
                let now = sim.now;
                // 排产只记录（产能分配由接入方的计划模型给出），不改变仿真世界。
                Ok(record(
                    sim,
                    key,
                    canonical,
                    json!({
                        "kind": "schedule",
                        "plan_id": plan,
                        "order": order,
                        "complete_minute": now,
                        "status": "accepted",
                    }),
                    Some(now),
                ))
            }
            other => bail!("unknown tool {other}"),
        }
    }

    fn sim(&mut self, id: &str) -> Result<&mut Sim> {
        self.sims
            .get_mut(id)
            .with_context(|| format!("unknown simulation {id}"))
    }

    fn create(&mut self, config: SimulationConfig, requested: Option<String>) -> Result<Value> {
        let name = text("name", &config.name)?;
        ensure!(
            self.sims.len() < MAX_SIMULATIONS,
            "simulation capacity reached ({MAX_SIMULATIONS})"
        );
        // 编号：调用方指定时必须形如 `DES-正整数` 且未被使用（连接器传入引擎的下一个编号，
        // 避免调用方重启后与旧仿真撞号）；不指定时取从 `DES-(已有数+1)` 起第一个未用的编号。
        let id = match requested {
            Some(id) => {
                ensure!(
                    id.strip_prefix("DES-")
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|n| n > 0 && format!("DES-{n}") == id),
                    "simulation_id must look like DES-<positive integer>"
                );
                ensure!(
                    !self.sims.contains_key(&id),
                    "simulation {id} already exists"
                );
                id
            }
            None => (self.sims.len() + 1..)
                .map(|n| format!("DES-{n}"))
                .find(|id| !self.sims.contains_key(id))
                .expect("a free id exists"),
        };
        let factory = config.factory.unwrap_or_else(FactoryConfig::line_b_default);
        let scenario = config.scenario;
        let (run, attempt) = run_with_attempt(&factory, &scenario)?;
        let events: Vec<FactoryEvent> = run
            .events
            .into_iter()
            .map(|e| prefixed_event(&id, e))
            .collect();
        let horizon_end = scenario.start_minute + scenario.horizon_minutes;
        let sim = Sim {
            id: id.clone(),
            factory,
            now: scenario.start_minute,
            attempt,
            interventions: vec![],
            spares_lost: false,
            events,
            truth: prefixed_truth(&id, run.truth),
            telemetry: Some((0, horizon_end, Arc::new(run.telemetry))),
            orders: BTreeMap::new(),
            keys: BTreeMap::new(),
            scenario,
        };
        let now = sim.now;
        self.sims.insert(id.clone(), sim);
        Ok(json!({"simulation_id": id, "name": name, "now": now, "provenance": "simulation"}))
    }

    fn advance(&mut self, a: AdvanceArgs) -> Result<Value> {
        let sim = self.sim(&a.simulation_id)?;
        ensure!(
            a.to_minute >= sim.now,
            "the clock cannot go back (now {})",
            sim.now
        );
        ensure!(
            a.to_minute - sim.now <= MAX_ADVANCE_MINUTES,
            "advance at most {MAX_ADVANCE_MINUTES} minutes at a time"
        );
        let before = sim.delivered();
        sim.now = a.to_minute;
        let delivered = sim.delivered() - before;
        Ok(json!({"simulation_id": sim.id, "now": sim.now, "delivered": delivered}))
    }

    fn fault(&mut self, a: FaultArgs) -> Result<Value> {
        let sim = self.sim(&a.simulation_id)?;
        match a.fault.kind.as_str() {
            "tool_spares_lost" => sim.spares_lost = true,
            other => bail!("unknown fault {other}"),
        }
        Ok(json!({"simulation_id": sim.id, "fault": a.fault.kind, "now": sim.now}))
    }

    fn ground_truth(&mut self, a: TruthArgs) -> Result<Value> {
        let Some(expected) = self.config.admin_token.as_deref() else {
            bail!("administrative tools are disabled: no admin token configured");
        };
        ensure!(
            a.admin_token.as_deref() == Some(expected),
            "admin token required"
        );
        let sim = self.sim(&a.simulation_id)?;
        Ok(serde_json::to_value(&sim.truth)?)
    }
}

/// 只保留指定设备（若给出）。
fn filter_machines(view: &mut Value, asset: Option<&str>, sim: &Sim) -> Result<()> {
    let Some(asset) = asset else { return Ok(()) };
    ensure!(sim.machine_exists(asset), "unknown machine {asset}");
    if let Some(list) = view["machines"].as_array_mut() {
        list.retain(|m| m["id"] == asset);
    }
    Ok(())
}

/// 幂等：同键同参数返回原回执；同键不同参数报错；新键返回 None。
fn replay(sim: &Sim, key: &str, canonical: &Value) -> Result<Option<Value>> {
    match sim.keys.get(key) {
        None => Ok(None),
        Some((args, order_id)) => {
            ensure!(
                args == canonical,
                "idempotency key {key} was already used with different arguments"
            );
            Ok(Some(sim.orders[order_id].receipt.clone()))
        }
    }
}

/// 记录一张工单与它的幂等键，返回回执。
fn record(
    sim: &mut Sim,
    key: String,
    canonical: Value,
    mut receipt: Value,
    complete_minute: Option<u64>,
) -> Value {
    let order_id = format!("WO-{}", sim.orders.len() + 1);
    receipt["order_id"] = json!(order_id);
    receipt["simulation_id"] = json!(sim.id);
    receipt["approval_ref"] = canonical["approval_ref"].clone();
    receipt["idempotency_key"] = json!(key);
    receipt["accepted_at"] = json!(sim.now);
    receipt["provenance"] = json!("simulation");
    sim.orders.insert(
        order_id.clone(),
        Order {
            receipt: receipt.clone(),
            complete_minute,
        },
    );
    sim.keys.insert(key, (canonical, order_id));
    receipt
}
