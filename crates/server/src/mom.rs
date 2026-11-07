//! Local-only MOM simulation service. Test identities, never enterprise auth.
use an_runtime::mom::{
    gap_register, Engine, EventInput, Intent, Master, ScriptedMaster, SimulationConfig,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Mutex<Engine>>,
    pub master: Arc<dyn Master>,
    slots: Arc<Semaphore>,
    active: Arc<Mutex<BTreeSet<String>>>,
}
impl AppState {
    pub fn scripted(engine: Arc<Mutex<Engine>>) -> Self {
        Self {
            engine,
            master: Arc::new(ScriptedMaster),
            slots: Arc::new(Semaphore::new(4)),
            active: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
}
type ApiResult<T> = Result<Json<T>, (StatusCode, Json<Value>)>;
fn err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":e.to_string()})),
    )
}
fn run<T>(state: &AppState, f: impl FnOnce(&mut Engine) -> anyhow::Result<T>) -> ApiResult<T> {
    let mut e = state
        .engine
        .lock()
        .map_err(|_| err("runtime unavailable"))?;
    f(&mut e).map(Json).map_err(err)
}
pub fn page() -> Html<&'static str> {
    Html(include_str!("../../../web/mom/index.html"))
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { page() }))
        .route("/api/mom/cases", get(list))
        .route(
            "/api/mom/simulations",
            get(simulations).post(create_simulation),
        )
        .route("/api/mom/playback", post(playback))
        .route("/api/mom/gaps", get(|| async { Json(gap_register()) }))
        .route("/api/mom/scenarios/{scenario}", post(scenario))
        .route("/api/mom/events", post(ingest))
        .route("/api/mom/cases/{id}", get(human))
        .route("/api/mom/cases/{id}/agent", get(agent))
        .route("/api/mom/cases/{id}/events", get(events))
        .route("/api/mom/cases/{id}/commands", post(command))
        .route("/api/mom/cases/{id}/master", post(master))
        .layer(axum::extract::DefaultBodyLimit::max(128 * 1024))
        .with_state(state)
}
/// One bounded shared simulation clock per service. Shutdown owner aborts this task.
pub fn spawn_clock(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let Ok(mut engine) = state.engine.lock() else {
                break;
            };
            if let Err(error) = engine.playback_tick() {
                let _ = engine.set_playback(false, 0);
                tracing::warn!(event="task.status",task_id="mom-clock",error=%error,"MOM playback paused after error");
            }
        }
    })
}
async fn simulations(State(s): State<AppState>) -> ApiResult<Value> {
    run(&s, |e| Ok(e.simulations_view()))
}
async fn create_simulation(
    State(s): State<AppState>,
    Json(config): Json<SimulationConfig>,
) -> ApiResult<Value> {
    run(&s, |e| {
        Ok(json!({"case_id":e.create_simulation(config)?,"provenance":"mock"}))
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaybackRequest {
    playing: bool,
    #[serde(default = "playback_default")]
    minutes: u64,
}
fn playback_default() -> u64 {
    30
}
async fn playback(State(s): State<AppState>, Json(p): Json<PlaybackRequest>) -> ApiResult<Value> {
    run(&s, |e| {
        e.set_playback(p.playing, p.minutes)?;
        Ok(e.simulations_view())
    })
}
async fn list(State(s): State<AppState>) -> ApiResult<Value> {
    run(&s, |e| {
        let cases = e
            .case_ids()
            .iter()
            .map(|id| e.human_view(id))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(json!({"now":e.now(),"cases":cases,"mode":"simulation"}))
    })
}
async fn human(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    run(&s, |e| e.human_view(&id))
}
#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    20
}
async fn agent(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<Page>,
) -> ApiResult<Value> {
    run(&s, |e| e.agent_view(&id, q.after, q.limit))
}
async fn events(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<Page>,
) -> ApiResult<Value> {
    run(&s, |e| {
        Ok(serde_json::to_value(e.events(&id, q.after, q.limit)?)?)
    })
}
async fn ingest(State(s): State<AppState>, Json(event): Json<EventInput>) -> ApiResult<Value> {
    run(&s, |e| Ok(json!({"case_id":e.ingest(event)?})))
}
async fn scenario(State(s): State<AppState>, Path(kind): Path<String>) -> ApiResult<Value> {
    let result = run(&s, |e| {
        anyhow::ensure!(kind == "s1" || kind == "s2", "unknown scenario");
        let suffix = an_core::new_id("fixture");
        let base = e.now().max(if kind == "s1" { 497 } else { 540 });
        let id = if kind == "s1" {
            let mut first = EventInput::s1("order", base);
            first.id = format!("{suffix}-order");
            first.batch = suffix.clone();
            let id = e.ingest(first)?;
            for (k, delta) in [("vibration", 6), ("quality", 14)] {
                e.advance(base + delta)?;
                let mut x = EventInput::s1(k, base + delta);
                x.id = format!("{suffix}-{k}");
                x.batch = suffix.clone();
                e.ingest(x)?;
            }
            e.advance(base + 17)?;
            id
        } else {
            let mut x = EventInput::complaint(&suffix, base);
            x.batch = suffix;
            let id = e.ingest(x)?;
            e.advance(base + 4)?;
            id
        };
        Ok(json!({"case_id":id,"provenance":"mock"}))
    })?;
    let id = result.0["case_id"].as_str().unwrap().to_string();
    // An independent investigation is admitted once; the initial reply never waits for it.
    let _ = master(State(s.clone()), Path(id)).await?;
    Ok(result)
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Advance {
        minutes: u64,
    },
    Approve {
        scope: String,
        plan_revision: u64,
        actor: String,
    },
    Execute {
        scope: String,
    },
    Delegate {
        kind: String,
    },
    Cancel,
    Close,
    VerifyComplaint,
    RefreshPlan,
    Fault {
        key: String,
        value: Value,
    },
    Patch {
        node: String,
        dependency: String,
    },
}
async fn command(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(cmd): Json<Command>,
) -> ApiResult<Value> {
    run(&s, |e| {
        e.case(&id)?;
        match cmd {
            Command::Advance { minutes } => {
                anyhow::ensure!(minutes <= 2880, "advance limit 48h");
                let end = e.now() + minutes;
                for time in e.now()..=end {
                    e.advance(time)?;
                    e.run_children()?;
                }
            }
            Command::Approve {
                scope,
                plan_revision,
                actor,
            } => e.approve(&id, plan_revision, &scope, &actor)?,
            Command::Execute { scope } => e.execute(&id, &scope)?,
            Command::Delegate { kind } => {
                e.delegate(&id, &kind, 0)?;
            }
            Command::Cancel => e.cancel(&id)?,
            Command::Close => e.close(&id)?,
            Command::VerifyComplaint => e.verify_complaint(&id)?,
            Command::RefreshPlan => e.refresh_plan(&id)?,
            Command::Fault { key, value } => e.change_world(&id, &key, value)?,
            Command::Patch { node, dependency } => {
                e.apply_intent(&id, Intent::ProposePatch { node, dependency })?
            }
        }
        e.human_view(&id)
    })
}
struct ModelFlight {
    active: Arc<Mutex<BTreeSet<String>>>,
    id: String,
    _slot: OwnedSemaphorePermit,
}
impl Drop for ModelFlight {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}
async fn master(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let slot = s
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| err("global Master concurrency limit reached"))?;
    {
        let mut active = s.active.lock().map_err(|_| err("runtime unavailable"))?;
        if !active.insert(id.clone()) {
            return Err(err("Master already running for this case"));
        }
    }
    let _flight = ModelFlight {
        active: s.active.clone(),
        id: id.clone(),
        _slot: slot,
    };
    let (case, view, revision) = {
        let mut e = s.engine.lock().map_err(|_| err("runtime unavailable"))?;
        let revision = e.reserve_model(&id).map_err(err)?;
        let c = e.case(&id).map_err(err)?.clone();
        let v = e.agent_view(&id, 0, 20).map_err(err)?;
        (c, v, revision)
    };
    let intent = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        s.master.decide(&case, view),
    )
    .await
    .map_err(|_| err("Master timed out; budget reservation retained"))?
    .map_err(err)?;
    run(&s, |e| {
        e.apply_master_result(&id, revision, intent)?;
        e.human_view(&id)
    })
}
