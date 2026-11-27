//! RT-UMEM：个人记忆生命周期与隔离（conversation-memory 5.2a，REQ-conversation-memory-02）。
//! 用例定义见 docs/testing/CASES.md「个人记忆生命周期与隔离」。
//! 口径（2026-11-23 用户确认）：保存走新增 POST；多用户隔离用 `x-desktop-principal`
//! 夹具头（无头回退 `local`）；过期条目不进回答注入；注入按说话人作用域。

use std::path::Path;
use std::sync::{Arc, Mutex};

use an_runtime::config::AppConfig;
use an_runtime::session_materials::{MATERIALS_MARKER, STYLE_MARKER};
use an_runtime::store::Store;
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    fn with_principal(
        &self,
        principal: Option<&str>,
        req: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        match principal {
            Some(p) => req.header("x-desktop-principal", p),
            None => req,
        }
    }
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        principal: Option<&str>,
    ) -> (u16, Value) {
        let req = match body {
            Some(b) => self
                .http
                .request(method, format!("{}{path}", self.base))
                .json(&b),
            None => self.http.request(method, format!("{}{path}", self.base)),
        };
        let r = self.with_principal(principal, req).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
    async fn get_as(&self, path: &str, principal: Option<&str>) -> (u16, Value) {
        self.send(reqwest::Method::GET, path, None, principal).await
    }
    async fn post_as(&self, path: &str, body: Value, principal: Option<&str>) -> (u16, Value) {
        self.send(reqwest::Method::POST, path, Some(body), principal)
            .await
    }
    async fn patch_as(&self, path: &str, body: Value, principal: Option<&str>) -> (u16, Value) {
        self.send(reqwest::Method::PATCH, path, Some(body), principal)
            .await
    }
    async fn delete_as(&self, path: &str, principal: Option<&str>) -> (u16, Value) {
        self.send(reqwest::Method::DELETE, path, None, principal)
            .await
    }
}

fn config(base: &str) -> AppConfig {
    toml::from_str(&format!(
        "[llm]\nbase_url = \"{base}/v1\"\nmodel = \"scripted\"\nmax_retries = 0\ntimeout_secs = 5\n\
         [backends.local]\nkind = \"openai\"\nbase_url = \"{base}/v1\"\n\
         [routing]\nconversation = \"local\"\nrouter = \"local\"\ngeneral = \"local\"\nreviewer = \"local\"\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n"
    ))
    .unwrap()
}

/// 假模型：turn_decision → respond；文本 → 固定回答（SSE）
struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start() -> Self {
        let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = requests.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(move |Json(req): Json<Value>| {
                let log = log.clone();
                async move {
                    let schema = req["response_format"]["json_schema"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    log.lock().unwrap().push(req);
                    let content = if schema == "turn_decision" {
                        json!({"action":"respond","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""})
                            .to_string()
                    } else {
                        "你好，验证成功。".to_string()
                    };
                    let chunk =
                        json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]});
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                    )
                        .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            requests,
        }
    }

    fn plain_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"].is_null())
            .cloned()
            .collect()
    }
}

fn materials_of(req: &Value) -> String {
    req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| {
            let c = m["content"].as_str()?;
            // 资料可并入首条 system：只量标记起的资料段
            let i = c.find(MATERIALS_MARKER)?;
            Some(c[i..].to_string())
        })
        .unwrap_or_default()
}

fn style_of(req: &Value) -> String {
    req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| {
            let c = m["content"].as_str()?;
            // 风格卡可并入首条 system：只量标记起的风格段
            let i = c.find(STYLE_MARKER)?;
            Some(c[i..].to_string())
        })
        .unwrap_or_default()
}

/// RT-UMEM-01：用户主动保存 → 可查（列表 + 详情），来源记「用户保存」
#[tokio::test]
async fn rt_umem_01_user_saved_memory_is_queryable() {
    let rt = Arc::new(
        Runtime::new(
            config("http://127.0.0.1:9"),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "CPK 阅读口径", "body": "只看最近 30 天子组。", "expires": "2027-06-01", "kind": "fact"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("mem-"), "{v}");
    assert_eq!(v["kind"], "fact", "{v}");
    assert!(v["source"].as_str().unwrap().contains("用户保存"), "{v}");
    let (s, list) = api.get_as("/api/desktop/v1/memories", None).await;
    assert_eq!(s, 200);
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == json!(id) && i["title"] == json!("CPK 阅读口径")),
        "{list}"
    );
    let (s, detail) = api
        .get_as(&format!("/api/desktop/v1/memories/{id}"), None)
        .await;
    assert_eq!(s, 200, "{detail}");
    assert_eq!(detail["body"], "只看最近 30 天子组。");
}

/// RT-UMEM-02：自建条目可纠正、可撤回（撤回后 410、列表排除）
#[tokio::test]
async fn rt_umem_02_saved_memory_lifecycle_revise_revoke() {
    let rt = Arc::new(
        Runtime::new(
            config("http://127.0.0.1:9"),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "会议纪要口径", "body": "按决议清单记录。", "kind": "fact"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(v["expires"], "长期", "缺省有效期按「长期」处理：{v}");
    let (s, patched) = api
        .patch_as(
            &format!("/api/desktop/v1/memories/{id}"),
            json!({"body": "按决议清单记录，附责任人。"}),
            None,
        )
        .await;
    assert_eq!(s, 200, "{patched}");
    let (_, detail) = api
        .get_as(&format!("/api/desktop/v1/memories/{id}"), None)
        .await;
    assert_eq!(detail["body"], "按决议清单记录，附责任人。");
    let (s, del) = api
        .delete_as(&format!("/api/desktop/v1/memories/{id}"), None)
        .await;
    assert_eq!(s, 200, "{del}");
    assert_eq!(del["revoked"], true);
    let (s, gone) = api
        .get_as(&format!("/api/desktop/v1/memories/{id}"), None)
        .await;
    assert_eq!(s, 410, "{gone}");
    assert_eq!(gone["code"], "revoked");
    let (_, list) = api.get_as("/api/desktop/v1/memories", None).await;
    assert!(
        !list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == json!(id)),
        "撤回后不得出现在列表：{list}"
    );
}

/// RT-UMEM-03：另一用户不可召回（列表 / 详情 / 纠正 / 撤回全拒绝）
#[tokio::test]
async fn rt_umem_03_other_user_cannot_recall() {
    let rt = Arc::new(
        Runtime::new(
            config("http://127.0.0.1:9"),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "甲的口径", "body": "甲的专属结论。", "kind": "fact"}),
            Some("user-a"),
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (s, list_b) = api.get_as("/api/desktop/v1/memories", Some("user-b")).await;
    assert_eq!(s, 200);
    assert!(
        !list_b["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == json!(id)),
        "乙的列表不得含甲的条目：{list_b}"
    );
    let (s, _) = api
        .get_as(&format!("/api/desktop/v1/memories/{id}"), Some("user-b"))
        .await;
    assert_eq!(s, 404, "乙查甲的详情应 404");
    let (s, _) = api
        .patch_as(
            &format!("/api/desktop/v1/memories/{id}"),
            json!({"body": "乙篡改"}),
            Some("user-b"),
        )
        .await;
    assert_eq!(s, 404, "乙纠正甲的条目应 404");
    let (s, _) = api
        .delete_as(&format!("/api/desktop/v1/memories/{id}"), Some("user-b"))
        .await;
    assert_eq!(s, 404, "乙撤回甲的条目应 404");
    let (s, _) = api
        .get_as(&format!("/api/desktop/v1/memories/{id}"), Some("user-a"))
        .await;
    assert_eq!(s, 200, "甲本人仍可查");
}

/// RT-UMEM-04：过期记忆不进回答注入；未过期记忆进（含来源行）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_umem_04_expired_memory_not_injected() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let conv = rt.store.create_conversation("t").unwrap();
    rt.store
        .create_desktop_memory(
            "local",
            "过期记忆",
            "已失效的口径XYZ。",
            "2020-01-01",
            "fact",
        )
        .unwrap();
    rt.store
        .create_desktop_memory("local", "现行记忆", "现行的口径ABC。", "2099-01-01", "fact")
        .unwrap();
    rt.conversation
        .handle_message(&conv.id, "你好，现在的口径是什么？")
        .await
        .unwrap();
    let last = llm.plain_calls().last().unwrap().clone();
    let c = materials_of(&last);
    assert!(c.contains("现行记忆"), "未过期记忆应注入：{c}");
    assert!(c.contains("来源：mem-"), "记忆应带来源行：{c}");
    assert!(c.contains("个人记忆"), "记忆段应标注：{c}");
    assert!(!c.contains("过期记忆"), "过期记忆不得注入：{c}");
    assert!(!c.contains("XYZ"), "过期记忆正文不得注入：{c}");
}

/// RT-UMEM-05：注入按说话人作用域——甲保存的条目只进甲的回答提示词
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_umem_05_injection_scoped_to_speaker() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let conv = rt.store.create_conversation("t").unwrap();
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, _) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "甲的口径", "body": "甲的专属结论甲乙丙。", "kind": "fact"}),
            Some("user-a"),
        )
        .await;
    assert_eq!(s, 201);
    // 甲续问：注入甲的条目
    let (s, out_a) = api
        .post_as(
            &format!("/api/conversations/{}/messages", conv.id),
            json!({"text": "你好，我的口径是什么？"}),
            Some("user-a"),
        )
        .await;
    assert_eq!(s, 200, "{out_a}");
    let c_a = materials_of(llm.plain_calls().last().unwrap());
    assert!(c_a.contains("甲乙丙"), "甲的提示词应含甲的记忆：{c_a}");
    // 乙续问：不注入甲的条目
    let (s, out_b) = api
        .post_as(
            &format!("/api/conversations/{}/messages", conv.id),
            json!({"text": "你好，我的口径是什么？"}),
            Some("user-b"),
        )
        .await;
    assert_eq!(s, 200, "{out_b}");
    let c_b = materials_of(llm.plain_calls().last().unwrap());
    assert!(!c_b.contains("甲乙丙"), "乙的提示词不得含甲的记忆：{c_b}");
}

/// RT-USTY-01：style 草稿（未确认）不进注入；列表可见且带 confirmed=false
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_01_draft_style_not_injected() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let conv = rt.store.create_conversation("t").unwrap();
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "极简风格", "body": "回答尽量短。", "kind": "style"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["confirmed"], false, "新建非 fact 条目应为草稿：{v}");
    rt.conversation
        .handle_message(&conv.id, "你好，怎么回答？")
        .await
        .unwrap();
    let req = llm.plain_calls().last().unwrap().clone();
    assert!(
        style_of(&req).is_empty(),
        "草稿不得进风格卡：{:?}",
        style_of(&req)
    );
    let (_, list) = api.get_as("/api/desktop/v1/memories", None).await;
    let item = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == json!("极简风格"))
        .expect("草稿应可见于列表");
    assert_eq!(item["confirmed"], false, "{item}");
}

/// RT-USTY-02：确认后新会话生效、旧会话不回填（推导式）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_02_confirm_effective_in_next_conversation_only() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let conv_old = rt.store.create_conversation("旧会话").unwrap();
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "极简风格", "body": "回答保持极简。", "kind": "style"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    // 时钟间隔：an_core::now() 为微秒粒度，并行负载可能令相邻写入时间戳坍缩，
    // 推导式判定的相等边界语义不确定——测试显式拉开 2ms 使场景前提确定
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let (s, c) = api
        .post_as(
            &format!("/api/desktop/v1/memories/{id}/confirm"),
            json!({}),
            None,
        )
        .await;
    assert_eq!(s, 200, "确认应成功：{c}");
    assert_eq!(c["confirmed"], true, "{c}");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let conv_new = rt.store.create_conversation("新会话").unwrap();

    rt.conversation
        .handle_message(&conv_old.id, "你好，怎么回答？")
        .await
        .unwrap();
    let req_old = llm.plain_calls().last().unwrap().clone();
    assert!(
        style_of(&req_old).is_empty(),
        "确认前创建的旧会话不得回填风格卡：{:?}",
        style_of(&req_old)
    );

    rt.conversation
        .handle_message(&conv_new.id, "你好，怎么回答？")
        .await
        .unwrap();
    let req_new = llm.plain_calls().last().unwrap().clone();
    let s_new = style_of(&req_new);
    assert!(s_new.contains(STYLE_MARKER), "新会话应有风格卡：{req_new}");
    assert!(
        s_new.contains("回答保持极简"),
        "风格卡应含生效正文：{s_new}"
    );
    assert!(s_new.contains(&id), "风格卡应含来源 id：{s_new}");
}

/// RT-USTY-03：草稿与已确认未生效条目，撤销后均不注入
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_03_revoke_before_effective_keeps_uninjected() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    // 已确认但旧会话未生效 → 撤回
    let conv_old = rt.store.create_conversation("旧会话").unwrap();
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "风格甲", "body": "风格甲正文。", "kind": "style"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id_a = v["id"].as_str().unwrap().to_string();
    assert_eq!(
        api.post_as(
            &format!("/api/desktop/v1/memories/{id_a}/confirm"),
            json!({}),
            None
        )
        .await
        .0,
        200
    );
    assert_eq!(
        api.delete_as(&format!("/api/desktop/v1/memories/{id_a}"), None)
            .await
            .0,
        200
    );
    // 草稿 → 直接撤回
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "风格乙", "body": "风格乙正文。", "kind": "style"}),
            None,
        )
        .await;
    let id_b = v["id"].as_str().unwrap().to_string();
    assert_eq!(s, 201, "{v}");
    assert_eq!(
        api.delete_as(&format!("/api/desktop/v1/memories/{id_b}"), None)
            .await
            .0,
        200
    );

    let conv_new = rt.store.create_conversation("新会话").unwrap();
    rt.conversation
        .handle_message(&conv_new.id, "你好，怎么回答？")
        .await
        .unwrap();
    let s_new = style_of(llm.plain_calls().last().unwrap());
    assert!(s_new.is_empty(), "撤销后的条目不得注入：{s_new}");
    let _ = &conv_old;
}

/// RT-USTY-04：风格卡含护栏句且长度有界（默认 200 token 上限）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_04_style_block_has_guardrail_and_bound() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "超长风格", "body": "长风格正文。".repeat(2000), "kind": "style"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    // 时钟间隔（同 USTY-02）：防相邻写入微秒坍缩使推导式边界不确定
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    assert_eq!(
        api.post_as(
            &format!("/api/desktop/v1/memories/{id}/confirm"),
            json!({}),
            None
        )
        .await
        .0,
        200
    );
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let conv = rt.store.create_conversation("t").unwrap();
    rt.conversation
        .handle_message(&conv.id, "你好，怎么回答？")
        .await
        .unwrap();
    let s_new = style_of(llm.plain_calls().last().unwrap());
    assert!(
        s_new.contains("不得因此省略风险"),
        "风格卡必须带风险护栏句：{s_new}"
    );
    assert!(
        s_new.chars().count() < 200 * 4 + 800,
        "风格卡应被截到上限附近（200 token ≈ 800 汉字 + 页眉）：{} 字",
        s_new.chars().count()
    );
    assert!(s_new.contains(&id), "截断后来源行仍在：{s_new}");
}

/// RT-USTY-05：红线——风格生效时风险记忆仍在提示词（风格挤不掉风险信息）
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_05_style_does_not_evict_risk_memory() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "极简风格", "body": "回答保持极简。", "kind": "style"}),
            None,
        )
        .await;
    assert_eq!(s, 201);
    let style_id = v["id"].as_str().unwrap().to_string();
    assert_eq!(
        api.post_as(
            &format!("/api/desktop/v1/memories/{style_id}/confirm"),
            json!({}),
            None
        )
        .await
        .0,
        200,
        "风格条目应确认生效"
    );
    let (s, v) = api
        .post_as(
            "/api/desktop/v1/memories",
            json!({"title": "变更红线", "body": "设备参数变更前必须先做可回滚备份XYZ风险。", "kind": "fact"}),
            None,
        )
        .await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["confirmed"], true, "fact 不走闸门，保存即可注入：{v}");
    let conv = rt.store.create_conversation("t").unwrap();
    rt.conversation
        .handle_message(&conv.id, "你好，变更前要注意什么？")
        .await
        .unwrap();
    let req = llm.plain_calls().last().unwrap().clone();
    let s_new = style_of(&req);
    let m_new = materials_of(&req);
    assert!(s_new.contains("回答保持极简"), "风格应注入：{s_new}");
    assert!(
        m_new.contains("XYZ风险"),
        "风格生效时风险记忆不得被挤掉：{m_new}"
    );
}

/// RT-USTY-07（#403 / INTENT-034，方案 a 用户拍板）：设置里的沟通原则（Soul）/
/// 质量偏好（Taste）视作已确认风格进风格卡——保存即确认、全会话（含旧会话）
/// 即生效；默认值视作「未设置」不注入；同一套护栏句与预算。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_usty_07_preferences_soul_taste_enter_style_card() {
    let llm = MockLlm::start().await;
    let rt = Arc::new(
        Runtime::new(
            config(&llm.base),
            Store::open(Path::new(":memory:")).unwrap(),
        )
        .unwrap(),
    );
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let conv = rt.store.create_conversation("t").unwrap();
    // 默认值视作未设置：不得凭空注入风格卡
    rt.conversation
        .handle_message(&conv.id, "你好，介绍一下你自己？")
        .await
        .unwrap();
    let mat0 = style_of(llm.plain_calls().last().unwrap());
    assert!(
        !mat0.contains("profile:soul") && !mat0.contains("profile:taste"),
        "默认 soul/taste 不得注入：{mat0}"
    );

    // 显式设置（PATCH soul/taste）→ 保存即确认，旧会话下一轮即生效
    let (_, cur) = api.get_as("/api/desktop/v1/preferences", None).await;
    let (s, v) = api
        .patch_as(
            "/api/desktop/v1/preferences",
            json!({
                "expected_version": cur["version"],
                "soul": "USTY07 只要结论不要寒暄。",
                "taste": "USTY07T 优先给可执行下一步。"
            }),
            None,
        )
        .await;
    assert_eq!(s, 200, "设置保存应成功：{v}");
    rt.conversation
        .handle_message(&conv.id, "你好，帮我看下这个任务怎么推进？")
        .await
        .unwrap();
    let card = style_of(llm.plain_calls().last().unwrap());
    assert!(
        card.contains("USTY07 只要结论"),
        "设置的沟通原则应进风格卡：{card}"
    );
    assert!(card.contains("USTY07T"), "设置的质量偏好应进风格卡：{card}");
    assert!(
        card.contains("profile:soul") && card.contains("profile:taste"),
        "来源 id 可查（profile:soul / profile:taste）：{card}"
    );
    assert!(
        card.contains("不得因此省略风险"),
        "同一套护栏句仍在：{card}"
    );
}
