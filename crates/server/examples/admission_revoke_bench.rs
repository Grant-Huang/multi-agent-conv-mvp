//! E3 Q4：撤权实测。用正式的编排器、registry、准入账本和 HTTP 外部 agent 适配器，
//! 在执行组（主力 ext1 + 备份 ext2，均为外部 agent）上反复做"主力执行长任务时撤权"，记录：
//! - 从下令撤权到进行中任务变为 FAILED 的耗时（毫秒）；
//! - 撤权报告是否列出该任务；被撤权任务是否被交付（应为否）；
//! - 撤权后的新任务是否由备份完成；
//! - 主力被撤权后交回的结果是否被接受（应为否）。
//!
//! 不需要大模型：外部 agent 是本进程内的 HTTP 桩服务。
//! 用法：cargo run -p an-server --example admission_revoke_bench -- [轮数，默认 50] [输出.json]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use an_agents::a2a::{A2aHttpAgent, A2aSpec};
use an_core::{PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskStatus};
use an_llm::{LlmClient, LlmConfig};
use an_runtime::admission::*;
use an_runtime::bus::Bus;
use an_runtime::composer::Composer;
use an_runtime::orchestrator::Orchestrator;
use an_runtime::registry::AgentRegistry;
use an_runtime::store::Store;
use anyhow::Result;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

/// 启动一个桩外部 agent：每次执行等待 `hold`，返回带自己名字的结果
async fn stub(name: &'static str, hold: Duration) -> Result<String> {
    let app = Router::new().route(
        "/run",
        post(move || async move {
            tokio::time::sleep(hold).await;
            Json(json!({"status": "succeeded", "summary": format!("{name} 完成")}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(format!("http://{addr}"))
}

fn admit(l: &AdmissionLedger, id: &str) -> Result<()> {
    let mut caps = BTreeMap::new();
    caps.insert("read_context".to_string(), true);
    l.submit(AgentClaim {
        agent_id: id.into(),
        name: id.into(),
        description: "撤权实测".into(),
        capabilities: caps,
        requested_level: TrustLevel::Member,
        max_subtask_depth: None,
        working_language: Some("zh".into()),
    })?;
    l.clarify(id, vec![])?;
    l.score(
        id,
        Scores {
            completeness: 1.0,
            conversation: 1.0,
            test: 1.0,
        },
    )?;
    l.contract(
        id,
        Contract {
            role: "executor".into(),
            working_language: "zh".into(),
            context_scope: vec![],
            allowed_tools: vec![],
        },
    )?;
    l.activate(id)?;
    Ok(())
}

fn route() -> RouteDecision {
    RouteDecision {
        task_class: TaskClass::L1,
        execution_role: Role::General,
        required_tools: vec![],
        risk_level: Risk::Low,
        reason: String::new(),
    }
}

async fn wait_terminal(store: &Store, id: &str, limit: Duration) -> Option<TaskStatus> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let s = store.get_task(id).ok()??.status;
        if matches!(
            s,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        ) {
            return Some(s);
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    None
}

async fn wait_running(store: &Store, id: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if store.get_task(id).ok().flatten().map(|t| t.status) == Some(TaskStatus::Running) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    false
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let rounds: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let primary_url = stub("ext1", Duration::from_secs(2)).await?;
    let backup_url = stub("ext2", Duration::from_millis(20)).await?;
    let mut rows = vec![];
    for round in 0..rounds {
        let store = Arc::new(Store::open(Path::new(":memory:"))?);
        let bus = Arc::new(Bus::new(store.clone()));
        let mut assign = BTreeMap::new();
        assign.insert(
            "general".to_string(),
            vec!["ext1".to_string(), "ext2".to_string()],
        );
        let registry = AgentRegistry::new(assign);
        let ledger = Arc::new(AdmissionLedger::open(
            store.clone(),
            AdmissionConfig::default(),
        )?);
        registry.set_admission(ledger.clone());
        for (id, url) in [("ext1", &primary_url), ("ext2", &backup_url)] {
            registry.register(Arc::new(A2aHttpAgent::new(A2aSpec {
                id: id.into(),
                name: id.into(),
                role: Role::General,
                agent_type: "ext".into(),
                endpoint: url.clone(),
                max_concurrent: 1,
                timeout: Duration::from_secs(30),
            })?));
            admit(&ledger, id)?;
        }
        let llm = LlmClient::new(LlmConfig {
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "none".into(),
            max_retries: 0,
            ..Default::default()
        })?;
        let orch = Arc::new(Orchestrator::new(
            store.clone(),
            bus,
            registry.clone(),
            None,
            Composer::new(llm),
            0,
            Duration::from_secs(30),
        ));
        orch.set_admission(ledger.clone());
        let conv = store.create_conversation("bench")?.id;
        let policy = PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        };
        let a = orch.submit(&conv, "长任务", "", route(), policy.clone())?;
        let running = wait_running(&store, &a.id).await;
        let t0 = Instant::now();
        let report = orch.revoke_agent("ext1", "实测")?;
        let st = wait_terminal(&store, &a.id, Duration::from_secs(5)).await;
        let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
        // 等主力原本的执行时长过去，确认它交回的结果没有被接受、也没有交付
        tokio::time::sleep(Duration::from_millis(2_100)).await;
        let final_a = store.get_task(&a.id)?.map(|t| t.status);
        let delivered_a = store
            .list_messages(&conv, None)?
            .iter()
            .filter(|m| m.role == "assistant" && m.task_id.as_deref() == Some(&a.id))
            .count();
        let b = orch.submit(&conv, "下一个", "", route(), policy)?;
        let st_b = wait_terminal(&store, &b.id, Duration::from_secs(5)).await;
        let by_backup = store
            .get_task(&b.id)?
            .and_then(|t| t.result)
            .is_some_and(|r| r.summary.contains("ext2"));
        rows.push(json!({
            "round": round, "was_running": running,
            "revoke_to_failed_ms": latency_ms,
            "status_after_revoke": st.map(|s| s.as_str()),
            "final_status": final_a.map(|s| s.as_str()),
            "listed_in_report": report.aborted.contains(&a.id),
            "revoked_task_delivered": delivered_a,
            "next_task_status": st_b.map(|s| s.as_str()),
            "next_task_by_backup": by_backup,
            "new_epoch": report.epoch,
        }));
        eprintln!("round {round}: {latency_ms:.2} ms");
    }
    let mut lat: Vec<f64> = rows
        .iter()
        .filter_map(|r| r["revoke_to_failed_ms"].as_f64())
        .collect();
    lat.sort_by(f64::total_cmp);
    let pct = |p: f64| lat[((lat.len() as f64 - 1.0) * p).round() as usize];
    let count = |f: &dyn Fn(&serde_json::Value) -> bool| rows.iter().filter(|r| f(r)).count();
    let summary = json!({
        "rounds": rounds,
        "latency_ms": {"p50": pct(0.5), "p95": pct(0.95), "max": pct(1.0)},
        "aborted_in_report": count(&|r| r["listed_in_report"] == true),
        "failed_with_revocation": count(&|r| r["final_status"] == "FAILED"),
        "revoked_task_delivered": count(&|r| r["revoked_task_delivered"] != 0),
        "next_task_by_backup": count(&|r| r["next_task_by_backup"] == true),
    });
    let out = json!({"summary": summary, "rows": rows});
    let text = serde_json::to_string_pretty(&out)?;
    match args.get(2) {
        Some(p) => std::fs::write(p, text)?,
        None => println!("{}", serde_json::to_string_pretty(&out["summary"])?),
    }
    Ok(())
}
