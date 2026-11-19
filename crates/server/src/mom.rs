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
    /// E1-C：配置后，S1 创建前从 RelOS 取回关系上下文（见 `mom_relos`）；未配置时完全使用 Mock
    pub relos: Option<Arc<crate::mom_relos::RelosClient>>,
    /// 车间 DES 审核层的 AI 审核（阶段④）；未配置时 AI 审核不可用，只做规则检查。
    pub reviewer: Option<Arc<dyn an_runtime::mom::des::review::Reviewer>>,
    /// 阶段⑤a-3：模拟工厂连接器（`an-mom --des-mcp`）。配置后，创建仿真、推进时钟、发写操作都经它进行，
    /// 不回退到进程内仿真；未配置时使用进程内仿真（行为与之前相同）。
    pub des: Option<Arc<crate::mom_des_connector::DesConnector>>,
}
impl AppState {
    pub fn scripted(engine: Arc<Mutex<Engine>>) -> Self {
        Self {
            engine,
            master: Arc::new(ScriptedMaster),
            slots: Arc::new(Semaphore::new(4)),
            active: Arc::new(Mutex::new(BTreeSet::new())),
            relos: None,
            reviewer: None,
            des: None,
        }
    }
    /// 接入模拟工厂连接器（阶段⑤a-3）。
    pub fn with_des(mut self, connector: Arc<crate::mom_des_connector::DesConnector>) -> Self {
        self.des = Some(connector);
        self
    }
    /// 配置车间 DES 审核层的 AI 审核（阶段④）。
    pub fn with_reviewer(
        mut self,
        reviewer: Arc<dyn an_runtime::mom::des::review::Reviewer>,
    ) -> Self {
        self.reviewer = Some(reviewer);
        self
    }
    /// 启用 RelOS 上下文（`base_url` 例如 `http://127.0.0.1:8000`）。
    pub fn with_relos(mut self, base_url: &str) -> anyhow::Result<Self> {
        self.relos = Some(Arc::new(crate::mom_relos::RelosClient::new(base_url)?));
        Ok(self)
    }
}
type ApiResult<T> = Result<Json<T>, (StatusCode, Json<Value>)>;
fn err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":e.to_string()})),
    )
}
/// 接了模拟工厂时的错误（阶段⑤a-3）：模拟工厂不在线（`E_DES_UNAVAILABLE`）返回 503 并带 `code`，
/// 其他（参数错误、模拟工厂拒绝等）返回 400。
pub(crate) fn des_error(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = format!("{e:#}");
    if message.contains(crate::mom_des_connector::E_DES_UNAVAILABLE) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": message, "code": crate::mom_des_connector::E_DES_UNAVAILABLE})),
        )
    } else {
        err(message)
    }
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
        // 车间 DES 仿真的只读接口（阶段②），见 `mom_sim`。
        .merge(crate::mom_sim::routes())
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
            if let Err(error) = clock_step(&state).await {
                if error.to_string().contains("runtime unavailable") {
                    break;
                }
                tracing::warn!(event="task.status",task_id="mom-clock",error=%error,"MOM playback paused after error");
            }
        }
    })
}

fn lock_engine(state: &AppState) -> anyhow::Result<std::sync::MutexGuard<'_, Engine>> {
    state
        .engine
        .lock()
        .map_err(|_| anyhow::anyhow!("runtime unavailable"))
}

/// 共享时钟走一步（后台时钟每秒调用一次；测试直接调用）。出错时暂停播放并返回错误。
///
/// 接了模拟工厂时（阶段⑤a-3）：先补发待发写操作；播放中时经连接器推进一分钟（模拟工厂与引擎同步），
/// 再运行子任务、扣减播放剩余分钟。等待模拟工厂时不持有引擎锁；每次调用有连接器的 30 秒超时兜底。
pub async fn clock_step(state: &AppState) -> anyhow::Result<()> {
    let Some(des) = state.des.clone() else {
        let mut engine = lock_engine(state)?;
        if let Err(error) = engine.playback_tick() {
            let _ = engine.set_playback(false, 0);
            return Err(error);
        }
        return Ok(());
    };
    let result: anyhow::Result<()> = async {
        des.dispatch_writes(&state.engine).await?;
        let next = {
            let engine = lock_engine(state)?;
            if !engine.playback_playing() {
                return Ok(());
            }
            engine.now() + 1
        };
        des.tick(&state.engine, next).await?;
        lock_engine(state)?.playback_finish_step()?;
        des.dispatch_writes(&state.engine).await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        if let Ok(mut engine) = lock_engine(state) {
            let _ = engine.set_playback(false, 0);
        }
        return Err(error);
    }
    Ok(())
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
    let suffix = an_core::new_id("fixture");
    // RelOS 上下文在加锁前异步取回（5 秒超时），失败时得到带原因的 status，不阻断案例创建
    let relos_context = match (&s.relos, kind.as_str()) {
        (Some(client), "s1") => Some(
            client
                .fetch(&crate::mom_relos::RelosQuery {
                    asset: "CNC-XX",
                    line: "Line-B",
                    batch: &suffix,
                })
                .await,
        ),
        _ => None,
    };
    let result = run(&s, |e| {
        anyhow::ensure!(kind == "s1" || kind == "s2", "unknown scenario");
        let base = e.now().max(if kind == "s1" { 497 } else { 540 });
        let id = if kind == "s1" {
            let mut first = EventInput::s1("order", base);
            first.id = format!("{suffix}-order");
            first.batch = suffix.clone();
            let id = e.ingest(first)?;
            if let Some(context) = relos_context {
                // 作为观测写入，引擎记录 WorldChanged 事件；诊断工具只读这条记录
                e.change_world(&id, "relos", context)?;
            }
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
    /// 车间 DES 案例：选定排产方案（阶段③），之后需要重新批准。
    SelectPlan {
        plan: String,
    },
    /// 车间 DES 案例：记录根因假设（阶段④），供审核规则比对。
    ClaimCause {
        causes: Vec<String>,
        actor: String,
    },
}
async fn command(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(cmd): Json<Command>,
) -> ApiResult<Value> {
    let Some(des) = s.des.clone() else {
        return run(&s, |e| apply_command(e, &id, cmd));
    };
    // 接了模拟工厂（阶段⑤a-3）：推进经连接器逐分钟进行，每分钟之后照旧运行子任务并发出写操作。
    if let Command::Advance { minutes } = cmd {
        let (start, end) = run(&s, |e| {
            e.case(&id)?;
            anyhow::ensure!(minutes <= 2880, "advance limit 48h");
            Ok((e.now(), e.now() + minutes))
        })?
        .0;
        for time in start..=end {
            des.tick(&s.engine, time).await.map_err(des_error)?;
            let _ = run(&s, |e| e.run_children())?;
            des.dispatch_writes(&s.engine).await.map_err(des_error)?;
        }
        return run(&s, |e| e.human_view(&id));
    }
    // 其他命令照常在引擎中生效；之后立即发出审批执行产生的写操作，返回的案例已含回执。
    let _ = run(&s, |e| apply_command(e, &id, cmd))?;
    if let Err(e) = des.dispatch_writes(&s.engine).await {
        // 命令已生效，写操作留在待发清单中，下一次命令或时钟步会重发（幂等键保证只生效一次）。
        let (status, Json(mut body)) = des_error(e);
        body["applied"] = json!(true);
        return Err((status, Json(body)));
    }
    run(&s, |e| e.human_view(&id))
}

/// 在引擎中执行一条案例命令并返回案例的人类视图（进程内模式下 `advance` 也在这里）。
fn apply_command(e: &mut Engine, id: &str, cmd: Command) -> anyhow::Result<Value> {
    let id = id.to_string();
    {
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
            Command::SelectPlan { plan } => e.select_des_plan(&id, &plan)?,
            Command::ClaimCause { causes, actor } => e.record_cause_claim(&id, causes, &actor)?,
            Command::Fault { key, value } => e.change_world(&id, &key, value)?,
            Command::Patch { node, dependency } => {
                e.apply_intent(&id, Intent::ProposePatch { node, dependency })?
            }
        }
        e.human_view(&id)
    }
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
