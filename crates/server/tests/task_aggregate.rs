//! RT-SAGG（#397 P1，S-Agent 整合规则 4）：跨任务聚合只读投影。
//! 逐条带来源（conversation_id + task_id + updated_at），不做产物改写；
//! 未带执行者身份前与既有任务查询同口径（A7 落地时统一过滤）。

use std::path::Path;
use std::sync::Arc;

use an_core::{
    PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskResult, TaskStatus, WorkerStatus,
};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::Value;

async fn serve(rt: Arc<Runtime>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, an_server::api(rt)).await.unwrap() });
    format!("http://{addr}")
}

fn runtime() -> Arc<Runtime> {
    Arc::new(
        Runtime::new(
            AppConfig::parse(
                r#"
[llm]
base_url = "http://127.0.0.1:9/v1"
model = "m"
max_retries = 0
[agents.general]
enabled = false
[agents.coding]
enabled = false
"#,
            )
            .unwrap()
            .0,
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    )
}

fn seed(rt: &Runtime, id: &str, goal: &str, summary: Option<&str>) {
    let t = an_core::TaskRecord {
        id: id.into(),
        conversation_id: "conv-agg".into(),
        goal: goal.into(),
        role: Role::General,
        status: TaskStatus::Completed,
        route: RouteDecision {
            task_class: TaskClass::L1,
            execution_role: Role::General,
            required_tools: vec![],
            risk_level: Risk::Low,
            reason: String::new(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        },
        result: summary.map(|smm| TaskResult {
            status: WorkerStatus::Succeeded,
            summary: smm.into(),
            artifacts: vec![an_core::Artifact {
                kind: "file".into(),
                name: format!("{id}-report.md"),
                content: "正文".into(),
            }],
            evidence: vec![an_core::Evidence {
                kind: "tool_call".into(),
                content: "write_file 写入完成".into(),
            }],
            errors: vec![],
        }),
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: an_core::now(),
        updated_at: an_core::now(),
    };
    rt.store.insert_task(&t, "", None).unwrap();
}

/// RT-SAGG-01：多任务聚合含逐条来源链接与产物名
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_sagg_01_aggregates_multiple_tasks_with_sources() {
    let rt = runtime();
    seed(&rt, "task-agg-1", "整理产线结论", Some("产线结论摘要"));
    seed(&rt, "task-agg-2", "统计 CPK", Some("CPK 统计完成"));
    let base = serve(rt.clone()).await;
    let r = reqwest::Client::new()
        .get(format!(
            "{base}/api/desktop/v1/tasks/aggregate?ids=task-agg-1,task-agg-2"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    for (id, goal, summary) in [
        ("task-agg-1", "整理产线结论", "产线结论摘要"),
        ("task-agg-2", "统计 CPK", "CPK 统计完成"),
    ] {
        let it = items.iter().find(|x| x["task_id"] == id).unwrap();
        assert_eq!(it["conversation_id"], "conv-agg", "来源链接（会话）");
        assert_eq!(it["goal"], goal);
        assert_eq!(it["summary"], summary, "聚合含结果摘要");
        assert_eq!(
            it["artifacts"][0]["name"],
            format!("{id}-report.md"),
            "产物名逐条可链接"
        );
        assert_eq!(it["evidence_count"], 1);
        assert!(it["updated_at"].is_string());
    }
    assert!(v["missing"].as_array().unwrap().is_empty());
}

/// RT-SAGG-02：不存在的 id 进 missing，不影响其余条目
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_sagg_02_missing_ids_reported_without_blocking_others() {
    let rt = runtime();
    seed(&rt, "task-agg-3", "有结果的任务", Some("完成"));
    let base = serve(rt).await;
    let r = reqwest::Client::new()
        .get(format!(
            "{base}/api/desktop/v1/tasks/aggregate?ids=task-agg-3,no-such-task"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["missing"][0], "no-such-task");
}

/// RT-SAGG-03：空 ids / 超上限 → 400 invalid_request（只读投影的输入守卫）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_sagg_03_empty_and_oversized_ids_rejected() {
    let rt = runtime();
    let base = serve(rt).await;
    let client = reqwest::Client::new();
    let r = client
        .get(format!("{base}/api/desktop/v1/tasks/aggregate?ids="))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["code"], "invalid_request");
    let ids = (0..51)
        .map(|i| format!("t{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let r = client
        .get(format!("{base}/api/desktop/v1/tasks/aggregate?ids={ids}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400, "51 个 id 应超上限被拒");
}
