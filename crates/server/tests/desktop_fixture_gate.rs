//! RT-FIXC（T-95）：夹具门控与事件写入去重。用例见 docs/testing/CASES.md。
//!
//! H4：Resume 夹具事件编号写死 → 第二次 seed 主键冲突，同批真实事件丢失；
//! 夹具接口在生产环境也可调用。验收见 docs/decisions/2026-11-24-review-h1-h4-fix-plan.md 任务卡 FIX-C。

use std::sync::Arc;
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn config(test_fixtures: bool) -> AppConfig {
    toml::from_str(&format!(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n\
         [desktop]\ntest_fixtures = {test_fixtures}\n"
    ))
    .unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    /// SSE 订阅只断言状态码，不消费流
    async fn get_status(&self, path: &str) -> u16 {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        r.status().as_u16()
    }
}

async fn boot(test_fixtures: bool) -> (Api, Arc<Runtime>) {
    let dir = std::env::temp_dir().join(an_core::new_id("an-desktop-fixture-gate"));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = Arc::new(
        Runtime::new(
            config(test_fixtures),
            Store::open(&dir.join("an.db")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (api, rt)
}

/// RT-FIXC-01：两次 seed 各自成立——第二个夹具会话用自己的事件与 after_cursor 可回放（H4 主断言）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_fixc_01_seed_twice_both_conversations_replayable() {
    let (api, rt) = boot(true).await;
    let (s1, v1) = api
        .post("/api/desktop/v1/resume/fixture/seed", json!({}))
        .await;
    assert_eq!(s1, 200, "{v1}");
    let cid1 = v1["conversation_id"].as_str().unwrap().to_string();
    let after1 = v1["after_cursor"].as_str().unwrap().to_string();

    let (s2, v2) = api
        .post("/api/desktop/v1/resume/fixture/seed", json!({}))
        .await;
    assert_eq!(s2, 200, "{v2}");
    let cid2 = v2["conversation_id"].as_str().unwrap().to_string();
    let after2 = v2["after_cursor"].as_str().unwrap().to_string();

    assert_ne!(cid1, cid2, "两次 seed 应产生两个会话");
    assert_ne!(after1, after2, "事件编号应每次生成，after_cursor 不得写死");

    // 各自会话的事件全部落库可回放（修复前：第二次 seed 主键冲突，第二会话事件丢失）
    let own1 = rt
        .store
        .list_conversation_events_after(&cid1, None)
        .unwrap();
    let own2 = rt
        .store
        .list_conversation_events_after(&cid2, None)
        .unwrap();
    match own1 {
        an_runtime::store::DesktopEventsAfter::Ok(items) => {
            assert_eq!(items.len(), 2, "第一个夹具会话应有 2 条事件");
        }
        other => panic!("第一个夹具会话事件不可回放: {other:?}"),
    }
    match own2 {
        an_runtime::store::DesktopEventsAfter::Ok(items) => {
            assert_eq!(
                items.len(),
                2,
                "第二个夹具会话应有自己的 2 条事件（H4：不得因编号冲突丢失）"
            );
            assert!(
                items.iter().all(|i| i.event.conversation_id == cid2),
                "事件归属错误的会话"
            );
        }
        other => panic!("第二个夹具会话事件不可回放: {other:?}"),
    }

    // 第二个夹具会话用自己的 after_cursor 订阅返回 200
    let st = api
        .get_status(&format!(
            "/api/desktop/v1/events?conversation_id={cid2}&after={after2}"
        ))
        .await;
    assert_eq!(
        st, 200,
        "第二个夹具会话用自己的 after_cursor 订阅应返回 200"
    );
}

/// RT-FIXC-02：开关关闭时夹具接口不挂载（生产环境 404）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_fixc_02_gate_closed_fixture_routes_404() {
    let (api, _rt) = boot(false).await;
    for path in [
        "/api/desktop/v1/resume/fixture/seed",
        "/api/desktop/v1/resume/fixture/replay-a",
        "/api/desktop/v1/resume/fixture/continue-b",
    ] {
        let (s, v) = api.post(path, json!({ "conversation_id": "conv-x" })).await;
        assert_eq!(
            s, 404,
            "夹具接口应在 test_fixtures=false 时关闭: {path} → {v}"
        );
    }
}
