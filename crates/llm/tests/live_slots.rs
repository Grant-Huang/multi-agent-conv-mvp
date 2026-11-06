//! 真实 llama-server 上验证槽位调度（默认不跑，需要本机 8200 的 35B 以 `-np 6` 运行）：
//! `cargo test -p an-llm --test live_slots -- --ignored --nocapture`
//! 地址可用环境变量 `LLAMA_URL` 覆盖（默认 `http://127.0.0.1:8200/v1`）。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use an_llm::{Backend, BackendConfig, BackendKind, ChatMessage, LlmClient, LlmConfig, Priority};
use serde_json::Value;

fn base_url() -> String {
    std::env::var("LLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:8200/v1".into())
}

async fn processing_slots(http: &reqwest::Client, base: &str) -> BTreeSet<u64> {
    let url = format!("{}/slots", base.trim_end_matches("/v1"));
    let Ok(resp) = http.get(url).send().await else {
        return BTreeSet::new();
    };
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|s| s["is_processing"] == true)
                .filter_map(|s| s["id"].as_u64())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore]
async fn requests_land_on_the_slots_the_scheduler_assigned() {
    let base = base_url();
    let backend = Arc::new(Backend::new(
        "local",
        BackendConfig {
            kind: BackendKind::LlamaCpp,
            base_url: base.clone(),
            slots: 6,
            interactive_slots: vec![0, 1],
            background_slots: vec![2, 3, 4, 5],
            ..Default::default()
        },
    ));
    let cfg = LlmConfig {
        base_url: base.clone(),
        model: "x".into(),
        max_tokens: Some(300),
        timeout_secs: 120,
        ..Default::default()
    };
    let make = |role: &str, p: Priority| {
        LlmClient::new(cfg.clone())
            .unwrap()
            .with_backend(backend.clone(), role, p)
    };
    let prompt = [ChatMessage::user(
        "请写一段 400 字左右的关于咖啡历史的介绍。",
    )];

    let mut handles = vec![];
    let front = make("conversation", Priority::Interactive);
    let p = prompt.clone();
    handles.push(tokio::spawn(async move { front.chat(&p).await }));
    for _ in 0..4 {
        let c = make("general", Priority::Background);
        let p = prompt.clone();
        handles.push(tokio::spawn(async move { c.chat(&p).await }));
    }

    // 生成期间不断读取 /slots，记录出现过忙碌的槽位
    let http = reqwest::Client::new();
    let mut seen = BTreeSet::new();
    for _ in 0..80 {
        seen.extend(processing_slots(&http, &base).await);
        if handles.iter().all(|h| h.is_finished()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for h in handles {
        let text = h.await.unwrap().expect("调用失败");
        assert!(!text.trim().is_empty());
    }
    println!("生成期间出现过的忙碌槽位: {seen:?}");
    assert!(seen.contains(&0), "前台请求应该在槽位 0：{seen:?}");
    assert!(
        !seen.contains(&1),
        "槽位 1 是前台备用槽位，这次没有第二个前台请求，不应被使用：{seen:?}"
    );
    assert!(
        seen.iter().filter(|s| **s >= 2).count() >= 3,
        "后台请求应该落在槽位 2~5：{seen:?}"
    );
    // 全部归还，服务端也空闲
    assert!(backend
        .sched
        .snapshot()
        .slots
        .iter()
        .all(|s| s.holder.is_none()));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        processing_slots(&http, &base).await.is_empty(),
        "调用结束后服务端不应再有忙碌的槽位"
    );
    let recs = backend.stats.recent();
    println!(
        "{:?}",
        recs.iter()
            .map(|r| (r.role.clone(), r.slot, r.total_ms))
            .collect::<Vec<_>>()
    );
}
