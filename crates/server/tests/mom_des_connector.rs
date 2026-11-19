//! 阶段⑤a-2：AgentNexus 侧模拟工厂连接器（RT-DESC-01～05）。
//!
//! 用例见 `docs/testing/CASES.md`「模拟工厂连接器（阶段⑤a-2）」。连接器经 stdio 启动 `an-des-mcp`；
//! 每个等待都有超时，模拟工厂未实现或卡住时测试失败而不是挂起。

use an_runtime::mom::des::{DesSimulationConfig, Pattern, ScenarioConfig};
use an_runtime::mom::Engine;
use an_server::mom_des_connector::{DesConnector, E_DES_UNAVAILABLE};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

const START: u64 = 497;
const ADMIN: &str = "connector-test-admin";
const WAIT: Duration = Duration::from_secs(60);

/// `an-des-mcp` 可执行文件：优先用环境变量 `AN_DES_MCP_BIN`；否则在本测试所在的 target 目录中找，
/// 找不到时用 cargo 构建一次（`cargo test --workspace` 会先构建它）。
fn des_mcp_bin() -> PathBuf {
    if let Ok(p) = std::env::var("AN_DES_MCP_BIN") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let bin = dir.join(format!("an-des-mcp{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = std::process::Command::new(cargo)
            .args(["build", "--locked", "-p", "an-des-mcp"])
            .status()
            .expect("build an-des-mcp");
        assert!(status.success(), "an-des-mcp must build");
    }
    bin
}

async fn connect() -> DesConnector {
    tokio::time::timeout(WAIT, DesConnector::spawn(&des_mcp_bin(), &[], ADMIN))
        .await
        .expect("spawn timed out")
        .expect("the simulated factory starts")
}

fn config(seed: u64, pattern: Pattern, horizon: u64) -> DesSimulationConfig {
    DesSimulationConfig {
        name: format!("connector-{seed}"),
        factory: None,
        scenario: ScenarioConfig {
            seed,
            pattern,
            start_minute: START,
            horizon_minutes: horizon,
            mechanisms: None,
        },
    }
}

fn cluster() -> Pattern {
    Pattern::ConcurrentCluster {
        window_minutes: 60,
        min_disruptions: 3,
    }
}

/// 第一个等待人工决策且有推荐方案、还没执行过的案例。
fn waiting_case(e: &Engine, done: &BTreeSet<String>) -> Option<(String, String)> {
    e.case_ids().into_iter().find_map(|id| {
        let c = e.case(&id).ok()?;
        let plan = c.proposal.as_ref()?["recommended"].as_str()?.to_string();
        (c.stage == "WAITING_HUMAN" && !done.contains(&id)).then_some((id, plan))
    })
}

fn decide(e: &mut Engine, id: &str, plan: &str) {
    e.select_des_plan(id, plan).expect("plan selectable");
    let rev = e.case(id).unwrap().plan_revision;
    e.approve(id, rev, "production", "supervisor").unwrap();
    e.execute(id, "production").unwrap();
}

/// 引擎可比较的状态：每个案例的批次、阶段、模板、事件种类序列、可观测设备、方案与动作结果。
/// 回执编号等由各自系统生成的标识不参与比较。
fn fingerprint(e: &Engine) -> Value {
    let cases: Vec<Value> = e
        .case_ids()
        .into_iter()
        .map(|id| {
            let c = e.case(&id).unwrap();
            let kinds: Vec<String> = e
                .events(&id, 0, 1000)
                .unwrap()
                .into_iter()
                .map(|ev| ev.kind)
                .collect();
            let plans: Vec<Value> = c
                .proposal
                .as_ref()
                .and_then(|p| p["plans"].as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .map(|p| json!({"id": p["id"], "feasible": p["feasible"], "completion": p["completion_minute"]}))
                .collect();
            let actions: Vec<Value> = c
                .actions
                .values()
                .map(|a| json!({"tool": a.tool, "state": a.state, "complete_at": a.complete_at}))
                .collect();
            json!({
                "id": id,
                "batch": c.batch,
                "stage": c.stage,
                "template": c.template,
                "kinds": kinds,
                "machines": c.world.get("des").map(|d| d["machines"].clone()),
                "plans": plans,
                "actions": actions,
            })
        })
        .collect();
    json!({"now": e.now(), "cases": cases})
}

#[tokio::test]
async fn rt_desc_01_mcp_mode_matches_the_in_process_engine() {
    for (seed, pattern) in [(1, Pattern::Script), (7, cluster())] {
        let horizon = 240;
        let cfg = config(seed, pattern, horizon);
        // 进程内模式。
        let mut local = Engine::memory().unwrap();
        let sim = local.create_des_simulation(cfg.clone()).unwrap();
        // MCP 模式。
        let factory = connect().await;
        let remote = Mutex::new(Engine::memory().unwrap());
        let remote_sim = factory
            .create_simulation(&remote, cfg)
            .await
            .expect("simulation created in the factory");
        assert_eq!(remote_sim, sim, "same simulation id");

        let mut done = BTreeSet::new();
        for t in START..=START + horizon {
            local.advance(t).unwrap();
            tokio::time::timeout(WAIT, factory.tick(&remote, t))
                .await
                .expect("tick timed out")
                .expect("tick");
            // 同样的人工决策：第一个等待决策的案例按推荐方案执行。
            if let Some((id, plan)) = waiting_case(&local, &done) {
                decide(&mut local, &id, &plan);
                decide(&mut remote.lock().unwrap(), &id, &plan);
                factory.dispatch_writes(&remote).await.expect("writes sent");
                done.insert(id);
            }
        }
        assert!(
            !done.is_empty(),
            "seed {seed}: at least one decision was made"
        );
        assert_eq!(
            fingerprint(&remote.lock().unwrap()),
            fingerprint(&local),
            "seed {seed}: MCP mode must match the in-process engine"
        );
    }
}

#[tokio::test]
async fn rt_desc_02_writes_only_after_approval_and_idempotent() {
    let factory = connect().await;
    let engine = Mutex::new(Engine::memory().unwrap());
    let sim = factory
        .create_simulation(&engine, config(1, Pattern::Script, 240))
        .await
        .unwrap();
    let mut waiting = None;
    for t in START..=START + 30 {
        factory.tick(&engine, t).await.unwrap();
        waiting = waiting_case(&engine.lock().unwrap(), &BTreeSet::new());
        if waiting.is_some() {
            break;
        }
    }
    let (id, _) = waiting.expect("P0 reaches a decision");
    // 等待决策时没有任何写操作。
    assert!(factory.dispatch_writes(&engine).await.unwrap().is_empty());
    assert!(
        factory.sent_writes().is_empty(),
        "nothing is written before approval"
    );
    // 未审批的执行被拒绝，也不产生写操作。
    {
        let mut e = engine.lock().unwrap();
        e.select_des_plan(&id, "C").unwrap();
        assert!(
            e.execute(&id, "production").is_err(),
            "execute needs approval"
        );
        assert!(e.des_pending_writes().is_empty());
    }
    assert!(factory.dispatch_writes(&engine).await.unwrap().is_empty());

    // 审批并执行后才发出写操作，带幂等键与审批引用。
    let pending = {
        let mut e = engine.lock().unwrap();
        let rev = e.case(&id).unwrap().plan_revision;
        e.approve(&id, rev, "production", "supervisor").unwrap();
        e.execute(&id, "production").unwrap();
        e.des_pending_writes()
    };
    let tools: BTreeSet<&str> = pending.iter().map(|w| w.tool.as_str()).collect();
    assert!(tools.contains("maintenance_order_create"), "{tools:?}");
    for w in &pending {
        assert!(w.key.starts_with(&format!("{id}:")), "key {}", w.key);
        assert_eq!(w.args["idempotency_key"], w.key.as_str());
        assert_eq!(w.args["simulation_id"], sim.as_str());
        assert!(
            w.args["approval_ref"]
                .as_str()
                .is_some_and(|r| r.contains(&id)),
            "approval reference names the case: {}",
            w.args
        );
    }
    // 第一次发出后回执"丢失"（不记录），再次发出：同键只生效一次，回执相同。
    let first = factory.send_writes(&pending).await.unwrap();
    let receipts = factory.dispatch_writes(&engine).await.unwrap();
    let ids = |r: &[Value]| -> Vec<Value> { r.iter().map(|x| x["order_id"].clone()).collect() };
    assert_eq!(
        ids(&first),
        ids(&receipts),
        "a resend returns the original receipts"
    );
    assert!(
        engine.lock().unwrap().des_pending_writes().is_empty(),
        "receipts recorded"
    );
    // 回执写回动作：维修动作的完成时刻来自模拟工厂的回执。
    let e = engine.lock().unwrap();
    let maintenance = e
        .case(&id)
        .unwrap()
        .actions
        .values()
        .find(|a| a.tool == "eam.maintenance")
        .expect("maintenance action")
        .clone();
    let receipt = receipts
        .iter()
        .find(|r| r["kind"] == "maintenance")
        .expect("maintenance receipt");
    assert_eq!(
        Some(maintenance.complete_at),
        receipt["complete_minute"].as_u64()
    );
    assert!(maintenance
        .receipt
        .contains(receipt["order_id"].as_str().unwrap()));
}

#[tokio::test]
async fn rt_desc_03_agents_only_see_observation_tools() {
    let factory = connect().await;
    let engine = Mutex::new(Engine::memory().unwrap());
    let sim = factory
        .create_simulation(&engine, config(1, Pattern::Script, 60))
        .await
        .unwrap();
    let allowed: BTreeSet<String> = factory.agent_tools().into_iter().collect();
    let expected: BTreeSet<String> = [
        "equipment_status",
        "equipment_trend",
        "quality_capability",
        "inventory_tooling",
        "events_poll",
        "order_status",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(allowed, expected, "agents get observation tools only");
    let status = factory
        .call_agent_tool("equipment_status", json!({"simulation_id": sim}))
        .await
        .expect("observation allowed");
    assert_eq!(status["provenance"], "simulation");
    for (tool, args) in [
        (
            "maintenance_order_create",
            json!({"simulation_id": sim, "machine": "CNC-XX", "start_minute": START, "idempotency_key": "x", "approval_ref": "x"}),
        ),
        (
            "simulation_advance",
            json!({"simulation_id": sim, "to_minute": START + 10}),
        ),
        (
            "simulation_ground_truth",
            json!({"simulation_id": sim, "admin_token": ADMIN}),
        ),
    ] {
        let err = factory
            .call_agent_tool(tool, args)
            .await
            .expect_err("non-observation tools are refused");
        assert!(err.to_string().contains("not allowed"), "{tool}: {err}");
    }
    // 被拒绝的控制调用没有发给模拟工厂：时钟没有动。
    let after = factory
        .call_agent_tool("equipment_status", json!({"simulation_id": sim}))
        .await
        .unwrap();
    assert_eq!(
        after["observed_at"], START,
        "the refused advance never reached the factory"
    );
}

#[tokio::test]
async fn rt_desc_04_offline_factory_fails_loudly_without_fallback() {
    let missing = PathBuf::from("/nonexistent/an-des-mcp");
    let err = tokio::time::timeout(WAIT, DesConnector::spawn(&missing, &[], ADMIN))
        .await
        .expect("spawn must not hang")
        .err()
        .expect("a missing factory is an error");
    assert!(err.to_string().contains(E_DES_UNAVAILABLE), "{err}");

    // 运行中模拟工厂退出：推进报错，引擎不推进、不写入观测，也没有进程内仿真可以回退。
    let factory = connect().await;
    let engine = Mutex::new(Engine::memory().unwrap());
    let sim = factory
        .create_simulation(&engine, config(1, Pattern::Script, 240))
        .await
        .unwrap();
    factory.tick(&engine, START + 5).await.unwrap();
    let before = {
        let e = engine.lock().unwrap();
        (e.now(), e.case_ids().len())
    };
    let pid = factory.child_pid().expect("factory process id");
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let err = tokio::time::timeout(WAIT, factory.tick(&engine, START + 30))
        .await
        .expect("tick must not hang")
        .expect_err("a dead factory is an error");
    assert!(err.to_string().contains(E_DES_UNAVAILABLE), "{err}");
    let e = engine.lock().unwrap();
    assert_eq!((e.now(), e.case_ids().len()), before, "no silent progress");
    assert!(
        e.des_factory_view(&sim).is_err(),
        "the engine has no in-process simulation to fall back to"
    );
}

#[tokio::test]
async fn rt_desc_05_review_truth_comes_from_the_factory_admin_tool() {
    let cfg = config(1, Pattern::Script, 240);
    let mut local = Engine::memory().unwrap();
    let sim = local.create_des_simulation(cfg.clone()).unwrap();
    let factory = connect().await;
    let remote = Mutex::new(Engine::memory().unwrap());
    factory.create_simulation(&remote, cfg).await.unwrap();
    assert_eq!(
        remote
            .lock()
            .unwrap()
            .des_review_truth(&sim)
            .expect("truth for review"),
        local.des_review_truth(&sim).unwrap(),
        "the review layer reads the factory's ground truth"
    );
    // 同一根因假设，审核规则结果一致。
    let mut waiting = None;
    for t in START..=START + 30 {
        local.advance(t).unwrap();
        factory.tick(&remote, t).await.unwrap();
        waiting = waiting_case(&local, &BTreeSet::new());
        if waiting.is_some() {
            break;
        }
    }
    let (id, _) = waiting.expect("P0 reaches a decision");
    let rule = |e: &mut Engine| {
        e.record_cause_claim(&id, vec!["spindle_wear:CNC-XX".into()], "master")
            .unwrap();
        let report = e.des_review_report(&id).unwrap();
        report["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rule"] == "root_cause_attribution")
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(rule(&mut remote.lock().unwrap()), rule(&mut local));
    assert_eq!(rule(&mut local), "pass");
}
