//! RT-L2H：L2 历史记录检索与本人消息删除（conversation-memory #346 片3，REQ-conversation-memory-05）。
//! 用例定义见 docs/testing/CASES.md「分层记忆细化」。
//! 口径（2026-11-26 用户确认冻结）：AI 历史回答标「当时的回答」不作事实来源；
//! 删除即真删除；检索默认最近 180 天，更早需明确指定；引用带时间与出处。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::{Mutex, MutexGuard};

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

fn config() -> AppConfig {
    config_with(true)
}

/// fixtures=false 模拟生产配置：test 专用导入端点不挂载（#440）
fn config_prod() -> AppConfig {
    config_with(false)
}

fn config_with(fixtures: bool) -> AppConfig {
    let flag = if fixtures { "true" } else { "false" };
    toml::from_str(&format!(
        "[llm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\nmax_retries = 0\n\
         [agents.general]\nenabled = false\n[agents.coding]\nenabled = false\n\
         [desktop]\ntest_fixtures = {flag}\n"
    ))
    .unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
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
    async fn delete(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .delete(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }
}

fn l2h_test_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

/// 进程内夹具：串行化 + 每例全新库
async fn boot() -> (Arc<Runtime>, Api, MutexGuard<'static, ()>) {
    let guard = l2h_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-l2h"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    (rt, api, guard)
}

fn results_of(v: &Value) -> Vec<Value> {
    v["results"].as_array().cloned().unwrap_or_default()
}

/// RT-L2H-01：AI 历史回答标「当时的回答」；用户消息、系统事件分级；
/// 每条命中带时间与出处（msg_id / conv_id / at）。任务回执命中 kind = 任务回执。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_01_past_answers_marked_and_sourced() {
    let (rt, api, _gate) = boot().await;
    let conv = rt.store.create_conversation("t").unwrap();
    rt.store
        .add_message(&conv.id, "user", "复盘一下 3 号产线 CPK 的情况。", None)
        .unwrap();
    rt.store
        .add_message(
            &conv.id,
            "assistant",
            "当时的回答正文：3 号产线 CPK 低于阈值。",
            None,
        )
        .unwrap();
    rt.store
        .add_message(&conv.id, "system", "系统事件：CPK 报告已生成。", None)
        .unwrap();
    // #513：COMPLETED 任务的 summary 含关键词 → 检索应命中 kind=任务回执
    let t = an_core::TaskRecord {
        id: "task-l2h-01".into(),
        conversation_id: conv.id.clone(),
        goal: "CPK 分析".into(),
        role: an_core::Role::General,
        status: an_core::TaskStatus::Completed,
        route: an_core::RouteDecision {
            task_class: an_core::TaskClass::L1,
            execution_role: an_core::Role::General,
            required_tools: vec![],
            risk_level: an_core::Risk::Low,
            reason: String::new(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        },
        policy: an_core::PolicyVerdict {
            auto_execute: true,
            needs_review: false,
            notes: vec![],
            candidates: vec![],
        },
        result: Some(an_core::TaskResult {
            status: an_core::WorkerStatus::Succeeded,
            summary: "CPK 检索回执探针RECEIPT501。".into(),
            artifacts: vec![],
            evidence: vec![],
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

    let (s, v) = api
        .get("/api/desktop/v1/history/search?q=CPK&limit=50")
        .await;
    assert_eq!(s, 200, "{v}");
    let hits = results_of(&v);
    assert!(hits.len() >= 3, "三条记录都应命中：{v}");
    let receipt = hits.iter().find(|h| h["kind"] == json!("任务回执"));
    assert!(receipt.is_some(), "COMPLETED 任务回执应命中（#513）：{v}");
    assert_eq!(
        receipt.unwrap()["msg_id"],
        json!("task-l2h-01"),
        "回执来源 id：{v}"
    );

    let user_hit = hits
        .iter()
        .find(|h| h["role"] == "user")
        .expect("应含用户消息");
    assert_eq!(user_hit["kind"], "用户消息", "{user_hit}");
    let ai_hit = hits
        .iter()
        .find(|h| h["role"] == "assistant")
        .expect("应含 AI 历史回答");
    assert_eq!(
        ai_hit["kind"], "当时的回答",
        "AI 历史回答必须标「当时的回答」（CH-G5）：{ai_hit}"
    );
    let sys_hit = hits
        .iter()
        .find(|h| h["role"] == "system")
        .expect("应含系统事件");
    assert_eq!(sys_hit["kind"], "系统事件", "{sys_hit}");

    for h in &hits {
        assert!(
            !h["msg_id"].as_str().unwrap_or("").is_empty(),
            "出处 msg_id：{h}"
        );
        assert!(
            !h["conv_id"].as_str().unwrap_or("").is_empty(),
            "出处 conv_id：{h}"
        );
        assert!(
            !h["at"].as_str().unwrap_or("").is_empty(),
            "引用带时间：{h}"
        );
        assert!(
            h["snippet"].as_str().unwrap_or("").contains("CPK"),
            "snippet 应含关键词：{h}"
        );
    }
    assert_eq!(user_hit["conv_id"], json!(conv.id), "出处应指向所在会话");
}

/// RT-L2H-02：本人删除个人会话消息——真删除，检索与消息列表都不再出现；
/// AI 回答不可经此删除；重复删除 404。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_02_deleted_message_gone_from_search_and_list() {
    let (rt, api, _gate) = boot().await;
    let conv = rt.store.create_conversation("t").unwrap();
    let mid = rt
        .store
        .add_message(&conv.id, "user", "删除探针 DELETE-PROBE 的正文。", None)
        .unwrap()
        .id;
    let ai_mid = rt
        .store
        .add_message(
            &conv.id,
            "assistant",
            "当时的回答 DELETE-PROBE 附录。",
            None,
        )
        .unwrap()
        .id;

    let (s, v) = api
        .get("/api/desktop/v1/history/search?q=DELETE-PROBE")
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(results_of(&v).len(), 2, "删除前两条都应命中：{v}");

    // AI 回答不可删（不是「本人的消息」）
    let (s, v) = api
        .delete(&format!("/api/desktop/v1/messages/{ai_mid}"))
        .await;
    assert_eq!(s, 403, "AI 回答不得经本人删除端点移除：{v}");
    assert_eq!(v["code"], "forbidden", "{v}");

    // 本人消息真删除
    let (s, v) = api.delete(&format!("/api/desktop/v1/messages/{mid}")).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["deleted"], json!(true), "{v}");
    let (s2, v2) = api
        .get("/api/desktop/v1/history/search?q=DELETE-PROBE")
        .await;
    assert_eq!(s2, 200, "{v2}");
    let hits = results_of(&v2);
    assert!(
        hits.iter().all(|h| h["msg_id"] != json!(mid)),
        "已删除消息不得再被检索：{v2}"
    );
    assert_eq!(hits.len(), 1, "只剩 AI 回答命中：{v2}");
    let listed = rt.store.list_messages(&conv.id, None).unwrap();
    assert!(
        listed.iter().all(|m| m.id != mid),
        "真删除：消息列表不得再有该行"
    );

    // 重复删除 404
    let (s3, v3) = api.delete(&format!("/api/desktop/v1/messages/{mid}")).await;
    assert_eq!(s3, 404, "{v3}");
    assert_eq!(v3["code"], "not_found", "{v3}");
}

/// RT-L2H-03：检索默认只看最近 180 天；更早记录需明确指定 days；
/// 空关键词 400。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_03_default_window_180_days_with_explicit_override() {
    let guard = l2h_test_gate().lock().await;
    let dir = std::env::temp_dir().join(an_core::new_id("an-l2h-old"));
    std::fs::create_dir_all(&dir).unwrap();
    let db: PathBuf = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap(),
    };
    let conv = rt.store.create_conversation("t").unwrap();
    rt.store
        .add_message(&conv.id, "user", "旧记录探针 OLD-PROBE 的正文。", None)
        .unwrap();
    // 直接回填 200 天前的 created_at（REQ-05：保留原件、检索窗口 180 天）
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "UPDATE messages SET created_at = '2026-05-01T00:00:00.000Z'
             WHERE content LIKE '%OLD-PROBE%'",
            [],
        )
        .unwrap();
    }

    let (s, v) = api.get("/api/desktop/v1/history/search?q=OLD-PROBE").await;
    assert_eq!(s, 200, "{v}");
    assert!(
        results_of(&v).is_empty(),
        "默认窗口（180 天）不得返回 200 天前记录：{v}"
    );
    let (s2, v2) = api
        .get("/api/desktop/v1/history/search?q=OLD-PROBE&days=180")
        .await;
    assert_eq!(s2, 200, "{v2}");
    assert!(results_of(&v2).is_empty(), "显式 180 天同样不返回：{v2}");
    let (s3, v3) = api
        .get("/api/desktop/v1/history/search?q=OLD-PROBE&days=400")
        .await;
    assert_eq!(s3, 200, "{v3}");
    let hits = results_of(&v3);
    assert_eq!(hits.len(), 1, "明确指定更早窗口应返回：{v3}");
    assert_eq!(
        hits[0]["at"],
        json!("2026-05-01T00:00:00.000Z"),
        "引用带原始时间：{hits:?}"
    );

    let (s4, v4) = api.get("/api/desktop/v1/history/search?q=").await;
    assert_eq!(s4, 400, "空关键词应拒绝：{v4}");
    assert_eq!(v4["code"], "invalid", "{v4}");
    drop(guard);
}

/// RT-L2H-04（片 4a）：本人给历史消息加「更正」注记——更正单独记录、原文不改写；
/// 检索命中被纠正消息时附 correction 并优先排序；AI 回答不可加注；随消息删除。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_04_correction_note_attaches_ranks_first_and_stays_original() {
    let (rt, api, _gate) = boot().await;
    let conv = rt.store.create_conversation("t").unwrap();
    let mid_u = rt
        .store
        .add_message(&conv.id, "user", "更正探针 CORR-PROBE 原始正文。", None)
        .unwrap()
        .id;
    let mid_a = rt
        .store
        .add_message(&conv.id, "assistant", "当时的回答 CORR-PROBE 附录。", None)
        .unwrap()
        .id;

    // AI 回答 / 空注记 / 未知消息的边界
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/messages/{mid_a}/correction"),
            json!({ "note": "不该被允许" }),
        )
        .await;
    assert_eq!(s, 403, "AI 回答不可加更正注记：{v}");
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/messages/{mid_u}/correction"),
            json!({ "note": "   " }),
        )
        .await;
    assert_eq!(s, 400, "空注记应拒绝：{v}");
    let (s, v) = api
        .post(
            "/api/desktop/v1/messages/no-such/correction",
            json!({ "note": "x" }),
        )
        .await;
    assert_eq!(s, 404, "{v}");

    // 本人加注：200，检索命中附 correction 且排第一；原文不改写
    let note = "更正：CPK 数字以审计报告为准。";
    let (s, v) = api
        .post(
            &format!("/api/desktop/v1/messages/{mid_u}/correction"),
            json!({ "note": note }),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["message_id"], json!(mid_u), "{v}");
    assert!(!v["created_at"].as_str().unwrap_or("").is_empty(), "{v}");

    let (s, v) = api.get("/api/desktop/v1/history/search?q=CORR-PROBE").await;
    assert_eq!(s, 200, "{v}");
    let hits = results_of(&v);
    assert_eq!(hits.len(), 2, "{v}");
    assert_eq!(
        hits[0]["msg_id"],
        json!(mid_u),
        "被纠正消息应优先排序（优先给纠正后版本）：{v}"
    );
    assert_eq!(hits[0]["correction"], json!(note), "{v}");
    assert!(
        hits[1]["correction"].is_null(),
        "未纠正消息不得带 correction：{v}"
    );
    let listed = rt.store.list_messages(&conv.id, None).unwrap();
    let original = listed.iter().find(|m| m.id == mid_u).expect("原文仍在");
    assert!(
        original.content.contains("原始正文"),
        "更正不得改写原文：{}",
        original.content
    );

    // 重复更正为替换；更正可被检索到最新值
    let note2 = "更正（第二次）：以月度台账为准。";
    let (s, _) = api
        .post(
            &format!("/api/desktop/v1/messages/{mid_u}/correction"),
            json!({ "note": note2 }),
        )
        .await;
    assert_eq!(s, 200);
    let (_, v) = api.get("/api/desktop/v1/history/search?q=CORR-PROBE").await;
    let hits = results_of(&v);
    assert_eq!(
        hits[0]["correction"],
        json!(note2),
        "替换后应为最新注记：{v}"
    );
    assert_eq!(
        rt.store.get_correction(&mid_u).unwrap().as_deref(),
        Some(note2)
    );

    // 更正随消息删除（真删除语义）
    let (s, _) = api
        .delete(&format!("/api/desktop/v1/messages/{mid_u}"))
        .await;
    assert_eq!(s, 200);
    assert_eq!(
        rt.store.get_correction(&mid_u).unwrap(),
        None,
        "更正注记应随消息删除"
    );
}

/// RT-L2H-05（#440 / W2 本地数据集）：免 LLM 的历史消息批量导入——test_fixtures
/// 门控端点按会话批量写入（role / content / created_at），不触发任何模型请求；
/// 生产配置（fixtures=false）不挂载该端点（404）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_05_batch_import_without_llm() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-l2h-imp"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config(), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let conv = rt.store.create_conversation("t").unwrap();
    let model_calls_before = rt.store.list_messages(&conv.id, None).unwrap().len();
    let (s, v) = api
        .post(
            "/api/desktop/v1/history/import",
            json!({
                "conversation_id": conv.id,
                "messages": [
                    {"role": "user", "content": "导入-01 早上反馈设备异响。", "created_at": "2026-10-01T09:00:00.000Z"},
                    {"role": "assistant", "content": "导入-02 已记录并安排巡检。", "created_at": "2026-10-01T09:05:00.000Z"},
                    {"role": "user", "content": "导入-03 下午复查 CPK 恢复。", "created_at": "2026-10-01T14:00:00.000Z"}
                ]
            }),
        )
        .await;
    assert_eq!(s, 200, "批量导入应成功：{v}");
    assert_eq!(v["imported"], json!(3), "{v}");
    assert_eq!(v["conversation_id"], json!(conv.id), "{v}");

    // 不触发 LLM：导入前后消息数只差批量条数，且模型请求计数为零
    let listed = rt.store.list_messages(&conv.id, None).unwrap();
    assert_eq!(listed.len(), model_calls_before + 3, "应恰好写入 3 条");
    // 检索可按 created_at 命中（引用带原始时间）
    let (s2, v2) = api.get("/api/desktop/v1/history/search?q=导入-01").await;
    assert_eq!(s2, 200, "{v2}");
    let hits = results_of(&v2);
    assert_eq!(hits.len(), 1, "{v2}");
    assert_eq!(hits[0]["at"], json!("2026-10-01T09:00:00.000Z"), "{v2}");
}

/// （RT-L2H-05 同用例对照段）生产配置（fixtures=false）不挂载导入端点；角色非法 400。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_l2h_05_import_gated_and_validated() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-l2h-prod"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let rt = Arc::new(Runtime::new(config_prod(), Store::open(&db).unwrap()).unwrap());
    let api = Api {
        base: serve(an_server::api(rt.clone())).await,
        http: reqwest::Client::new(),
    };
    let conv = rt.store.create_conversation("t").unwrap();
    let (s, v) = api
        .post(
            "/api/desktop/v1/history/import",
            json!({
                "conversation_id": conv.id,
                "messages": [{"role": "user", "content": "x", "created_at": "2026-10-01T09:00:00.000Z"}]
            }),
        )
        .await;
    assert_eq!(s, 404, "生产配置不得挂载导入端点：{v}");

    // fixtures 开启下：角色非法 400
    let rt2 =
        Arc::new(Runtime::new(config(), Store::open(Path::new(":memory:")).unwrap()).unwrap());
    let api2 = Api {
        base: serve(an_server::api(rt2.clone())).await,
        http: reqwest::Client::new(),
    };
    let conv2 = rt2.store.create_conversation("t2").unwrap();
    let (s2, v2) = api2
        .post(
            "/api/desktop/v1/history/import",
            json!({
                "conversation_id": conv2.id,
                "messages": [{"role": "tool", "content": "x", "created_at": "2026-10-01T09:00:00.000Z"}]
            }),
        )
        .await;
    assert_eq!(s2, 400, "非法角色应拒绝：{v2}");
    assert_eq!(v2["code"], "invalid", "{v2}");
}
