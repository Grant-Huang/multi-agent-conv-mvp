use super::{model::*, tools};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Emissions = Vec<(String, String, Value, u64, u64)>;
pub struct Engine {
    db: Connection,
    state: Snapshot,
    sequence: u64,
}
impl Engine {
    pub fn memory() -> Result<Self> {
        Self::from_db(Connection::open_in_memory()?)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_db(Connection::open(path)?)
    }
    fn from_db(db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version <= 1, "unsupported MOM schema version");
        db.execute_batch("BEGIN IMMEDIATE;
          CREATE TABLE IF NOT EXISTS mom_snapshot(id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL, seq INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS mom_events(seq INTEGER PRIMARY KEY, case_id TEXT NOT NULL, body TEXT NOT NULL, published INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS mom_cursors(consumer TEXT NOT NULL, case_id TEXT NOT NULL, seq INTEGER NOT NULL, PRIMARY KEY(consumer,case_id));
          PRAGMA user_version=1; COMMIT;")?;
        let row = db.query_row("SELECT body,seq FROM mom_snapshot WHERE id=1", [], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
        });
        let (state, sequence) = match row {
            Ok((s, n)) => (serde_json::from_str(&s)?, n),
            Err(rusqlite::Error::QueryReturnedNoRows) => (Snapshot::default(), 0),
            Err(e) => return Err(e.into()),
        };
        let mut e = Self {
            db,
            state,
            sequence,
        };
        e.update(|s, events| {
            for c in s.cases.values_mut() {
                let mut changed = false;
                for ch in &mut c.children {
                    if ch.status == "RUNNING" {
                        ch.status = "TIMED_OUT".into();
                        changed = true;
                    }
                }
                if changed {
                    emit(
                        c,
                        "LeaseExpired",
                        json!({"reason":"runtime restart"}),
                        events,
                    );
                }
            }
            Ok(())
        })?;
        Ok(e)
    }
    fn update<T>(
        &mut self,
        f: impl FnOnce(&mut Snapshot, &mut Emissions) -> Result<T>,
    ) -> Result<T> {
        let mut next = self.state.clone();
        let mut emissions = vec![];
        let result = f(&mut next, &mut emissions)?;
        // Every persistent state change must advance the compare-and-swap token,
        // including clock-only transactions and recovery metadata.
        if emissions.is_empty() && serde_json::to_vec(&next)? != serde_json::to_vec(&self.state)? {
            emissions.push((
                "__runtime".into(),
                "RuntimeStateChanged".into(),
                json!({"now":next.now}),
                next.now,
                0,
            ));
        }

        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = tx
            .query_row("SELECT seq FROM mom_snapshot WHERE id=1", [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap_or(0);
        ensure!(
            saved == self.sequence,
            "stale runtime; reopen database before retrying"
        );
        let mut sequence = self.sequence;
        for (id, kind, payload, at, revision) in emissions {
            sequence += 1;
            let event = BoardEvent {
                sequence,
                revision,
                case_id: id.clone(),
                kind,
                at,
                payload,
            };
            tx.execute(
                "INSERT INTO mom_events(seq,case_id,body) VALUES(?1,?2,?3)",
                params![sequence, id, serde_json::to_string(&event)?],
            )?;
        }
        tx.execute("INSERT INTO mom_snapshot(id,body,seq) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body,seq=excluded.seq",params![serde_json::to_string(&next)?,sequence])?;
        tx.commit()?;
        self.state = next;
        self.sequence = sequence;
        Ok(result)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn now(&self) -> u64 {
        self.state.now
    }
    pub fn case(&self, id: &str) -> Result<&Case> {
        self.state.cases.get(id).context("unknown case")
    }
    pub fn case_ids(&self) -> Vec<String> {
        self.state.cases.keys().cloned().collect()
    }
    pub fn ingest(&mut self, input: EventInput) -> Result<String> {
        ensure!(
            matches!(input.scenario.as_str(), "s1" | "s2"),
            "unknown scenario"
        );
        ensure!(
            matches!(
                input.source.as_str(),
                "MES" | "QMS" | "EAM" | "WMS" | "customer"
            ),
            "unsupported fixture source"
        );
        ensure!(
            !input.id.is_empty() && !input.batch.is_empty() && !input.customer.is_empty(),
            "event identity and genealogy required"
        );
        ensure!(
            input.id.len() <= 200 && serde_json::to_vec(&input)?.len() <= 65536,
            "event too large"
        );
        self.update(|s,events|{
            // Source-scoped event identity. Same event key with changed content is a conflict.
            let key=format!("{}:{}",input.source,input.id);
            if let Some(id)=s.seen.get(&key){let c=&s.cases[id];let old=c.observations.iter().find(|o|o.event.source==input.source&&o.event.id==input.id).context("missing original event")?;ensure!(serde_json::to_value(&old.event)?==serde_json::to_value(&input)?,"event identity conflict");return Ok(id.clone());}
            s.now=s.now.max(input.occurred_at);
            let id=s.cases.values().find(|c|c.scenario==input.scenario&&c.batch==input.batch&&c.customer==input.customer&&c.asset==input.asset&&!matches!(c.stage.as_str(),"CLOSED"|"CANCELLED")).map(|c|c.id.clone()).unwrap_or_else(||format!("CASE-{}-{}",input.scenario,s.cases.len()+1));
            let received_at=s.now;
            let c=s.cases.entry(id.clone()).or_insert_with(||new_case(&id,&input,received_at));
            if !c.nodes.is_empty() {
                c.resource_version+=1;c.approvals.clear();
                if c.actions.is_empty(){
                    c.plan_revision+=1;c.proposal=None;c.initial_report=None;c.eight_d=None;
                    for n in c.nodes.values_mut(){n.status="READY".into();n.output=None;n.started_at=None;n.finished_at=None;n.due_at=None;}
                    for ch in &mut c.children{if ch.status=="RUNNING"{ch.status="SUPERSEDED".into();}}
                    c.stage="ASSESS".into();
                }else{c.stage="BLOCKED".into();}
                emit(c,"PlanInvalidated",json!({"reason":"new relevant observation"}),events);
            }
            c.observations.push(Observation{event:input.clone(),received_at:s.now,provenance:"mock".into()});
            let evid=format!("obs:{}:{}",input.source,input.id);c.evidence.insert(evid.clone(),json!({"source":input.source,"payload":input.payload,"occurred_at":input.occurred_at,"received_at":s.now,"provenance":"mock","mock_ids":["M01"]}));
            emit(c,"ObservationRecorded",json!({"evidence_ref":evid}),events);
            if input.payload["safety_trip"]==true || c.world["safety_trip"]==true {c.world["safety_trip"]=json!(true);c.stage="BLOCKED".into();c.approvals.clear();emit(c,"SafetyBlocked",json!({"safety_system":"external interlock; simulator does not control equipment"}),events);}
            else {instantiate(c,events)?;}
            s.seen.insert(key,id.clone());
            pump(c,s.now,events)?;
            Ok(id)
        })
    }
    pub fn advance(&mut self, to: u64) -> Result<()> {
        ensure!(to >= self.state.now, "virtual clock cannot reverse");
        ensure!(to - self.state.now <= 100_000, "advance horizon too large");
        self.update(|s, events| {
            for now in s.now..=to {
                for c in s.cases.values_mut() {
                    tick(c, now, events)?;
                }
                s.now = now;
            }
            Ok(())
        })
    }
    pub fn change_world(&mut self, id: &str, key: &str, value: Value) -> Result<()> {
        const ALLOWED: &[&str] = &[
            "resources_available",
            "certified",
            "tool_available",
            "resource_conflict",
            "maintenance_timeout",
            "quality_timeout",
            "dispatch_failure",
            "lost_receipt",
            "child_timeout",
            "missing_genealogy",
            "conflicting_records",
            "history",
            "quantity",
            "capacity_xy",
            "capacity_xz",
            "root_cause_verified",
            "effectiveness_verified",
        ];
        ensure!(ALLOWED.contains(&key), "unknown fault injection");
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            c.world[key] = value;
            if matches!(
                key,
                "resources_available"
                    | "certified"
                    | "tool_available"
                    | "resource_conflict"
                    | "quantity"
                    | "capacity_xy"
                    | "capacity_xz"
            ) {
                c.resource_version += 1;
            }
            emit(c, "WorldChanged", json!({"key":key}), events);
            Ok(())
        })
    }
    pub fn approve(&mut self, id: &str, revision: u64, scope: &str, actor: &str) -> Result<()> {
        ensure!(
            actor == "supervisor",
            "only fixture supervisor may approve; enterprise authentication not implemented"
        );
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            ensure!(!c.cancelled && c.stage != "BLOCKED", "case blocked");
            ensure!(revision == c.plan_revision, "stale plan approval");
            match scope {
                "production" => ensure!(
                    c.scenario == "s1"
                        && c.proposal.as_ref().is_some_and(|p| p["feasible"] == true),
                    "no feasible production plan"
                ),
                "initial_response" => ensure!(
                    c.scenario == "s2" && c.initial_report.is_some(),
                    "report unavailable"
                ),
                "eight_d" => ensure!(c.scenario == "s2" && c.eight_d.is_some(), "8D unavailable"),
                _ => bail!("unknown approval scope"),
            }
            ensure!(
                s.now <= c.resource_observed_at + fixture_policy(&c.scenario).snapshot_ttl_minutes,
                "evidence snapshot stale; refresh plan"
            );
            c.approvals.insert(
                scope.into(),
                Approval {
                    plan_revision: revision,
                    scope: scope.into(),
                    approver: actor.into(),
                    expires_at: s.now + fixture_policy(&c.scenario).approval_ttl_minutes,
                    resource_version: c.resource_version,
                },
            );
            emit(
                c,
                "DecisionApproved",
                json!({"scope":scope,"plan_revision":revision}),
                events,
            );
            Ok(())
        })
    }
    pub fn execute(&mut self, id: &str, scope: &str) -> Result<()> {
        self.update(|s,events|{
            let resources_busy=s.cases.iter().any(|(other,c)|other!=id&&c.world["reservations"].as_object().is_some_and(|r|!r.is_empty()));
            let c=s.cases.get_mut(id).context("unknown case")?;ensure!(!c.cancelled&&c.stage!="BLOCKED","case blocked");
            // A completed idempotent command can be queried again even after approval expires.
            let group=format!("{}:{}:{}",c.id,c.plan_revision,scope);
            if c.actions.values().any(|a|a.key.starts_with(&format!("{group}:"))){return Ok(());}
            let a=c.approvals.get(scope).context("approval required")?;
            ensure!(a.plan_revision==c.plan_revision&&s.now<=a.expires_at&&a.resource_version==c.resource_version,"approval stale or expired");
            ensure!(s.now<=c.resource_observed_at+fixture_policy(&c.scenario).snapshot_ttl_minutes,"snapshot expired");
            if scope=="production" {
                ensure!(!resources_busy,"resources reserved by another case");
                ensure!(c.world["resources_available"]==true&&c.world["certified"]==true&&c.world["tool_available"]==true&&c.world["resource_conflict"]!=true,"resources unavailable");
                charge_tools(c,3)?;
                c.world["reservations"]["tool-32"]=json!(c.id);c.world["reservations"]["XY-XZ"]=json!(c.id);
                for tool in ["mro.transfer","eam.maintenance","mes.reschedule"] {
                    let key=format!("{group}:{tool}");let failed=tool=="mes.reschedule"&&c.world["dispatch_failure"]==true;
                    c.actions.insert(key.clone(),Action{key:key.clone(),tool:tool.into(),state:if failed{"FAILED"}else if c.world["lost_receipt"]==true{"UNKNOWN"}else{"ACCEPTED"}.into(),accepted_at:s.now,complete_at:s.now+2,receipt:format!("mock-op:{key}"),verification:None});
                }
                c.stage=if c.world["dispatch_failure"]==true {c.world["reservations"]=json!({});"PARTIAL"}else{"VERIFYING"}.into();
                if c.stage=="PARTIAL"{emit(c,"CompensationRequired",json!({"released":"reservations","irreversible":"accepted maintenance and transfer need operator review"}),events);}
            }else {
                charge_tools(c,1)?;
                let report=if scope=="initial_response"{c.initial_report.clone()}else{c.eight_d.clone()}.context("report missing")?;
                let key=format!("{group}:customer.send");
                c.actions.insert(key.clone(),Action{key:key.clone(),tool:"customer.send".into(),state:"ACCEPTED".into(),accepted_at:s.now,complete_at:s.now+1,receipt:format!("mock-op:{key}"),verification:None});
                c.outbox.push(json!({"key":key,"customer":c.customer,"report":report,"provenance":"mock","mock_ids":["M16"],"sent_at":s.now}));
                c.stage="INVESTIGATING".into();
            }
            emit(c,"ActionAccepted",json!({"scope":scope,"provenance":"mock"}),events);Ok(())})
    }
    pub fn close(&mut self, id: &str) -> Result<()> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            let s1 = c.scenario == "s1" && c.stage == "VERIFYING";
            let s2 = c.scenario == "s2"
                && c.eight_d
                    .as_ref()
                    .is_some_and(|v| v["formal_complete"] == true)
                && c.outbox
                    .iter()
                    .any(|v| v["report"]["formal_complete"] == true);
            ensure!(
                (s1 || s2)
                    && !c.actions.is_empty()
                    && c.actions.values().all(|a| a.state == "VERIFIED"),
                "actual verification incomplete; complaint closure requires validated D4/D6/D8"
            );
            c.stage = "CLOSED".into();
            for ch in &mut c.children {
                if ch.status == "RUNNING" {
                    ch.status = "CANCELLED".into();
                }
            }
            c.world["reservations"] = json!({});
            emit(
                c,
                "CaseClosed",
                json!({"scope":"simulated receipts only"}),
                events,
            );
            Ok(())
        })
    }
    /// Refresh stale read evidence without granting new authority or budget.
    pub fn refresh_plan(&mut self, id: &str) -> Result<()> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            ensure!(
                !c.cancelled && c.world["safety_trip"] != true,
                "case cancelled or safety blocked"
            );
            ensure!(
                c.replans < 2 && c.budget_remaining >= c.nodes.len() as u32,
                "refresh budget exhausted"
            );
            ensure!(
                c.actions.is_empty(),
                "accepted actions require reconciliation, not plan reset"
            );
            c.replans += 1;
            c.plan_revision += 1;
            c.approvals.clear();
            c.proposal = None;
            c.initial_report = None;
            c.eight_d = None;
            for n in c.nodes.values_mut() {
                n.status = "READY".into();
                n.started_at = None;
                n.finished_at = None;
                n.due_at = None;
                n.output = None;
            }
            for ch in &mut c.children {
                if ch.status == "RUNNING" {
                    ch.status = "SUPERSEDED".into();
                }
            }
            c.stage = "ASSESS".into();
            emit(
                c,
                "PlanRefreshed",
                json!({"revision":c.plan_revision}),
                events,
            );
            pump(c, s.now, events)
        })
    }
    pub fn set_budget(&mut self, id: &str, remaining: u32) -> Result<()> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            ensure!(
                remaining <= c.budget_remaining,
                "cannot silently increase budget"
            );
            c.budget_remaining = remaining;
            emit(c, "BudgetChanged", json!({"remaining":remaining}), events);
            Ok(())
        })
    }
    pub fn delegate(&mut self, id: &str, kind: &str, depth: u32) -> Result<u64> {
        ensure!(depth == 0, "recursive delegation forbidden");
        self.update(|s,events|{
            let global=s.cases.values().flat_map(|c|&c.children).filter(|ch|ch.status=="RUNNING").count();ensure!(global<4,"global child capacity exhausted");
            let c=s.cases.get_mut(id).context("unknown case")?;
            ensure!(!c.cancelled&&!matches!(c.stage.as_str(),"BLOCKED"|"CLOSED"),"inactive case");
            ensure!((c.scenario=="s1"&&kind=="equipment_investigation")||(c.scenario=="s2"&&kind=="complaint_trace_investigation"),"unsupported child capability");
            ensure!(c.children.len()<2&&!c.children.iter().any(|ch|ch.status=="RUNNING"),"child quota exhausted");
            charge(c,6)?;s.next_token+=1;let token=s.next_token;
            c.children.push(Child{token,kind:kind.into(),status:"RUNNING".into(),started_at:s.now,lease_until:s.now+fixture_policy(&c.scenario).child_lease_minutes,due_at:s.now+fixture_policy(&c.scenario).child_latency_minutes,input_revision:c.plan_revision,report:None});
            emit(c,"ChildStarted",json!({"token":token,"kind":kind,"read_only":true,"provenance":"mock","mock_ids":["M19"]}),events);Ok(token)})
    }
    pub fn submit_child(&mut self, id: &str, token: u64, refs: Vec<String>) -> Result<()> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            submit_child(c, token, refs, s.now, events)
        })
    }
    pub fn cancel(&mut self, id: &str) -> Result<()> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            c.cancelled = true;
            c.stage = "CANCELLED".into();
            c.approvals.clear();
            for node in c.nodes.values_mut() {
                if matches!(node.status.as_str(), "READY" | "RUNNING") {
                    node.status = "CANCELLED".into();
                }
            }
            for ch in &mut c.children {
                if ch.status == "RUNNING" {
                    ch.status = "CANCELLED".into();
                }
            }
            c.world["reservations"] = json!({});
            emit(
                c,
                "CaseCancelled",
                json!({"accepted_actions":"must still be reconciled; not rolled back"}),
                events,
            );
            Ok(())
        })
    }
    pub fn apply_intent(&mut self, id: &str, intent: Intent) -> Result<()> {
        match intent {
            Intent::RequestDelegation { kind } => self.delegate(id, &kind, 0).map(|_| ()),
            other => self.update(|s, events| {
                let c = s.cases.get_mut(id).context("unknown case")?;
                ensure!(!c.cancelled, "case cancelled");
                match other {
                    Intent::SelectTemplate { template } => ensure!(
                        template == c.template && !template.is_empty(),
                        "template not applicable"
                    ),
                    Intent::ProposePatch { node, dependency } => {
                        ensure!(c.replans < 2, "replan budget exceeded");
                        ensure!(c.budget_remaining > 0, "budget exhausted");
                        ensure!(c.actions.is_empty(), "cannot rewrite dispatched plan");
                        ensure!(
                            c.nodes.contains_key(&node)
                                && c.nodes.contains_key(&dependency)
                                && node != dependency,
                            "invalid node"
                        );
                        let n = c.nodes.get_mut(&node).unwrap();
                        ensure!(!n.deps.contains(&dependency), "duplicate dependency");
                        n.deps.push(dependency);
                        validate_dag(&c.nodes)?;
                        c.plan_revision += 1;
                        c.replans += 1;
                        c.approvals.clear();
                        c.proposal = None;
                        for n in c.nodes.values_mut() {
                            n.status = "READY".into();
                            n.started_at = None;
                            n.finished_at = None;
                            n.due_at = None;
                            n.output = None;
                        }
                        for ch in &mut c.children {
                            if ch.status == "RUNNING" {
                                ch.status = "SUPERSEDED".into();
                            }
                        }
                        c.stage = "ASSESS".into();
                        pump(c, s.now, events)?;
                    }
                    Intent::RequestDecision { scope } => {
                        ensure!(
                            matches!(
                                scope.as_str(),
                                "production" | "initial_response" | "eight_d"
                            ),
                            "unknown scope"
                        );
                        emit(c, "DecisionRequested", json!({"scope":scope}), events);
                    }
                    Intent::SubmitSummary { evidence_refs } => {
                        ensure!(
                            !evidence_refs.is_empty()
                                && evidence_refs.iter().all(|r| c.evidence.contains_key(r)),
                            "unknown evidence"
                        );
                        emit(
                            c,
                            "SummarySubmitted",
                            json!({"evidence_refs":evidence_refs,"provenance":"mock"}),
                            events,
                        );
                    }
                    Intent::Wait => {}
                    Intent::RequestDelegation { .. } => unreachable!(),
                }
                Ok(())
            }),
        }
    }
    /// Run ready scripted investigation workers; no shell or recursive agents.
    pub fn run_children(&mut self) -> Result<()> {
        self.update(|s, events| {
            for c in s.cases.values_mut() {
                let tokens: Vec<_> = c
                    .children
                    .iter()
                    .filter(|ch| {
                        ch.status == "RUNNING"
                            && ch.due_at <= s.now
                            && s.now < ch.lease_until
                            && c.world["child_timeout"] != true
                    })
                    .map(|ch| ch.token)
                    .collect();
                for token in tokens {
                    let refs = c.evidence.keys().take(4).cloned().collect();
                    submit_child(c, token, refs, s.now, events)?;
                }
            }
            Ok(())
        })
    }
    pub fn verify_complaint(&mut self, id: &str) -> Result<()> {
        self.update(|s,events|{let c=s.cases.get_mut(id).context("unknown case")?;
            ensure!(c.scenario=="s2"&&!c.cancelled&&c.eight_d.is_some(),"complaint report unavailable");
            ensure!(c.world["root_cause_verified"]==true&&c.world["effectiveness_verified"]==true,"physical root cause and effectiveness evidence missing");
            ensure!(c.world["missing_genealogy"]!=true&&c.world["conflicting_records"]!=true,"unresolved source conflict");
            let evidence=format!("mock-field-verification:{}",c.id);
            c.evidence.insert(evidence.clone(),json!({"source":"QMS/mock-field-verification","root_cause_verified":true,"effectiveness_verified":true,"provenance":"mock","mock_ids":["M12"],"observed_at":s.now}));
            c.plan_revision+=1;c.approvals.clear();c.resource_observed_at=s.now;
            let steps:Vec<_>=(1..=8).map(|i|json!({"step":format!("D{i}"),"status":"VERIFIED_IN_MOCK","evidence_refs":[evidence]})).collect();
            c.eight_d=Some(json!({"kind":"FORMAL_8D","formal_complete":true,"steps":steps,"provenance":"mock","limitations":["Simulated field evidence only"]}));
            emit(c,"ComplaintVerificationRecorded",json!({"evidence_ref":evidence}),events);Ok(())})
    }
    pub fn reserve_model(&mut self, id: &str) -> Result<u64> {
        self.update(|s, events| {
            let c = s.cases.get_mut(id).context("unknown case")?;
            ensure!(
                !c.cancelled && c.model_calls < 24,
                "model budget exhausted or cancelled"
            );
            charge(c, 1)?;
            c.model_calls += 1;
            emit(c, "ModelCallReserved", json!({"mock_ids":["M23"]}), events);
            Ok(c.revision)
        })
    }
    pub fn apply_master_result(&mut self, id: &str, revision: u64, intent: Intent) -> Result<()> {
        ensure!(
            self.case(id)?.revision == revision,
            "facts changed during model call; discard stale proposal"
        );
        self.apply_intent(id, intent)
    }
    pub fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<BoardEvent>> {
        self.case(id)?;
        ensure!((1..=1000).contains(&limit), "invalid page size");
        let mut stmt = self.db.prepare(
            "SELECT body FROM mom_events WHERE case_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![id, after, limit], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn pending_events(&self, after: u64, limit: usize) -> Result<Vec<BoardEvent>> {
        ensure!((1..=1000).contains(&limit), "invalid page size");
        let mut stmt = self.db.prepare(
            "SELECT body FROM mom_events WHERE seq>?1 AND published=0 ORDER BY seq LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after, limit], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn mark_published(&mut self, through: u64) -> Result<()> {
        ensure!(through <= self.sequence, "future sequence");
        self.db
            .execute("UPDATE mom_events SET published=1 WHERE seq<=?1", [through])?;
        Ok(())
    }
    pub fn acknowledge(&mut self, consumer: &str, id: &str, seq: u64) -> Result<()> {
        self.case(id)?;
        ensure!(
            seq <= self.sequence && !consumer.is_empty(),
            "invalid cursor"
        );
        self.db.execute("INSERT INTO mom_cursors(consumer,case_id,seq) VALUES(?1,?2,?3) ON CONFLICT(consumer,case_id) DO UPDATE SET seq=MAX(seq,excluded.seq)",params![consumer,id,seq])?;
        Ok(())
    }
    pub fn cursor(&self, consumer: &str, id: &str) -> Result<u64> {
        Ok(self
            .db
            .query_row(
                "SELECT seq FROM mom_cursors WHERE consumer=?1 AND case_id=?2",
                params![consumer, id],
                |r| r.get(0),
            )
            .unwrap_or(0))
    }
    pub fn agent_view(&self, id: &str, after: u64, limit: usize) -> Result<Value> {
        ensure!((1..=999).contains(&limit), "invalid page size");
        let c = self.case(id)?;
        let mut ev = self.events(id, after, limit + 1)?;
        let truncated = ev.len() > limit;
        if truncated {
            ev.truncate(limit);
        }
        let next = ev.last().map_or(after, |v| v.sequence);
        Ok(
            json!({"case_id":id,"case_revision":c.revision,"plan_revision":c.plan_revision,"projection_seq":self.sequence,"observed_at":self.state.now,"freshness":"current","constraints":c.constraints,"stage":c.stage,"template":c.template,"nodes":c.nodes,"evidence_index":c.evidence.keys().collect::<Vec<_>>(),"proposal":c.proposal,"decisions":c.approvals,"budget_remaining":c.budget_remaining,"children":c.children,"events":ev,"truncated":truncated,"next_cursor":next,"provenance":"mock","mock_ids":c.mock_ids}),
        )
    }
    pub fn human_view(&self, id: &str) -> Result<Value> {
        let c = self.case(id)?;
        Ok(
            json!({"case_id":id,"case_revision":c.revision,"plan_revision":c.plan_revision,"projection_seq":self.sequence,"observed_at":self.state.now,"freshness":"current","stage":c.stage,"scenario":c.scenario,"customer":c.customer,"batch":c.batch,"constraints":c.constraints,"dag":c.nodes,"proposal":c.proposal,"initial_report":c.initial_report,"eight_d":c.eight_d,"decision_inbox":[{"scope":if c.scenario=="s1"{"production"}else{"initial_response"},"plan_revision":c.plan_revision,"approved":c.approvals}],"actions":c.actions,"children":c.children,"deadlines":c.deadlines,"alerts":c.alerts,"breaches":c.breaches,"evidence":c.evidence,"outbox":c.outbox,"budget":{"remaining":c.budget_remaining,"tool_calls":c.tool_calls,"model_calls":c.model_calls},"provenance":"mock","mock_ids":c.mock_ids,"limitations":["Scripted Master and fixture tools; no production writes","8D root cause remains unverified until physical evidence exists"]}),
        )
    }
}
fn emit(c: &mut Case, kind: &str, payload: Value, events: &mut Emissions) {
    c.revision += 1;
    events.push((c.id.clone(), kind.into(), payload, c.updated_at, c.revision));
}
fn charge(c: &mut Case, n: u32) -> Result<()> {
    ensure!(c.budget_remaining >= n, "budget exhausted");
    c.budget_remaining -= n;
    Ok(())
}
fn charge_tools(c: &mut Case, n: u32) -> Result<()> {
    ensure!(c.tool_calls + n <= 40, "tool call budget exhausted");
    charge(c, n)?;
    c.tool_calls += n;
    Ok(())
}
fn new_case(id: &str, e: &EventInput, received_at: u64) -> Case {
    let deadlines = if e.scenario == "s2" {
        BTreeMap::from([
            ("initial".into(), received_at + 240),
            ("eight_d".into(), received_at + 1440),
        ])
    } else {
        BTreeMap::new()
    };
    Case {
        id: id.into(),
        scenario: e.scenario.clone(),
        batch: e.batch.clone(),
        customer: e.customer.clone(),
        asset: e.asset.clone(),
        updated_at: received_at,
        revision: 0,
        plan_revision: 1,
        stage: "COLLECTING".into(),
        template: String::new(),
        constraints: vec![
            "Never bypass safety interlock or quality release".into(),
            "Approval must match current plan and resource conditions".into(),
            "Mock evidence is not real industrial validation".into(),
        ],
        observations: vec![],
        nodes: BTreeMap::new(),
        evidence: BTreeMap::new(),
        proposal: None,
        initial_report: None,
        eight_d: None,
        approvals: BTreeMap::new(),
        actions: BTreeMap::new(),
        children: vec![],
        outbox: vec![],
        world: fixture_world(&e.scenario),
        resource_version: 1,
        resource_observed_at: e.occurred_at,
        budget_remaining: 40,
        tool_calls: 0,
        model_calls: 0,
        replans: 0,
        deadlines,
        alerts: vec![],
        breaches: vec![],
        mock_ids: fixture_policy(&e.scenario).mock_ids,
        cancelled: false,
    }
}
fn instantiate(c: &mut Case, events: &mut Emissions) -> Result<()> {
    if c.stage == "BLOCKED" {
        return Ok(());
    }
    if c.observations.iter().any(|o| {
        !matches!(
            o.event.kind.as_str(),
            "order" | "vibration" | "quality" | "complaint"
        )
    }) {
        c.stage = "BLOCKED".into();
        emit(
            c,
            "TemplateRejected",
            json!({"reason":"unsupported feature signature"}),
            events,
        );
        return Ok(());
    }
    if !c.nodes.is_empty() {
        return Ok(());
    }
    if c.scenario == "s1" {
        let kinds: BTreeSet<_> = c
            .observations
            .iter()
            .map(|o| o.event.kind.as_str())
            .collect();
        if !["order", "vibration", "quality"]
            .iter()
            .all(|k| kinds.contains(k))
        {
            return Ok(());
        }
        c.template = "DAG-TPL-COMPOSITE-MAINT-QUAL-RESCHEDULE@1".into();
        c.nodes = BTreeMap::from([
            ("maintenance".into(), Node::new("maintenance.assess", &[])),
            ("quality".into(), Node::new("quality.assess", &[])),
            ("resources".into(), Node::new("resources.check", &[])),
            (
                "schedule".into(),
                Node::new(
                    "schedule.simulate",
                    &["maintenance", "quality", "resources"],
                ),
            ),
        ]);
    } else {
        c.template = "DAG-TPL-CUSTOMER-COMPLAINT-8D@1".into();
        c.nodes = BTreeMap::from([
            ("trace".into(), Node::new("genealogy.trace", &[])),
            ("quality".into(), Node::new("quality.assess", &[])),
            (
                "production".into(),
                Node::new("production.reconstruct", &[]),
            ),
            ("history".into(), Node::new("equipment.history", &[])),
        ]);
    }
    validate_dag(&c.nodes)?;
    c.stage = "ASSESS".into();
    emit(
        c,
        "PlanAccepted",
        json!({"template":c.template,"plan_revision":c.plan_revision}),
        events,
    );
    Ok(())
}
pub fn validate_dag(nodes: &BTreeMap<String, Node>) -> Result<()> {
    ensure!(nodes.len() <= 20, "too many nodes");
    let mut done = BTreeSet::new();
    for node in nodes.values() {
        ensure!(tools::known(&node.tool), "unknown tool");
        ensure!(
            node.deps.iter().all(|d| nodes.contains_key(d)),
            "missing dependency"
        );
    }
    loop {
        let before = done.len();
        for (id, n) in nodes {
            if n.deps.iter().all(|d| done.contains(d)) {
                done.insert(id.clone());
            }
        }
        if done.len() == nodes.len() {
            return Ok(());
        }
        ensure!(done.len() > before, "cyclic DAG");
    }
}
fn pump(c: &mut Case, now: u64, events: &mut Emissions) -> Result<()> {
    if c.cancelled || matches!(c.stage.as_str(), "BLOCKED" | "CLOSED" | "BUDGET_EXHAUSTED") {
        return Ok(());
    }
    let running = c.nodes.values().filter(|n| n.status == "RUNNING").count();
    let ready: Vec<_> = c
        .nodes
        .iter()
        .filter(|(_, n)| {
            n.status == "READY"
                && n.deps
                    .iter()
                    .all(|d| matches!(c.nodes[d].status.as_str(), "SUCCEEDED" | "UNKNOWN"))
        })
        .map(|(id, _)| id.clone())
        .take(
            fixture_policy(&c.scenario)
                .max_parallel
                .saturating_sub(running),
        )
        .collect();
    for id in ready {
        if charge_tools(c, 1).is_err() {
            c.stage = "BUDGET_EXHAUSTED".into();
            emit(c, "BudgetExhausted", json!({}), events);
            break;
        }
        let n = c.nodes.get_mut(&id).unwrap();
        n.status = "RUNNING".into();
        n.started_at = Some(now);
        n.due_at = Some(now + fixture_policy(&c.scenario).latency_minutes);
        emit(c, "ToolStarted", json!({"node":id}), events);
    }
    Ok(())
}
fn tick(c: &mut Case, now: u64, events: &mut Emissions) -> Result<()> {
    c.updated_at = now;
    // Reconcile already accepted effects even when parent work was cancelled.
    let mut verified = vec![];
    for (key, a) in &mut c.actions {
        if a.complete_at <= now && matches!(a.state.as_str(), "ACCEPTED" | "UNKNOWN") {
            a.state = "VERIFIED".into();
            a.verification = Some(format!("mock-verification:{key}"));
            verified.push(key.clone());
        }
    }
    for key in verified {
        emit(
            c,
            "ActionVerified",
            json!({"key":key,"provenance":"mock"}),
            events,
        );
    }
    if c.cancelled || matches!(c.stage.as_str(), "CLOSED" | "BLOCKED") {
        return Ok(());
    }
    let due: Vec<_> = c
        .nodes
        .iter()
        .filter(|(_, n)| n.status == "RUNNING" && n.due_at.is_some_and(|t| t <= now))
        .map(|(id, n)| (id.clone(), n.tool.clone()))
        .collect();
    for (id, tool) in due {
        let mut response = tools::read(c, &tool, now)?;
        let eid = format!("tool:{}:{}:{}", c.id, c.plan_revision, id);
        response.evidence_refs = vec![eid.clone()];
        c.evidence
            .insert(eid.clone(), serde_json::to_value(&response)?);
        let n = c.nodes.get_mut(&id).unwrap();
        n.status = if response.status == "ok" {
            "SUCCEEDED"
        } else {
            "UNKNOWN"
        }
        .into();
        n.finished_at = Some(now);
        n.output = Some(response.data.clone());
        n.evidence_refs = vec![eid.clone()];
        if id == "resources" {
            c.resource_observed_at = now;
        }
        if id == "quality" && c.scenario == "s1" {
            c.world["held"][&c.batch] = json!("pending validation");
            emit(
                c,
                "ContainmentApplied",
                json!({"scope":c.batch,"authorization":"fixture preauthorized quality hold","provenance":"mock","mock_ids":["M07","M20"]}),
                events,
            );
        }
        if id == "trace" && response.status == "ok" {
            c.world["held"] = c.world["inventory"].clone();
            emit(
                c,
                "ContainmentApplied",
                json!({"scope":c.world["held"],"authorization":"fixture preauthorized inventory hold","provenance":"mock","mock_ids":["M07","M20"]}),
                events,
            );
        }
        emit(
            c,
            "EvidenceRecorded",
            json!({"node":id,"evidence_ref":eid,"status":response.status}),
            events,
        );
        if id == "schedule" {
            c.proposal = Some(response.data);
            c.resource_observed_at = now;
            c.stage = if c.proposal.as_ref().is_some_and(|p| p["feasible"] == true) {
                "WAITING_HUMAN"
            } else {
                "BLOCKED"
            }
            .into();
            emit(
                c,
                "DecisionRequested",
                json!({"scope":"production"}),
                events,
            );
        }
    }
    pump(c, now, events)?;
    if c.scenario == "s2"
        && c.initial_report.is_none()
        && c.nodes
            .get("trace")
            .is_some_and(|n| matches!(n.status.as_str(), "SUCCEEDED" | "UNKNOWN"))
        && c.nodes
            .get("quality")
            .is_some_and(|n| matches!(n.status.as_str(), "SUCCEEDED" | "UNKNOWN"))
    {
        let refs: Vec<_> = c.evidence.keys().cloned().collect();
        c.initial_report = Some(
            json!({"kind":"INITIAL_RESPONSE","root_cause_status":"UNKNOWN","held":c.world["held"],"delivered_risk":c.world["delivered"],"containment_complete":false,"next_update_at":c.deadlines["eight_d"],"evidence_refs":refs,"provenance":"mock","mock_ids":c.mock_ids}),
        );
        let steps:Vec<_>=(1..=8).map(|i|json!({"step":format!("D{i}"),"owner":if i<4 {"quality"}else{"investigation"},"status":if i<=3 {"DOCUMENTED"}else{"UNVERIFIED"},"evidence_refs":if i<=3 {refs.clone()}else{vec![]}})).collect();
        c.eight_d = Some(
            json!({"kind":"PROGRESS_8D","formal_complete":false,"steps":steps,"limitations":["D4 root cause and D6 effectiveness not verified"],"provenance":"mock"}),
        );
        c.stage = "WAITING_HUMAN".into();
        c.resource_observed_at = now;
        emit(
            c,
            "DecisionRequested",
            json!({"scope":"initial_response"}),
            events,
        );
    }
    let children: Vec<_> = c
        .children
        .iter()
        .filter(|ch| ch.status == "RUNNING")
        .map(|ch| (ch.token, ch.lease_until, ch.due_at))
        .collect();
    for (token, lease, due) in children {
        if now >= lease {
            c.children
                .iter_mut()
                .find(|ch| ch.token == token)
                .unwrap()
                .status = "TIMED_OUT".into();
            emit(
                c,
                "ChildTimedOut",
                json!({"token":token,"limitations":["Investigation incomplete"]}),
                events,
            );
        } else {
            let _ = due;
        }
    }
    if c.scenario == "s2" {
        for (kind, deadline) in c.deadlines.clone() {
            let satisfied = if kind == "initial" {
                c.outbox.iter().any(|r| {
                    r["report"]["kind"] == "INITIAL_RESPONSE"
                        && r["sent_at"].as_u64().is_some_and(|t| t <= deadline)
                })
            } else {
                c.outbox.iter().any(|r| {
                    r["report"]["formal_complete"] == true
                        && r["sent_at"].as_u64().is_some_and(|t| t <= deadline)
                })
            };
            if satisfied {
                continue;
            }
            let thresholds = if kind == "initial" {
                vec![60, 15]
            } else {
                vec![240, 60]
            };
            for threshold in thresholds {
                let key = format!("{kind}:{threshold}");
                if now >= deadline.saturating_sub(threshold) && !c.alerts.contains(&key) {
                    c.alerts.push(key.clone());
                    emit(c, "SlaWarning", json!({"key":key}), events);
                }
            }
            if now > deadline && !c.breaches.contains(&kind) {
                c.breaches.push(kind.clone());
                emit(c, "SlaBreached", json!({"kind":kind}), events);
            }
        }
    }
    Ok(())
}
fn submit_child(
    c: &mut Case,
    token: u64,
    refs: Vec<String>,
    now: u64,
    events: &mut Emissions,
) -> Result<()> {
    ensure!(!c.cancelled, "parent cancelled");
    ensure!(
        !refs.is_empty() && refs.iter().all(|r| c.evidence.contains_key(r)),
        "child must cite existing evidence"
    );
    let ch = c
        .children
        .iter_mut()
        .find(|ch| ch.token == token)
        .context("unknown child token")?;
    ensure!(
        ch.status == "RUNNING" && now < ch.lease_until && ch.input_revision == c.plan_revision,
        "stale child lease or plan"
    );
    ch.status = "COMPLETED".into();
    ch.report = Some(
        json!({"findings":[{"status":"HYPOTHESIS","claim":"Equipment history may relate to quality trend; causal verification required"}],"evidence_refs":refs,"recommended_checks":["physical inspection and controlled measurement"],"limitations":["Scripted investigation; no causal proof"],"provenance":"mock","mock_ids":["M19"]}),
    );
    emit(
        c,
        "ChildReportSubmitted",
        json!({"token":token,"evidence_refs":refs}),
        events,
    );
    Ok(())
}
