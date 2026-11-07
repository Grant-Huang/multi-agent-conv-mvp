use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventInput {
    pub id: String,
    pub scenario: String,
    pub kind: String,
    pub source: String,
    pub occurred_at: u64,
    pub batch: String,
    pub customer: String,
    pub asset: String,
    pub payload: Value,
}
impl EventInput {
    pub fn s1(kind: &str, at: u64) -> Self {
        Self {
            id: format!("s1-{kind}"),
            scenario: "s1".into(),
            kind: kind.into(),
            source: match kind {
                "order" => "MES",
                "vibration" => "EAM",
                _ => "QMS",
            }
            .into(),
            occurred_at: at,
            batch: "B77".into(),
            customer: "A".into(),
            asset: "CNC-XX".into(),
            payload: json!({"line":"Line-B","quantity":500}),
        }
    }
    pub fn complaint(id: &str, at: u64) -> Self {
        Self {
            id: id.into(),
            scenario: "s2".into(),
            kind: "complaint".into(),
            source: "customer".into(),
            occurred_at: at,
            batch: "B77".into(),
            customer: "A".into(),
            asset: "CNC-XX".into(),
            payload: json!({"shipment":"C100","ctq":"diameter","samples":5}),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub event: EventInput,
    pub received_at: u64,
    pub provenance: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub tool: String,
    pub deps: Vec<String>,
    pub status: String,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub due_at: Option<u64>,
    pub output: Option<Value>,
    pub evidence_refs: Vec<String>,
}
impl Node {
    pub fn new(tool: &str, deps: &[&str]) -> Self {
        Self {
            tool: tool.into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            status: "READY".into(),
            started_at: None,
            finished_at: None,
            due_at: None,
            output: None,
            evidence_refs: vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Approval {
    pub plan_revision: u64,
    pub scope: String,
    pub approver: String,
    pub expires_at: u64,
    pub resource_version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub key: String,
    pub tool: String,
    pub state: String,
    pub accepted_at: u64,
    pub complete_at: u64,
    pub receipt: String,
    pub verification: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Child {
    pub token: u64,
    pub kind: String,
    pub status: String,
    pub started_at: u64,
    pub lease_until: u64,
    pub due_at: u64,
    pub input_revision: u64,
    pub report: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub scenario: String,
    pub batch: String,
    pub customer: String,
    pub asset: String,
    #[serde(default)]
    pub updated_at: u64,
    pub revision: u64,
    pub plan_revision: u64,
    pub stage: String,
    pub template: String,
    pub constraints: Vec<String>,
    pub observations: Vec<Observation>,
    pub nodes: BTreeMap<String, Node>,
    pub evidence: BTreeMap<String, Value>,
    pub proposal: Option<Value>,
    pub initial_report: Option<Value>,
    pub eight_d: Option<Value>,
    pub approvals: BTreeMap<String, Approval>,
    pub actions: BTreeMap<String, Action>,
    pub children: Vec<Child>,
    pub outbox: Vec<Value>,
    pub world: Value,
    pub resource_version: u64,
    pub resource_observed_at: u64,
    pub budget_remaining: u32,
    pub tool_calls: u32,
    pub model_calls: u32,
    pub replans: u32,
    pub deadlines: BTreeMap<String, u64>,
    pub alerts: Vec<String>,
    pub breaches: Vec<String>,
    pub mock_ids: Vec<String>,
    pub cancelled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardEvent {
    pub sequence: u64,
    pub case_id: String,
    pub kind: String,
    pub at: u64,
    pub revision: u64,
    pub payload: Value,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub simulations: BTreeMap<String, super::Simulation>,
    #[serde(default)]
    pub playback: super::Playback,
    pub now: u64,
    pub cases: BTreeMap<String, Case>,
    pub seen: BTreeMap<String, String>,
    pub next_token: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    SelectTemplate { template: String },
    ProposePatch { node: String, dependency: String },
    RequestDelegation { kind: String },
    RequestDecision { scope: String },
    SubmitSummary { evidence_refs: Vec<String> },
    Wait,
}

pub fn fixture_world(scenario: &str) -> Value {
    if scenario == "s1" {
        serde_json::from_str(include_str!("../../../../fixtures/mom/s1/world.json"))
            .expect("embedded fixture")
    } else {
        serde_json::from_str(include_str!("../../../../fixtures/mom/s2/world.json"))
            .expect("embedded fixture")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationPolicy {
    pub latency_minutes: u64,
    pub max_parallel: usize,
    pub child_latency_minutes: u64,
    pub child_lease_minutes: u64,
    pub approval_ttl_minutes: u64,
    pub snapshot_ttl_minutes: u64,
    pub mock_ids: Vec<String>,
}
pub fn fixture_policy(scenario: &str) -> SimulationPolicy {
    let text = if scenario == "s1" {
        include_str!("../../../../fixtures/mom/s1/tools.json")
    } else {
        include_str!("../../../../fixtures/mom/s2/tools.json")
    };
    serde_json::from_str(text).expect("embedded simulator policy")
}
