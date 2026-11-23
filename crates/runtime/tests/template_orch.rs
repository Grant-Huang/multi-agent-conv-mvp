//! WP4 编排集成：RT-TPL-11～16（docs/testing/CASES.md）。
//! 假 agent + 内存库；Composer 指向不可达地址，交付退回模板文案。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use an_core::{
    now, AgentAdapter, AgentCard, PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskRecord,
    TaskResult, TaskSpec, TaskStatus, WorkerStatus,
};
use an_llm::{LlmClient, LlmConfig};
use an_runtime::builtin_templates;
use an_runtime::bus::Bus;
use an_runtime::composer::Composer;
use an_runtime::orchestrator::Orchestrator;
use an_runtime::registry::AgentRegistry;
use an_runtime::store::Store;
use an_runtime::template::TemplateRegistry;
use serde_json::json;

struct Env {
    orch: Arc<Orchestrator>,
    store: Arc<Store>,
}

/// 按目标里的「· 分支 {id}」返回合法/非法信封；`hold` 可拖住取消用例。
struct EnvelopeAgent {
    card: AgentCard,
    hold: Duration,
    /// 若 goal 含此子串则返回坏信封（RT-TPL-13）
    bad_branch: Option<&'static str>,
    runs: AtomicUsize,
}

#[async_trait::async_trait]
impl AgentAdapter for EnvelopeAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }
    async fn run(&self, spec: TaskSpec, _: an_core::EventSink) -> anyhow::Result<TaskResult> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if !self.hold.is_zero() {
            tokio::time::sleep(self.hold).await;
        }
        let bid = branch_from_goal(&spec.goal).unwrap_or("unknown");
        let summary = if self.bad_branch == Some(bid) {
            format!(r#"{{"branch_id":"{bid}","status":"ok","missing":"","payload":{{}}}}"#)
        } else {
            match bid {
                "damage" => {
                    r#"{"branch_id":"damage","status":"ok","missing":"","payload":{"verdict":"可修","hours":"6"}}"#
                        .into()
                }
                "parts" => {
                    r#"{"branch_id":"parts","status":"ok","missing":"","payload":{"shortage_amount":"1200"}}"#
                        .into()
                }
                "history" => {
                    r#"{"branch_id":"history","status":"ok","missing":"","payload":{"warranty_risk":"低"}}"#
                        .into()
                }
                other => json!({
                    "branch_id": other,
                    "status": "ok",
                    "missing": "",
                    "payload": {"ok": "1"}
                })
                .to_string(),
            }
        };
        Ok(TaskResult {
            status: WorkerStatus::Succeeded,
            summary,
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        })
    }
}

fn branch_from_goal(goal: &str) -> Option<&str> {
    goal.rsplit_once("· 分支 ").map(|(_, id)| id.trim())
}

fn env(hold: Duration, bad_branch: Option<&'static str>) -> Env {
    let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
    let bus = Arc::new(Bus::new(store.clone()));
    let registry = AgentRegistry::new(BTreeMap::new());
    let agent = Arc::new(EnvelopeAgent {
        card: AgentCard {
            id: "fake-tpl".into(),
            kind: "test".into(),
            agent_type: "fake".into(),
            location: "本机".into(),
            max_concurrent: 8,
            role: Role::General,
            name: "模板假 agent".into(),
            description: String::new(),
            tools: vec![],
            enabled: true,
            disabled_reason: None,
        },
        hold,
        bad_branch,
        runs: AtomicUsize::new(0),
    });
    registry.register(agent);
    let llm = LlmClient::new(LlmConfig {
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "none".into(),
        max_retries: 0,
        ..Default::default()
    })
    .unwrap();
    let templates = Arc::new(builtin_templates::builtin_registry(4).expect("内置模板"));
    let orch = Arc::new(Orchestrator::new(
        store.clone(),
        bus,
        registry,
        None,
        Composer::new(llm),
        0,
        Duration::from_secs(30),
        templates,
    ));
    Env { orch, store }
}

fn quote_route(slots_ok: &str, template_id: &str) -> RouteDecision {
    RouteDecision {
        task_class: TaskClass::L1,
        execution_role: Role::General,
        required_tools: vec![],
        risk_level: Risk::Low,
        reason: "RT-TPL".into(),
        template_id: template_id.into(),
        deliverable: "报价单".into(),
        slots_ok: slots_ok.into(),
    }
}

fn auto() -> PolicyVerdict {
    PolicyVerdict {
        auto_execute: true,
        needs_review: false,
        notes: vec!["测试".into()],
        candidates: vec![],
    }
}

async fn wait_status(store: &Store, id: &str, want: &[TaskStatus], secs: u64) -> TaskStatus {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = store.get_task(id).unwrap().unwrap().status;
        if want.contains(&s) {
            return s;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "等待 {want:?} 超时，当前 {s:?} id={id}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_parent_done(store: &Store, parent_id: &str) -> TaskRecord {
    wait_status(
        store,
        parent_id,
        &[
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ],
        15,
    )
    .await;
    store.get_task(parent_id).unwrap().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_tpl_11_split_creates_parent_and_branches() {
    let e = env(Duration::from_millis(30), None);
    let conv = e.store.create_conversation("tpl11").unwrap().id;
    let parent = e
        .orch
        .submit(
            &conv,
            "做报价单",
            "序列号 SN-1",
            quote_route("yes", "quote"),
            auto(),
        )
        .unwrap();
    assert_eq!(parent.fanout_kind.as_deref(), Some("root"));
    assert_eq!(parent.template_id.as_deref(), Some("quote"));
    // 等三路分支落库
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let children = loop {
        let c = e.store.list_children(&parent.id).unwrap();
        let branches: Vec<_> = c
            .iter()
            .filter(|t| t.fanout_kind.as_deref() == Some("branch"))
            .cloned()
            .collect();
        if branches.len() == 3 {
            break branches;
        }
        assert!(std::time::Instant::now() < deadline, "未创建满 3 个 branch");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(children.len(), 3);
    let ids: std::collections::HashSet<_> = children
        .iter()
        .filter_map(|c| c.branch_id.clone())
        .collect();
    assert_eq!(
        ids,
        ["damage", "parts", "history"]
            .into_iter()
            .map(String::from)
            .collect()
    );
    let _ = wait_parent_done(&e.store, &parent.id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_tpl_12_all_ok_join_completes_parent() {
    let e = env(Duration::ZERO, None);
    let conv = e.store.create_conversation("tpl12").unwrap().id;
    let parent = e
        .orch
        .submit(&conv, "报价", "ctx", quote_route("yes", "quote"), auto())
        .unwrap();
    let done = wait_parent_done(&e.store, &parent.id).await;
    assert_eq!(done.status, TaskStatus::Completed);
    let summary = done
        .result
        .as_ref()
        .map(|r| r.summary.as_str())
        .unwrap_or("");
    assert!(summary.contains("damage"), "{summary}");
    assert!(summary.contains("parts"), "{summary}");
    assert!(summary.contains("history"), "{summary}");
    assert!(
        summary.contains("可修") || summary.contains("verdict"),
        "{summary}"
    );
    let children = e.store.list_children(&parent.id).unwrap();
    assert!(
        children
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("join")),
        "应有 join 子任务"
    );
    // Composer 模板交付写入会话（编排侧先交付再 COMPLETED；此处仍等齐，断言不弱化）
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let msgs = loop {
        let msgs = e.store.list_messages(&conv, None).unwrap();
        if msgs.iter().any(|m| m.role == "assistant") {
            break msgs;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "父任务应有一次交付消息"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        msgs.iter().any(|m| m.role == "assistant"),
        "父任务应有一次交付消息"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_tpl_13_bad_envelope_marked_missing() {
    let e = env(Duration::ZERO, Some("parts"));
    let conv = e.store.create_conversation("tpl13").unwrap().id;
    let parent = e
        .orch
        .submit(
            &conv,
            "报价缺件坏信封",
            "ctx",
            quote_route("yes", "quote"),
            auto(),
        )
        .unwrap();
    let done = wait_parent_done(&e.store, &parent.id).await;
    assert_eq!(done.status, TaskStatus::Completed);
    let summary = done
        .result
        .as_ref()
        .map(|r| r.summary.as_str())
        .unwrap_or("");
    assert!(
        summary.contains("parts")
            && (summary.contains("missing")
                || summary.contains("缺失")
                || summary.contains("信封")),
        "交付应标明 parts 缺失：{summary}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_tpl_14_slots_no_stays_single() {
    let e = env(Duration::ZERO, None);
    let conv = e.store.create_conversation("tpl14").unwrap().id;
    let task = e
        .orch
        .submit(&conv, "整单", "ctx", quote_route("no", "quote"), auto())
        .unwrap();
    assert!(task.fanout_kind.is_none());
    assert!(task.parent_id.is_none());
    let _ = wait_status(
        &e.store,
        &task.id,
        &[TaskStatus::Completed, TaskStatus::Failed],
        10,
    )
    .await;
    assert!(e.store.list_children(&task.id).unwrap().is_empty());

    let unknown = e
        .orch
        .submit(
            &conv,
            "未注册",
            "ctx",
            quote_route("yes", "no_such_template"),
            auto(),
        )
        .unwrap();
    assert!(unknown.fanout_kind.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_tpl_15_cancel_parent_cancels_active_children() {
    let e = env(Duration::from_secs(60), None);
    let conv = e.store.create_conversation("tpl15").unwrap().id;
    let parent = e
        .orch
        .submit(
            &conv,
            "取消测试",
            "ctx",
            quote_route("yes", "quote"),
            auto(),
        )
        .unwrap();
    // 等到至少有一个 branch 在跑
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let kids = e.store.list_children(&parent.id).unwrap();
        if kids
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("branch") && !c.status.is_terminal())
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "分支未启动");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(e.orch.cancel(&parent.id).unwrap());
    let p = e.store.get_task(&parent.id).unwrap().unwrap();
    assert_eq!(p.status, TaskStatus::Cancelled);
    let kids = e.store.list_children(&parent.id).unwrap();
    for c in &kids {
        if c.fanout_kind.as_deref() == Some("branch") {
            assert_eq!(c.status, TaskStatus::Cancelled, "{}", c.id);
        }
    }
    assert!(
        !kids
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("join")),
        "取消后不应新建 join"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_tpl_16_needs_human_blocks_join_until_accept() {
    // 不走 spawn：直接落库 parent + 2 完成分支 + 1 待验收分支，验收后应汇合
    let e = env(Duration::ZERO, None);
    let conv = e.store.create_conversation("tpl16").unwrap().id;
    let ts = now();
    let parent = TaskRecord {
        id: an_core::new_id("task"),
        conversation_id: conv.clone(),
        goal: "验收阻塞汇合".into(),
        role: Role::General,
        status: TaskStatus::Running,
        route: quote_route("yes", "quote"),
        policy: auto(),
        result: None,
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: Some("root".into()),
        branch_id: None,
        template_id: Some("quote".into()),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };
    e.store.insert_task(&parent, "ctx").unwrap();

    let mk_branch = |bid: &str, summary: &str| {
        let ts = now();
        TaskRecord {
            id: an_core::new_id("task"),
            conversation_id: conv.clone(),
            goal: format!("{} · 分支 {bid}", parent.goal),
            role: Role::General,
            status: TaskStatus::Created,
            route: quote_route("yes", "quote"),
            policy: auto(),
            result: Some(TaskResult {
                status: WorkerStatus::Succeeded,
                summary: summary.into(),
                artifacts: vec![],
                evidence: vec![],
                errors: vec![],
            }),
            review: None,
            assignment: None,
            parent_id: Some(parent.id.clone()),
            fanout_kind: Some("branch".into()),
            branch_id: Some(bid.into()),
            template_id: Some("quote".into()),
            created_at: ts.clone(),
            updated_at: ts,
        }
    };

    let d = mk_branch(
        "damage",
        r#"{"branch_id":"damage","status":"ok","missing":"","payload":{"verdict":"可修","hours":"6"}}"#,
    );
    let p = mk_branch(
        "parts",
        r#"{"branch_id":"parts","status":"ok","missing":"","payload":{"shortage_amount":"1200"}}"#,
    );
    let h = mk_branch(
        "history",
        r#"{"branch_id":"history","status":"ok","missing":"","payload":{"warranty_risk":"低"}}"#,
    );
    for (t, to) in [
        (&d, TaskStatus::Completed),
        (&p, TaskStatus::Completed),
        (&h, TaskStatus::NeedsHuman),
    ] {
        e.store.insert_task(t, "").unwrap();
        assert!(
            e.store
                .finish_task(&t.id, &[TaskStatus::Created], to, t.result.as_ref())
                .unwrap(),
            "未能将 {} 置为 {to:?}",
            t.branch_id.as_deref().unwrap_or("?")
        );
    }

    // 待验收期间不得有 join
    tokio::time::sleep(Duration::from_millis(100)).await;
    let before = e.store.list_children(&parent.id).unwrap();
    assert!(
        !before
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("join")),
        "NEEDS_HUMAN 时不应启动 join"
    );

    e.orch
        .human_review_verdict(&h.id, true, "验收通过")
        .await
        .unwrap();
    let done = wait_parent_done(&e.store, &parent.id).await;
    assert_eq!(done.status, TaskStatus::Completed);
    let after = e.store.list_children(&parent.id).unwrap();
    assert!(
        after
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("join")),
        "验收后应汇合"
    );
}

#[test]
fn builtin_registry_loads_three_templates() {
    let reg = builtin_templates::builtin_registry(4).unwrap();
    assert!(reg.get("quote").is_some());
    assert!(reg.get("stop_line").is_some());
    assert!(reg.get("release").is_some());
    let _empty = TemplateRegistry::default();
}
