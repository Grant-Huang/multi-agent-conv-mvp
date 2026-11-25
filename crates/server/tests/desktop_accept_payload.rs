//! RT-ADACCEPT-08：HTTP 完整载荷冲突不能重复写消息。
use std::sync::Arc;
use std::time::Duration;

use an_runtime::{config::AppConfig, store::Store, Runtime};
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_adaccept_08_http_rejects_changed_payload() {
    let cfg: AppConfig = toml::from_str(
        "[llm]\nbase_url='http://127.0.0.1:9/v1'\nmodel='test'\nmax_retries=0\n\
         timeout_secs=1\n[agents.general]\nenabled=false\n[agents.coding]\nenabled=false\n",
    )
    .unwrap();
    let rt = Arc::new(
        Runtime::new(cfg, Store::open(std::path::Path::new(":memory:")).unwrap()).unwrap(),
    );
    let a = rt.store.create_conversation("A").unwrap().id;
    let b = rt.store.create_conversation("B").unwrap().id;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = an_server::api(rt.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // 作用域退出（含断言 panic）时也停止自己的服务。
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _stop = Stop(server);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let original =
        json!({"client_operation_id":"op-http", "text":"查询 SMT-03", "scope":"continue"});
    let first = http
        .post(format!(
            "http://{addr}/api/desktop/v1/conversations/{a}/requests"
        ))
        .json(&original)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status().as_u16(), 202);
    let first: Value = first.json().await.unwrap();
    for (conv, scope, task) in [
        (b.as_str(), "continue", None),
        (a.as_str(), "new", None),
        (a.as_str(), "continue", Some("other-task")),
    ] {
        let body = json!({"client_operation_id":"op-http", "text":"查询 SMT-03", "scope":scope, "task_id":task});
        let response = http
            .post(format!(
                "http://{addr}/api/desktop/v1/conversations/{conv}/requests"
            ))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 409, "不同载荷必须 HTTP 409");
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["code"], "operation_conflict");
        assert_eq!(value["request_id"], first["request_id"]);
        assert_eq!(value["message_id"], first["message_id"]);
        assert!(!value.to_string().contains("查询 SMT-03"));
    }
    let replay = http
        .post(format!(
            "http://{addr}/api/desktop/v1/conversations/{a}/requests"
        ))
        .json(&original)
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status().as_u16(), 202);
    let replay: Value = replay.json().await.unwrap();
    assert_eq!(replay["request_id"], first["request_id"]);
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["dispatch_count"], 1);
    let messages = rt.store.list_messages(&a, None).unwrap();
    assert_eq!(messages.iter().filter(|m| m.role == "user").count(), 1);
    assert!(rt.store.list_messages(&b, None).unwrap().is_empty());
}
