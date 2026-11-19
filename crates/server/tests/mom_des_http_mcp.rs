//! 阶段⑤a-3：an-mom 接入模拟工厂连接器（RT-DESC-06～11）。
//!
//! 用例见 `docs/testing/CASES.md`「阶段⑤a-3：an-mom 接入模拟工厂连接器」，设计见 `docs/mom/DES_CLOSED_LOOP.md`
//! §11a"⑤a-3 设计"。每个等待都有超时，服务未实现或卡住时测试失败而不是挂起。

use an_runtime::mom::Engine;
use an_server::mom::{self, AppState};
use an_server::mom_des_connector::{DesConnector, E_DES_UNAVAILABLE};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// `an-des-mcp` 可执行文件的查找（与 `mom_des_connector.rs` 共用）。
#[path = "support/des_mcp.rs"]
mod des_mcp;
use des_mcp::des_mcp_bin;

const ADMIN: &str = "http-mcp-test-admin";
const WAIT: Duration = Duration::from_secs(60);
/// 6 个观测类工具（开放给桌面与会话）。
const OBSERVE_TOOLS: [&str; 6] = [
    "equipment_status",
    "equipment_trend",
    "events_poll",
    "inventory_tooling",
    "order_status",
    "quality_capability",
];

/// P0 固定剧本仿真的创建请求（与 HTTP-DES-01 相同）。
fn p0_body() -> Value {
    json!({"name":"p0","factory":null,"scenario":{"seed":1,"pattern":{"kind":"script"},"start_minute":497,"horizon_minutes":240,"mechanisms":null}})
}

/// 进程内服务的状态。
fn in_process_state() -> AppState {
    AppState::scripted(Arc::new(Mutex::new(Engine::memory().unwrap())))
}

/// 接了模拟工厂的服务状态，以及连接器本身（测试用它查看已发出的写操作、杀掉模拟工厂进程）。
async fn mcp_state() -> (AppState, Arc<DesConnector>) {
    let connector = Arc::new(
        tokio::time::timeout(WAIT, DesConnector::spawn(&des_mcp_bin(), &[], ADMIN))
            .await
            .expect("spawn timed out")
            .expect("the simulated factory starts"),
    );
    (in_process_state().with_des(connector.clone()), connector)
}

async fn serve(state: AppState) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, mom::router(state)).await.unwrap();
    });
    (url, task)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(WAIT).build().unwrap()
}

async fn post(c: &reqwest::Client, url: String, body: Value) -> (u16, Value) {
    let r = c.post(url).json(&body).send().await.expect("request");
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn get(c: &reqwest::Client, url: String) -> (u16, Value) {
    let r = c.get(url).send().await.expect("request");
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

/// 用 S1 场景取得一个案例（用它发 `advance` 命令推进共享时钟）。
async fn s1_case(c: &reqwest::Client, url: &str) -> String {
    let (status, v) = post(c, format!("{url}/api/mom/scenarios/s1"), json!({})).await;
    assert_eq!(status, 200, "S1 scenario: {v}");
    v["case_id"].as_str().expect("case id").to_string()
}

/// 经 HTTP 创建 P0 仿真，返回（仿真编号，来源）。
async fn create_p0(c: &reqwest::Client, url: &str) -> (String, Value) {
    let (status, v) = post(c, format!("{url}/api/mom/des/simulations"), p0_body()).await;
    assert_eq!(status, 200, "create: {v}");
    (
        v["simulation_id"]
            .as_str()
            .expect("simulation id")
            .to_string(),
        v["source"].clone(),
    )
}

async fn advance(c: &reqwest::Client, url: &str, case: &str, minutes: u64) -> (u16, Value) {
    post(
        c,
        format!("{url}/api/mom/cases/{case}/commands"),
        json!({"command":"advance","minutes":minutes}),
    )
    .await
}

/// DES 案例（批次以仿真编号开头）的可比较状态：批次、阶段、事件种类序列。
fn des_fingerprint(e: &Engine, sim: &str) -> Vec<Value> {
    let prefix = format!("{sim}:");
    e.case_ids()
        .into_iter()
        .filter_map(|id| {
            let c = e.case(&id).ok()?;
            c.batch.starts_with(&prefix).then(|| {
                let kinds: Vec<String> = e
                    .events(&id, 0, 1000)
                    .unwrap()
                    .into_iter()
                    .map(|ev| ev.kind)
                    .collect();
                json!({"id": id, "batch": c.batch, "stage": c.stage, "kinds": kinds})
            })
        })
        .collect()
}

/// 第一个等待人工决策且有推荐方案的 DES 案例。
fn waiting_des_case(e: &Engine, sim: &str) -> Option<(String, String, u64)> {
    let prefix = format!("{sim}:");
    e.case_ids().into_iter().find_map(|id| {
        let c = e.case(&id).ok()?;
        let plan = c.proposal.as_ref()?["recommended"].as_str()?.to_string();
        (c.stage == "WAITING_HUMAN" && c.batch.starts_with(&prefix)).then_some((
            id,
            plan,
            c.plan_revision,
        ))
    })
}

/// 标准答案字段与隐状态名不得出现在返回中。
fn assert_no_truth(label: &str, v: &Value) {
    let text = v.to_string();
    for m in [
        "root_causes",
        "mechanisms",
        "ground_truth",
        "spindle_wear",
        "wear_to_dimension",
    ] {
        assert!(!text.contains(m), "{label} leaks {m}");
    }
}

fn kill(connector: &DesConnector) {
    let pid = connector.child_pid().expect("factory process id");
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .unwrap();
}

/// 每个测试一个临时目录（an-mom 的数据库与日志写在这里）。
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "an-mom-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn rt_desc_06_an_mom_starts_with_the_simulated_factory_or_exits() {
    let exe = env!("CARGO_BIN_EXE_an-mom");
    let help = std::process::Command::new(exe)
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("--des-mcp"),
        "help documents --des-mcp"
    );

    // 命令不存在：启动失败并退出，不回退到进程内仿真。
    let dir = temp_dir("missing");
    let out = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(exe)
            .current_dir(&dir)
            .args([
                "--db",
                "mom.db",
                "--bind",
                "127.0.0.1:0",
                "--des-mcp",
                "/nonexistent/an-des-mcp",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("an-mom must exit when the factory cannot start")
    .unwrap();
    assert!(!out.status.success(), "startup fails");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains(E_DES_UNAVAILABLE), "{text}");

    // 真实启动：读取启动行里的实际监听地址，经 HTTP 创建仿真并读取工具清单。
    let dir = temp_dir("real");
    let mut child = tokio::process::Command::new(exe)
        .current_dir(&dir)
        .args(["--db", "mom.db", "--bind", "127.0.0.1:0", "--des-mcp"])
        .arg(des_mcp_bin())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let address = tokio::time::timeout(WAIT, async {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(rest) = line.split("listening on ").nth(1) {
                return rest
                    .split_whitespace()
                    .next()
                    .map(|a| a.trim_end_matches('.').to_string());
            }
        }
        None
    })
    .await
    .expect("an-mom prints its address")
    .expect("listening line");
    assert!(
        !address.ends_with(":0"),
        "the startup line names the bound port: {address}"
    );
    let url = format!("http://{address}");
    let c = client();
    let (_, source) = create_p0(&c, &url).await;
    assert_eq!(source, "mcp");
    let (status, tools) = get(&c, format!("{url}/api/mom/des/tools")).await;
    assert_eq!(status, 200, "{tools}");
    let names: BTreeSet<&str> = tools["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert_eq!(names, OBSERVE_TOOLS.into_iter().collect());
    child.kill().await.ok();
}

#[tokio::test]
async fn rt_desc_07_http_create_and_advance_match_the_in_process_service() {
    let c = client();
    let local = in_process_state();
    let (remote, _connector) = mcp_state().await;
    let mut runs = vec![];
    for state in [local, remote] {
        let (url, task) = serve(state.clone()).await;
        let case = s1_case(&c, &url).await;
        let (sim, source) = create_p0(&c, &url).await;
        let (status, v) = advance(&c, &url, &case, 120).await;
        assert_eq!(status, 200, "advance: {v}");
        let fp = des_fingerprint(&state.engine.lock().unwrap(), &sim);
        runs.push((sim, source, fp));
        task.abort();
    }
    let (local, remote) = (&runs[0], &runs[1]);
    assert_eq!(local.1, "in_process");
    assert_eq!(remote.1, "mcp");
    assert_eq!(local.0, remote.0, "same simulation id");
    assert!(!local.2.is_empty(), "P0 forms at least one DES case");
    assert_eq!(local.2, remote.2, "same DES cases, stages and events");
}

#[tokio::test]
async fn rt_desc_08_http_execution_sends_writes_before_returning() {
    let c = client();
    let (state, connector) = mcp_state().await;
    let (url, task) = serve(state.clone()).await;
    let s1 = s1_case(&c, &url).await;
    let (sim, source) = create_p0(&c, &url).await;
    assert_eq!(source, "mcp");
    let mut waiting = None;
    for _ in 0..60 {
        let (status, v) = advance(&c, &url, &s1, 1).await;
        assert_eq!(status, 200, "advance: {v}");
        waiting = waiting_des_case(&state.engine.lock().unwrap(), &sim);
        if waiting.is_some() {
            break;
        }
    }
    let (id, plan, _) = waiting.expect("P0 reaches a decision");
    let cmd = |body: Value| post(&c, format!("{url}/api/mom/cases/{id}/commands"), body);

    // 未审批的执行被拒绝，不发出写操作。
    let (status, _) = cmd(json!({"command":"execute","scope":"production"})).await;
    assert_eq!(status, 400, "execute needs approval");
    assert!(connector.sent_writes().is_empty(), "nothing written");

    let (status, v) = cmd(json!({"command":"select_plan","plan":plan})).await;
    assert_eq!(status, 200, "select: {v}");
    let rev = v["plan_revision"].as_u64().expect("plan revision");
    let (status, v) = cmd(
        json!({"command":"approve","scope":"production","plan_revision":rev,"actor":"supervisor"}),
    )
    .await;
    assert_eq!(status, 200, "approve: {v}");
    let (status, view) = cmd(json!({"command":"execute","scope":"production"})).await;
    assert_eq!(status, 200, "execute: {view}");

    // 执行返回时写操作已经发出，回执已写回动作。
    let sent = connector.sent_writes();
    let maintenance: Vec<&Value> = sent
        .iter()
        .filter(|w| w["tool"] == "maintenance_order_create")
        .collect();
    assert!(
        !maintenance.is_empty(),
        "a maintenance order was sent: {sent:?}"
    );
    for w in &maintenance {
        assert!(
            w["args"]["approval_ref"]
                .as_str()
                .is_some_and(|r| r.contains(&id)),
            "approval reference names the case: {w}"
        );
    }
    let order = maintenance[0]["receipt"]["order_id"]
        .as_str()
        .expect("order id")
        .to_string();
    let receipts: Vec<String> = view["actions"]
        .as_object()
        .expect("actions")
        .values()
        .filter(|a| a["tool"] == "eam.maintenance")
        .filter_map(|a| a["receipt"].as_str().map(String::from))
        .collect();
    assert!(
        receipts.iter().any(|r| r.contains(&order)),
        "the returned case carries the receipt {order}: {receipts:?}"
    );

    // 之后推进不重发。
    let count = connector.sent_writes().len();
    let (status, _) = advance(&c, &url, &s1, 30).await;
    assert_eq!(status, 200);
    assert_eq!(connector.sent_writes().len(), count, "no resend");
    task.abort();
}

#[tokio::test]
async fn rt_desc_09_the_shared_clock_advances_through_the_factory() {
    let c = client();
    let (state, connector) = mcp_state().await;
    let (url, task) = serve(state.clone()).await;
    // 先取得 S1 案例，让共享时钟越过仿真起点（起点之前模拟工厂没有时钟，DES-27）。
    s1_case(&c, &url).await;
    let (sim, source) = create_p0(&c, &url).await;
    assert_eq!(source, "mcp");
    let now0 = state.engine.lock().unwrap().now();
    state.engine.lock().unwrap().set_playback(true, 5).unwrap();
    for _ in 0..5 {
        tokio::time::timeout(WAIT, mom::clock_step(&state))
            .await
            .expect("clock step must not hang")
            .expect("clock step");
    }
    let now = state.engine.lock().unwrap().now();
    assert_eq!(now, now0 + 5, "five minutes");
    let status = connector
        .call_agent_tool("equipment_status", json!({"simulation_id": sim}))
        .await
        .unwrap();
    assert_eq!(
        status["observed_at"].as_u64(),
        Some(now),
        "the factory clock follows the engine"
    );
    // 播放结束后时钟不动。
    mom::clock_step(&state).await.unwrap();
    assert_eq!(state.engine.lock().unwrap().now(), now, "playback ended");

    // 模拟工厂退出：报错、时钟不变、播放暂停。
    state.engine.lock().unwrap().set_playback(true, 5).unwrap();
    kill(&connector);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let err = tokio::time::timeout(WAIT, mom::clock_step(&state))
        .await
        .expect("clock step must not hang")
        .expect_err("a dead factory is an error");
    assert!(err.to_string().contains(E_DES_UNAVAILABLE), "{err}");
    assert_eq!(
        state.engine.lock().unwrap().now(),
        now,
        "no silent progress"
    );
    let (_, sims) = get(&c, format!("{url}/api/mom/simulations")).await;
    assert_eq!(
        sims["playback"]["playing"], false,
        "playback paused: {sims}"
    );
    task.abort();
}

#[tokio::test]
async fn rt_desc_10_offline_factory_returns_503_without_fallback() {
    let c = client();
    let (state, connector) = mcp_state().await;
    let (url, task) = serve(state.clone()).await;
    let s1 = s1_case(&c, &url).await;
    let (sim, _) = create_p0(&c, &url).await;
    let before = {
        let e = state.engine.lock().unwrap();
        (e.now(), e.simulations_view())
    };
    kill(&connector);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let calls = [
        (format!("{url}/api/mom/des/simulations"), p0_body()),
        (
            format!("{url}/api/mom/cases/{s1}/commands"),
            json!({"command":"advance","minutes":5}),
        ),
        (
            format!("{url}/api/mom/des/tools/equipment_status"),
            json!({"simulation_id": sim}),
        ),
    ];
    for (target, body) in calls {
        let t = Instant::now();
        let (status, v) = post(&c, target.clone(), body).await;
        assert!(t.elapsed() < Duration::from_secs(10), "{target} is slow");
        assert_eq!(status, 503, "{target}: {v}");
        assert_eq!(v["code"], E_DES_UNAVAILABLE, "{target}: {v}");
    }
    let e = state.engine.lock().unwrap();
    assert_eq!(
        (e.now(), e.simulations_view()),
        before,
        "no fallback simulation and no silent progress"
    );
    drop(e);
    task.abort();
}

#[tokio::test]
async fn rt_desc_11_desktop_tools_and_views_go_through_the_factory() {
    let c = client();
    // 没有接模拟工厂：工具接口不可用。
    let (url, task) = serve(in_process_state()).await;
    let (status, v) = get(&c, format!("{url}/api/mom/des/tools")).await;
    assert_eq!(status, 400, "no simulated factory: {v}");
    task.abort();

    let (state, _connector) = mcp_state().await;
    let (url, task) = serve(state.clone()).await;
    let s1 = s1_case(&c, &url).await;
    let (sim, source) = create_p0(&c, &url).await;
    assert_eq!(source, "mcp");
    let (status, _) = advance(&c, &url, &s1, 30).await;
    assert_eq!(status, 200);
    let now = state.engine.lock().unwrap().now();

    let (status, tools) = get(&c, format!("{url}/api/mom/des/tools")).await;
    assert_eq!(status, 200, "{tools}");
    let names: BTreeSet<&str> = tools["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert_eq!(names, OBSERVE_TOOLS.into_iter().collect());

    let tool = |name: &str, body: Value| post(&c, format!("{url}/api/mom/des/tools/{name}"), body);
    let (status, status_view) = tool("equipment_status", json!({"simulation_id": sim})).await;
    assert_eq!(status, 200, "{status_view}");
    assert_eq!(status_view["provenance"], "simulation");
    assert_no_truth("equipment_status", &status_view);
    let (status, trend) = tool("equipment_trend", json!({"simulation_id": sim})).await;
    assert_eq!(status, 200, "{trend}");

    let (status, v) = tool(
        "simulation_ground_truth",
        json!({"simulation_id": sim, "admin_token": ADMIN}),
    )
    .await;
    assert_eq!(status, 403, "{v}");
    assert!(v.to_string().contains("not allowed"), "{v}");
    assert_no_truth("rejected admin call", &v);
    let (status, v) = tool(
        "simulation_advance",
        json!({"simulation_id": sim, "to_minute": now + 100}),
    )
    .await;
    assert_eq!(status, 403, "{v}");
    let (_, again) = tool("equipment_status", json!({"simulation_id": sim})).await;
    assert_eq!(
        again["observed_at"], status_view["observed_at"],
        "the rejected advance did not reach the factory"
    );

    // 工厂视图、趋势预测与模拟工厂的观测工具结果相同；what-if 用引擎里的快照计算。
    let (status, view) = get(&c, format!("{url}/api/mom/factory?simulation={sim}")).await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(view, status_view, "factory view = equipment_status");
    assert_eq!(view["observed_at"].as_u64(), Some(now));
    let (status, forecast) = get(
        &c,
        format!("{url}/api/mom/factory/forecast?simulation={sim}"),
    )
    .await;
    assert_eq!(status, 200, "{forecast}");
    assert_eq!(forecast, trend, "forecast = equipment_trend");
    assert_no_truth("forecast", &forecast);
    let (status, w) = post(
        &c,
        format!("{url}/api/mom/factory/whatif"),
        json!({"simulation_id":sim,"asset":"CNC-XX","quantity":500,"due_minute":960}),
    )
    .await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(w["plans"].as_array().map(|p| p.len()), Some(3));
    assert_no_truth("what-if", &w);
    task.abort();
}
