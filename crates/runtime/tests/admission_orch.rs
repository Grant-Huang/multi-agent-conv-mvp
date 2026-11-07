//! RT-ADM-09..13、16：准入接入 registry 与 orchestrator（用例见 docs/testing/CASES.md）。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use an_core::{
    AgentAdapter, AgentCard, EventSink, PolicyVerdict, Risk, Role, RouteDecision, TaskClass,
    TaskResult, TaskSpec, TaskStatus, WorkerStatus,
};
use an_llm::{LlmClient, LlmConfig};
use an_runtime::admission::*;
use an_runtime::bus::Bus;
use an_runtime::composer::Composer;
use an_runtime::orchestrator::Orchestrator;
use an_runtime::registry::AgentRegistry;
use an_runtime::store::Store;

struct Fake {
    card: AgentCard,
    hold: Duration,
}

#[async_trait::async_trait]
impl AgentAdapter for Fake {
    fn card(&self) -> &AgentCard {
        &self.card
    }
    async fn run(&self, _: TaskSpec, _: EventSink) -> anyhow::Result<TaskResult> {
        tokio::time::sleep(self.hold).await;
        Ok(TaskResult {
            status: WorkerStatus::Succeeded,
            summary: format!("{} 做完了", self.card.id),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        })
    }
}

/// kind = "a2a" 表示运行时接入的外部 agent；其他 kind 视为配置文件里写的 agent
fn agent(id: &str, kind: &str, ty: &str, hold_ms: u64) -> Arc<Fake> {
    Arc::new(Fake {
        card: AgentCard {
            id: id.into(),
            kind: kind.into(),
            agent_type: ty.into(),
            location: "本机".into(),
            max_concurrent: 1,
            role: Role::General,
            name: id.into(),
            description: String::new(),
            tools: vec![],
            enabled: true,
            disabled_reason: None,
        },
        hold: Duration::from_millis(hold_ms),
    })
}

struct Env {
    store: Arc<Store>,
    registry: Arc<AgentRegistry>,
    orch: Arc<Orchestrator>,
    ledger: Arc<AdmissionLedger>,
}

fn env(assign: BTreeMap<String, Vec<String>>, cfg: AdmissionConfig, agents: Vec<Arc<Fake>>) -> Env {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let bus = Arc::new(Bus::new(store.clone()));
    let registry = AgentRegistry::new(assign);
    for a in agents {
        registry.register(a);
    }
    let ledger = Arc::new(AdmissionLedger::open(store.clone(), cfg).unwrap());
    registry.set_admission(ledger.clone());
    // 端口 1 不可连接：交付消息退回模板，不依赖真实模型
    let llm = LlmClient::new(LlmConfig {
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "none".into(),
        max_retries: 0,
        ..Default::default()
    })
    .unwrap();
    let orch = Arc::new(Orchestrator::new(
        store.clone(),
        bus,
        registry.clone(),
        None,
        Composer::new(llm),
        0,
        Duration::from_secs(10),
    ));
    orch.set_admission(ledger.clone());
    Env {
        store,
        registry,
        orch,
        ledger,
    }
}

/// 让外部 agent 以指定等级通过准入（Trusted 会先审批）
fn admit(l: &AdmissionLedger, id: &str, level: TrustLevel) -> u64 {
    let mut caps = BTreeMap::new();
    caps.insert("read_context".to_string(), true);
    l.submit(AgentClaim {
        agent_id: id.into(),
        name: id.into(),
        description: "测试".into(),
        capabilities: caps,
        requested_level: level,
        max_subtask_depth: None,
        working_language: Some("zh".into()),
    })
    .unwrap();
    l.clarify(id, vec![]).unwrap();
    l.score(
        id,
        Scores {
            completeness: 1.0,
            conversation: 1.0,
            test: 1.0,
        },
    )
    .unwrap();
    l.contract(
        id,
        Contract {
            role: "executor".into(),
            working_language: "zh".into(),
            context_scope: vec![],
            allowed_tools: vec![],
        },
    )
    .unwrap();
    if level == TrustLevel::Trusted {
        l.approve(id, "supervisor").unwrap();
    }
    l.activate(id).unwrap()
}

fn route(risk: Risk) -> RouteDecision {
    RouteDecision {
        task_class: TaskClass::L1,
        execution_role: Role::General,
        required_tools: vec![],
        risk_level: risk,
        reason: String::new(),
    }
}

fn auto() -> PolicyVerdict {
    PolicyVerdict {
        auto_execute: true,
        needs_review: false,
        notes: vec![],
    }
}

fn status_of(store: &Store, id: &str) -> TaskStatus {
    store.get_task(id).unwrap().unwrap().status
}

async fn wait_for(store: &Store, id: &str, want: &[TaskStatus], secs: u64) -> TaskStatus {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = status_of(store, id);
        if want.contains(&s) {
            return s;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "等待 {want:?} 超时，当前 {s:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn deliveries(store: &Store, conv: &str, task_id: &str) -> usize {
    store
        .list_messages(conv, None)
        .unwrap()
        .iter()
        .filter(|m| m.role == "assistant" && m.task_id.as_deref() == Some(task_id))
        .count()
}

fn result_text(store: &Store, id: &str) -> String {
    serde_json::to_string(&store.get_task(id).unwrap().unwrap().result).unwrap()
}

#[tokio::test]
async fn rt_adm_09_registry_allocates_by_admission() {
    let e = env(
        BTreeMap::new(),
        AdmissionConfig::default(),
        vec![agent("ext", "a2a", "ext", 0)],
    );
    // 未准入的外部 agent：不分配，角色不可用
    let err = e
        .registry
        .acquire_for(Role::General, Risk::Low, |_| {})
        .await
        .err()
        .expect("未准入的外部 agent 不应被分配");
    assert_eq!(err.code, an_core::codes::ROUTE_NO_AGENT);
    assert!(err.message.contains("准入"), "{}", err.message);
    assert!(!e.registry.is_enabled(Role::General));
    // 以 Guest 准入：只领低风险
    let l = &e.ledger;
    let mut caps = BTreeMap::new();
    caps.insert("read_context".to_string(), true);
    l.submit(AgentClaim {
        agent_id: "ext".into(),
        name: "ext".into(),
        description: "测试".into(),
        capabilities: caps,
        requested_level: TrustLevel::Guest,
        max_subtask_depth: None,
        working_language: Some("zh".into()),
    })
    .unwrap();
    l.clarify("ext", vec![]).unwrap();
    l.score(
        "ext",
        Scores {
            completeness: 1.0,
            conversation: 1.0,
            test: 1.0,
        },
    )
    .unwrap();
    l.contract(
        "ext",
        Contract {
            role: "observer".into(),
            working_language: "zh".into(),
            context_scope: vec![],
            allowed_tools: vec![],
        },
    )
    .unwrap();
    l.activate("ext").unwrap();
    let lease = e
        .registry
        .acquire_for(Role::General, Risk::Low, |_| {})
        .await
        .unwrap();
    assert_eq!(lease.agent.card().id, "ext");
    assert_eq!(lease.epoch, Some(1));
    drop(lease);
    assert!(e
        .registry
        .acquire_for(Role::General, Risk::Medium, |_| {})
        .await
        .is_err());

    // 配置 agent（非 a2a）没有记录时视为已信任
    let e2 = env(
        BTreeMap::new(),
        AdmissionConfig::default(),
        vec![agent("local", "builtin", "general", 0)],
    );
    let lease = e2
        .registry
        .acquire_for(Role::General, Risk::High, |_| {})
        .await
        .unwrap();
    assert_eq!(lease.epoch, Some(1));
    assert!(e2.ledger.get("local").is_none(), "分配不应写入准入记录");

    // 未开启准入：外部 agent 照常分配（行为不变）
    let plain = AgentRegistry::new(BTreeMap::new());
    plain.register(agent("ext", "a2a", "ext", 0));
    let lease = plain
        .acquire_for(Role::General, Risk::High, |_| {})
        .await
        .unwrap();
    assert_eq!(lease.epoch, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_adm_10_revoke_aborts_running_and_backup_takes_over() {
    let mut assign = BTreeMap::new();
    assign.insert(
        "general".to_string(),
        vec!["ext1".to_string(), "ext2".to_string()],
    );
    let e = env(
        assign,
        AdmissionConfig::default(),
        vec![
            agent("ext1", "a2a", "ext", 3_000),
            agent("ext2", "a2a", "ext", 10),
        ],
    );
    admit(&e.ledger, "ext1", TrustLevel::Member);
    admit(&e.ledger, "ext2", TrustLevel::Member);
    let conv = e.store.create_conversation("t").unwrap().id;
    let a = e
        .orch
        .submit(&conv, "长任务", "", route(Risk::Low), auto())
        .unwrap();
    wait_for(&e.store, &a.id, &[TaskStatus::Running], 5).await;
    let started = std::time::Instant::now();
    let report = e.orch.revoke_agent("ext1", "测试撤权").unwrap();
    assert_eq!(report.aborted, vec![a.id.clone()]);
    assert_eq!(report.epoch, 2);
    let s = wait_for(&e.store, &a.id, &[TaskStatus::Failed], 2).await;
    assert_eq!(s, TaskStatus::Failed);
    assert!(started.elapsed() < Duration::from_secs(1), "撤权应立即生效");
    assert!(result_text(&e.store, &a.id).contains(an_core::codes::AGENT_REVOKED));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(deliveries(&e.store, &conv, &a.id), 0, "被撤权的任务不交付");
    // 后续任务由备份完成
    let b = e
        .orch
        .submit(&conv, "下一个", "", route(Risk::Low), auto())
        .unwrap();
    wait_for(&e.store, &b.id, &[TaskStatus::Completed], 5).await;
    assert!(result_text(&e.store, &b.id).contains("ext2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_adm_11_stale_credential_result_is_discarded() {
    let e = env(
        BTreeMap::new(),
        AdmissionConfig::default(),
        vec![agent("ext", "a2a", "ext", 400)],
    );
    admit(&e.ledger, "ext", TrustLevel::Member);
    let conv = e.store.create_conversation("t").unwrap().id;
    // 凭证有效：照常接受
    let ok = e
        .orch
        .submit(&conv, "正常", "", route(Risk::Low), auto())
        .unwrap();
    wait_for(&e.store, &ok.id, &[TaskStatus::Completed], 5).await;
    // 执行期间被降级（6 次连续不合格 = 3 次警告）
    let t = e
        .orch
        .submit(&conv, "执行中被降级", "", route(Risk::Low), auto())
        .unwrap();
    wait_for(&e.store, &t.id, &[TaskStatus::Running], 5).await;
    for _ in 0..6 {
        e.ledger.record_check("ext", false).unwrap();
    }
    assert_eq!(e.ledger.get("ext").unwrap().epoch, 2);
    let s = wait_for(
        &e.store,
        &t.id,
        &[TaskStatus::Failed, TaskStatus::Completed],
        5,
    )
    .await;
    assert_eq!(s, TaskStatus::Failed, "旧凭证交回的结果必须被丢弃");
    assert!(result_text(&e.store, &t.id).contains(an_core::codes::AGENT_REVOKED));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(deliveries(&e.store, &conv, &t.id), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_adm_12_revoke_and_completion_never_overwrite() {
    let rounds = 40;
    let agents: Vec<Arc<Fake>> = (0..rounds)
        // 偶数轮：执行 300ms，先等到 RUNNING 再撤权（必须中止）；奇数轮：几毫秒，提交后立刻撤权（竞争）
        .map(|i| {
            let hold = if i % 2 == 0 { 300 } else { (i % 4) as u64 };
            agent(&format!("ext{i:02}"), "a2a", "ext", hold)
        })
        .collect();
    let e = env(BTreeMap::new(), AdmissionConfig::default(), agents);
    for i in 0..rounds {
        admit(&e.ledger, &format!("ext{i:02}"), TrustLevel::Member);
    }
    let conv = e.store.create_conversation("t").unwrap().id;
    let mut outcomes: HashMap<String, bool> = HashMap::new();
    for i in 0..rounds {
        let id = format!("ext{i:02}");
        let task = e
            .orch
            .submit(&conv, "竞争", "", route(Risk::Low), auto())
            .unwrap();
        if i % 2 == 0 {
            wait_for(&e.store, &task.id, &[TaskStatus::Running], 5).await;
        }
        let report = e.orch.revoke_agent(&id, "并发测试").unwrap();
        outcomes.insert(task.id.clone(), report.aborted.contains(&task.id));
        let s = wait_for(
            &e.store,
            &task.id,
            &[TaskStatus::Completed, TaskStatus::Failed],
            5,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        // 终态不再变化
        assert_eq!(status_of(&e.store, &task.id), s);
        let n = deliveries(&e.store, &conv, &task.id);
        if outcomes[&task.id] {
            assert_eq!(s, TaskStatus::Failed, "{}", task.id);
            assert_eq!(n, 0, "{}", task.id);
        }
        if s == TaskStatus::Completed {
            assert_eq!(n, 1, "{}", task.id);
        }
        if i % 2 == 0 {
            assert!(
                outcomes[&task.id],
                "执行中的任务必须被撤权中止：{}",
                task.id
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_adm_13_probation_tracks_completed_tasks() {
    let cfg = AdmissionConfig {
        sample_rate: 1.0,
        ..AdmissionConfig::default()
    };
    let mut assign = BTreeMap::new();
    assign.insert("general".to_string(), vec!["ext".to_string()]);
    let e = env(assign, cfg, vec![agent("ext", "a2a", "ext", 0)]);
    admit(&e.ledger, "ext", TrustLevel::Member);
    let conv = e.store.create_conversation("t").unwrap().id;
    let mut ids = vec![];
    for i in 0..3 {
        let t = e
            .orch
            .submit(&conv, &format!("任务{i}"), "", route(Risk::Low), auto())
            .unwrap();
        wait_for(&e.store, &t.id, &[TaskStatus::Completed], 5).await;
        ids.push(t.id);
    }
    let p = e.ledger.get("ext").unwrap().probation;
    assert_eq!(p.tasks_completed, 3);
    assert_eq!(p.pending_checks, ids);
    e.ledger.record_check_for("ext", &ids[0], true).unwrap();
    assert_eq!(
        e.ledger.get("ext").unwrap().probation.pending_checks.len(),
        2
    );
    assert!(e
        .ledger
        .record_check_for("ext", "task-unknown", true)
        .is_err());
    assert!(
        e.ledger.record_check_for("ext", &ids[0], true).is_err(),
        "不能重复回写"
    );

    // 配置 agent 完成任务不产生准入记录
    let e2 = env(
        BTreeMap::new(),
        AdmissionConfig::default(),
        vec![agent("local", "builtin", "general", 0)],
    );
    let conv = e2.store.create_conversation("t").unwrap().id;
    let t = e2
        .orch
        .submit(&conv, "x", "", route(Risk::High), auto())
        .unwrap();
    wait_for(&e2.store, &t.id, &[TaskStatus::Completed], 5).await;
    assert!(e2.ledger.get("local").is_none());
}

#[test]
fn rt_adm_16_config_section() {
    let base = "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n[agents.general]\nenabled = false\n";
    let (cfg, unknown) = an_runtime::config::AppConfig::parse(base).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(!cfg.admission.enabled);
    assert_eq!(cfg.admission.rules(), AdmissionConfig::default());
    let rt = an_runtime::Runtime::new(cfg, Store::open(Path::new(":memory:")).unwrap()).unwrap();
    assert!(rt.admission.is_none());

    let text = format!("{base}[admission]\nenabled = true\nsample_rate = 0.5\nseed = 3\n");
    let (cfg, unknown) = an_runtime::config::AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(cfg.admission.enabled);
    assert_eq!(cfg.admission.rules().sample_rate, 0.5);
    assert_eq!(cfg.admission.rules().seed, 3);
    let rt = an_runtime::Runtime::new(cfg, Store::open(Path::new(":memory:")).unwrap()).unwrap();
    assert!(rt.admission.is_some());

    let bad = format!("{base}[admission]\nenabled = true\nsample_rate = 2.0\n");
    let (cfg, _) = an_runtime::config::AppConfig::parse(&bad).unwrap();
    assert!(an_runtime::Runtime::new(cfg, Store::open(Path::new(":memory:")).unwrap()).is_err());
}
