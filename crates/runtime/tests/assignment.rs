//! 运行时集成用例 RT-ASSIGN-01 ~ RT-ASSIGN-05（定义见 docs/testing/CASES.md）：
//! 任务记录里的执行者（agent、主力 / 备份、排队时长）和 Policy 给出的候选执行者。
//! 用内存库 + 假 agent；交付消息的模型指向不可连接的地址，Composer 立即退回模板，不依赖真实模型。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use an_core::{
    AgentAdapter, AgentCard, EventSink, PolicyVerdict, Risk, Role, RouteDecision, TaskClass,
    TaskRecord, TaskResult, TaskSpec, TaskStatus, WorkerStatus,
};
use an_llm::{LlmClient, LlmConfig};
use an_runtime::bus::Bus;
use an_runtime::composer::Composer;
use an_runtime::orchestrator::Orchestrator;
use an_runtime::policy::Policy;
use an_runtime::registry::AgentRegistry;
use an_runtime::store::Store;

/// 假 agent：每次执行先等 `hold` 再返回成功
struct FakeAgent {
    card: AgentCard,
    hold: Duration,
}

#[async_trait::async_trait]
impl AgentAdapter for FakeAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }
    async fn run(&self, _: TaskSpec, _: EventSink) -> anyhow::Result<TaskResult> {
        tokio::time::sleep(self.hold).await;
        Ok(TaskResult {
            status: WorkerStatus::Succeeded,
            summary: "做完了".into(),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        })
    }
}

fn fake(id: &str, max_concurrent: u32, hold: Duration) -> Arc<FakeAgent> {
    Arc::new(FakeAgent {
        card: AgentCard {
            id: id.into(),
            kind: "test".into(),
            agent_type: "fake".into(),
            location: "本机".into(),
            max_concurrent,
            role: Role::General,
            name: format!("Agent {id}"),
            description: String::new(),
            tools: vec![],
            enabled: true,
            disabled_reason: None,
        },
        hold,
    })
}

struct Env {
    orch: Arc<Orchestrator>,
    store: Arc<Store>,
}

/// `assign`：general 执行组成员（空 = 不配置执行组）
fn env(agents: Vec<Arc<FakeAgent>>, assign: &[&str]) -> (Env, Arc<AgentRegistry>) {
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let bus = Arc::new(Bus::new(store.clone()));
    let mut groups = BTreeMap::new();
    if !assign.is_empty() {
        groups.insert(
            "general".to_string(),
            assign.iter().map(|s| s.to_string()).collect(),
        );
    }
    let registry = AgentRegistry::new(groups);
    for a in agents {
        registry.register(a);
    }
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
    (Env { orch, store }, registry)
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
        candidates: vec![],
    }
}

fn task(store: &Store, id: &str) -> TaskRecord {
    store.get_task(id).unwrap().unwrap()
}

/// 等到任务进入 `want` 中的某个状态，最多等 5 秒
async fn wait_for(store: &Store, id: &str, want: &[TaskStatus]) -> TaskRecord {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let t = task(store, id);
        if want.contains(&t.status) {
            return t;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "等待 {want:?} 超时，当前状态 {:?}",
            t.status
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn rt_assign_01_completed_task_records_its_agent() {
    // RT-ASSIGN-01：只有一个 agent 时，完成的任务记录了它，是主力、组内 1 个、首轮
    let (e, _) = env(vec![fake("solo", 1, Duration::from_millis(20))], &[]);
    let conv = e.store.create_conversation("t").unwrap().id;
    let t = e
        .orch
        .submit(&conv, "g", "", route(Risk::Low), auto())
        .unwrap();
    let done = wait_for(&e.store, &t.id, &[TaskStatus::Completed]).await;
    let a = done.assignment.expect("完成的任务必须记录执行者");
    assert_eq!(a.agent_id, "solo");
    assert_eq!(a.agent_name, "Agent solo");
    assert_eq!(a.kind, "test");
    assert_eq!(a.location, "本机");
    assert_eq!(a.member_index, 0);
    assert_eq!(a.group_size, 1);
    assert_eq!(a.round, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_assign_02_busy_primary_hands_task_to_backup() {
    // RT-ASSIGN-02：执行组 [a, b] 各并发 1，同时两个任务，一个落在主力 a，一个落在备份 b
    let hold = Duration::from_millis(300);
    let (e, _) = env(vec![fake("a", 1, hold), fake("b", 1, hold)], &["a", "b"]);
    let conv = e.store.create_conversation("t").unwrap().id;
    let t1 = e
        .orch
        .submit(&conv, "g1", "", route(Risk::Low), auto())
        .unwrap();
    let t2 = e
        .orch
        .submit(&conv, "g2", "", route(Risk::Low), auto())
        .unwrap();
    let r1 = wait_for(&e.store, &t1.id, &[TaskStatus::Running]).await;
    let r2 = wait_for(&e.store, &t2.id, &[TaskStatus::Running]).await;
    // 进入 RUNNING 时执行者已经写入（前端收到 RUNNING 事件刷新即可看到）
    let mut got: Vec<(String, u32, u32)> = [r1, r2]
        .into_iter()
        .map(|t| {
            let a = t.assignment.expect("RUNNING 时必须已记录执行者");
            (a.agent_id, a.member_index, a.group_size)
        })
        .collect();
    got.sort();
    assert_eq!(got, vec![("a".into(), 0, 2), ("b".into(), 1, 2)]);
}

#[tokio::test]
async fn rt_assign_03_queued_task_records_wait_time() {
    // RT-ASSIGN-03：唯一的 agent 被占用 300ms，第二个任务记录了排队时长
    let (e, _) = env(vec![fake("solo", 1, Duration::from_millis(300))], &[]);
    let conv = e.store.create_conversation("t").unwrap().id;
    let t1 = e
        .orch
        .submit(&conv, "g1", "", route(Risk::Low), auto())
        .unwrap();
    wait_for(&e.store, &t1.id, &[TaskStatus::Running]).await;
    let t2 = e
        .orch
        .submit(&conv, "g2", "", route(Risk::Low), auto())
        .unwrap();
    let done = wait_for(&e.store, &t2.id, &[TaskStatus::Completed]).await;
    let a = done.assignment.expect("完成的任务必须记录执行者");
    assert!(
        a.waited_ms >= 200,
        "排队约 300ms，记录的是 {}ms",
        a.waited_ms
    );
    // 没排队的第一个任务等待时间很短
    let first = task(&e.store, &t1.id)
        .assignment
        .expect("第一个任务也有执行者");
    assert!(
        first.waited_ms < 200,
        "第一个任务不该排队：{}ms",
        first.waited_ms
    );
}

#[test]
fn rt_assign_04_old_db_gains_assignment_column() {
    // RT-ASSIGN-04：旧库的 tasks 表没有 assignment_json，打开时自动补列，旧数据照常读出
    let dir = std::env::temp_dir().join(an_core::new_id("an-assign-migrate"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("old.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, goal TEXT NOT NULL,
                role TEXT NOT NULL, status TEXT NOT NULL, route_json TEXT NOT NULL,
                policy_json TEXT NOT NULL, result_json TEXT, review_json TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                context TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        // 旧版本写入的 policy_json 没有 candidates 字段
        conn.execute(
            r#"INSERT INTO tasks VALUES ('t1','c1','旧任务','general','COMPLETED',
               '{"task_class":"L1","execution_role":"general","required_tools":[],"risk_level":"low","reason":"旧"}',
               '{"auto_execute":true,"needs_review":false,"notes":["n"]}',
               NULL,NULL,'2026-01-01','2026-01-01','')"#,
            [],
        )
        .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let has_column: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'assignment_json'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_column, 1, "打开旧库时应补上 assignment_json 列");
    let t = task(&store, "t1");
    assert_eq!(t.goal, "旧任务");
    assert_eq!(t.status, TaskStatus::Completed);
    assert_eq!(t.policy.notes, vec!["n".to_string()]);
    assert!(t.policy.candidates.is_empty());
    assert!(t.assignment.is_none());
    // 重复打开（幂等）
    drop(store);
    Store::open(&db).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rt_assign_05_policy_lists_candidates_in_group_order() {
    // RT-ASSIGN-05：高风险路由需审批，候选执行者按执行组顺序列出，主力在前
    let hold = Duration::from_millis(1);
    let (_, registry) = env(vec![fake("a", 1, hold), fake("b", 1, hold)], &["a", "b"]);
    let policy = Policy {
        approval_risk: Risk::High,
    };
    let mut r = route(Risk::High);
    let v = policy.evaluate(&mut r, &registry).unwrap();
    assert!(!v.auto_execute);
    assert_eq!(
        v.candidates,
        vec![
            "Agent a（test，本机）".to_string(),
            "Agent b（test，本机）".to_string()
        ]
    );
    // 旧版本序列化的裁决没有 candidates，读出来为空
    let old: PolicyVerdict =
        serde_json::from_str(r#"{"auto_execute":false,"needs_review":false,"notes":[]}"#).unwrap();
    assert!(old.candidates.is_empty());
}
