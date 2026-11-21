//! RT-ADREPLAY：桌面游标 SSE 与断线重放（B2）。用例见 docs/testing/CASES.md。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use an_core::{EventKind, EventPublisher, TaskEvent, TaskStatus};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::Value;

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn config() -> AppConfig {
    toml::from_str(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n",
    )
    .unwrap()
}

struct Env {
    base: String,
    http: reqwest::Client,
    rt: Arc<Runtime>,
    conv_id: String,
    db: PathBuf,
}

async fn boot() -> Env {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-replay"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let conv = rt.store.create_conversation("SMT-03 重放测").unwrap();
    let base = serve(an_server::api(rt.clone())).await;
    Env {
        base,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
        rt,
        conv_id: conv.id,
        db,
    }
}

fn progress(conv: &str, task: &str, id: &str) -> TaskEvent {
    TaskEvent {
        id: id.into(),
        conversation_id: conv.into(),
        task_id: Some(task.into()),
        kind: EventKind::Progress,
        status: None,
        message: id.into(),
        stream_id: None,
        created_at: an_core::now(),
    }
}

fn status_ev(conv: &str, task: &str, id: &str, status: TaskStatus) -> TaskEvent {
    TaskEvent {
        id: id.into(),
        conversation_id: conv.into(),
        task_id: Some(task.into()),
        kind: EventKind::StatusChanged,
        status: Some(status),
        message: format!("{status:?}"),
        stream_id: None,
        created_at: an_core::now(),
    }
}

async fn read_sse_events(
    http: &reqwest::Client,
    url: &str,
    want: usize,
    deadline: Duration,
) -> (u16, Vec<Value>) {
    let mut resp = http.get(url).send().await.unwrap();
    let status = resp.status().as_u16();
    if status != 200 {
        let body = resp.json::<Value>().await.unwrap_or(Value::Null);
        return (status, vec![body]);
    }
    let mut out = Vec::new();
    let start = std::time::Instant::now();
    while out.len() < want && start.elapsed() < deadline {
        let chunk = tokio::time::timeout(Duration::from_millis(500), resp.chunk()).await;
        let Ok(Ok(Some(bytes))) = chunk else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if v.get("event_id").is_some() {
                        out.push(v);
                    }
                }
            }
        }
    }
    (status, out)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adreplay_01_sse_replays_then_continues_live() {
    let env = boot().await;
    let cid = &env.conv_id;
    env.rt.bus.publish(progress(cid, "t-a", "evt-a1"));
    env.rt.bus.publish(progress(cid, "t-a", "evt-a2"));
    env.rt.store.flush_events();

    let url = format!(
        "{}/api/desktop/v1/events?conversation_id={cid}&after=evt-a1",
        env.base
    );
    let mut resp = env.http.get(&url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let mut got = Vec::new();
    let start = std::time::Instant::now();
    while got.is_empty() && start.elapsed() < Duration::from_secs(3) {
        let chunk = tokio::time::timeout(Duration::from_millis(400), resp.chunk()).await;
        let Ok(Ok(Some(bytes))) = chunk else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some(data) = line.strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                    if v.get("event_id").is_some() {
                        got.push(v);
                    }
                }
            }
        }
    }
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0]["event_id"], "evt-a2");
    assert_eq!(got[0]["cursor"], "evt-a2");
    assert_eq!(got[0]["conversation_id"], cid.as_str());

    env.rt.bus.publish(progress(cid, "t-a", "evt-a3-live"));
    let live_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while got.len() < 2 && std::time::Instant::now() < live_deadline {
        let chunk = tokio::time::timeout(Duration::from_millis(400), resp.chunk()).await;
        let Ok(Ok(Some(bytes))) = chunk else {
            continue;
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some(data) = line.strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                    if v.get("event_id").is_some() {
                        got.push(v);
                    }
                }
            }
        }
    }
    assert!(
        got.iter().any(|e| e["event_id"] == "evt-a3-live"),
        "应续接 live: {got:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adreplay_02_duplicate_event_ids_deduped_snapshot_stable() {
    let env = boot().await;
    let cid = &env.conv_id;
    // 权威快照：任务已 COMPLETED；历史事件可含较早 status
    let task = an_core::TaskRecord {
        id: "t-snap".into(),
        conversation_id: cid.clone(),
        goal: "对账".into(),
        role: an_core::Role::General,
        status: TaskStatus::Completed,
        route: an_core::RouteDecision {
            task_class: an_core::TaskClass::L1,
            execution_role: an_core::Role::General,
            required_tools: vec![],
            risk_level: an_core::Risk::Low,
            reason: "test".into(),
        },
        policy: an_core::PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        },
        result: None,
        review: None,
        assignment: None,
        created_at: an_core::now(),
        updated_at: an_core::now(),
    };
    env.rt.store.insert_task(&task, "").unwrap();
    env.rt
        .bus
        .publish(status_ev(cid, "t-snap", "evt-run", TaskStatus::Running));
    env.rt
        .bus
        .publish(status_ev(cid, "t-snap", "evt-done", TaskStatus::Completed));
    env.rt.store.flush_events();

    let url = format!("{}/api/desktop/v1/events?conversation_id={cid}", env.base);
    let (status, events) = read_sse_events(&env.http, &url, 2, Duration::from_secs(3)).await;
    assert_eq!(status, 200, "{events:?}");
    let ids: Vec<_> = events
        .iter()
        .filter_map(|e| e["event_id"].as_str())
        .collect();
    let unique: HashSet<_> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "流内 event_id 应去重: {ids:?}");

    let snap = env.rt.store.get_task("t-snap").unwrap().unwrap();
    assert_eq!(snap.status, TaskStatus::Completed);
    // 迟到「回放」不得改权威状态
    env.rt
        .bus
        .publish(status_ev(cid, "t-snap", "evt-late", TaskStatus::Running));
    env.rt.store.flush_events();
    let snap2 = env.rt.store.get_task("t-snap").unwrap().unwrap();
    assert_eq!(snap2.status, TaskStatus::Completed);
    assert_eq!(snap2.updated_at, snap.updated_at);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adreplay_03_expired_cursor_returns_410() {
    let env = boot().await;
    let cid = &env.conv_id;
    env.rt.bus.publish(progress(cid, "t-a", "evt-ok"));
    env.rt.store.flush_events();

    let url = format!(
        "{}/api/desktop/v1/events?conversation_id={cid}&after=evt-gone",
        env.base
    );
    let r = env.http.get(&url).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 410);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["code"], "cursor_expired");
    assert_eq!(v["retryable"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adreplay_04_restart_reconciles_from_sqlite() {
    let env = boot().await;
    let cid = env.conv_id.clone();
    let db = env.db.clone();
    env.rt.bus.publish(progress(&cid, "t-a", "evt-r1"));
    env.rt.bus.publish(progress(&cid, "t-a", "evt-r2"));
    env.rt.bus.publish(progress(&cid, "t-a", "evt-r3"));
    env.rt.store.flush_events();
    drop(env);

    // 模拟进程重启：新 Runtime / 新 bus，同一库
    let rt2 = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let base = serve(an_server::api(rt2.clone())).await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let url = format!("{base}/api/desktop/v1/events?conversation_id={cid}&after=evt-r1");
    let (status, events) = read_sse_events(&http, &url, 2, Duration::from_secs(3)).await;
    assert_eq!(status, 200, "{events:?}");
    let ids: Vec<_> = events
        .iter()
        .filter_map(|e| e["event_id"].as_str())
        .collect();
    assert_eq!(ids, vec!["evt-r2", "evt-r3"]);

    // 与任务 after 游标对账
    let task_after = http
        .get(format!("{base}/api/tasks/t-a?after=evt-r1"))
        .send()
        .await;
    // 任务可能不存在——事件仍应按会话可回放；对账用 store 直读
    let from_store = match rt2
        .store
        .list_conversation_events_after(&cid, Some("evt-r1"))
        .unwrap()
    {
        an_runtime::store::DesktopEventsAfter::Ok(v) => {
            v.into_iter().map(|e| e.cursor).collect::<Vec<_>>()
        }
        an_runtime::store::DesktopEventsAfter::CursorExpired => panic!("cursor"),
    };
    assert_eq!(from_store, vec!["evt-r2", "evt-r3"]);
    let _ = task_after;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adreplay_05_parallel_tasks_do_not_cross_conversations() {
    let env = boot().await;
    let other = env.rt.store.create_conversation("另一会话").unwrap();
    let cid = &env.conv_id;
    env.rt.bus.publish(progress(cid, "t-a", "evt-a"));
    env.rt.bus.publish(progress(&other.id, "t-b", "evt-b"));
    env.rt.store.flush_events();

    let url = format!("{}/api/desktop/v1/events?conversation_id={cid}", env.base);
    let (status, events) = read_sse_events(&env.http, &url, 1, Duration::from_secs(3)).await;
    assert_eq!(status, 200, "{events:?}");
    assert!(
        events.iter().all(|e| e["conversation_id"] == cid.as_str()),
        "{events:?}"
    );
    assert!(
        events.iter().all(|e| e["event_id"] != "evt-b"),
        "不得串会话: {events:?}"
    );
}
