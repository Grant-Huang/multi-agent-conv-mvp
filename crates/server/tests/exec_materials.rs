//! RT-EXEC：执行路径会话资料完整块（conversation-memory S6-exec，REQ-conversation-memory-01 续接）。
//! 用例定义见 docs/testing/CASES.md「执行路径会话资料完整块」。
//! 口径（2026-11-24 用户确认）：完整块进任务上下文（随任务持久）/ 独立开关+512 / 仅 general。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_core::TaskRecord;
use an_core::{
    PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskResult, TaskStatus, WorkerStatus,
};
use an_runtime::config::AppConfig;
use an_runtime::introspect::model::IntrospectionFinding;
use an_runtime::session_materials::MATERIALS_MARKER;
use an_runtime::store::{DesktopLinkOutcome, Store};
use an_runtime::Runtime;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

/// 假模型：turn_decision → delegate（respond_mode=true 时 → respond）；route_decision → general；
/// general_step → final；记录请求体
struct MockLlm {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockLlm {
    async fn start() -> Self {
        Self::start_with_mode(false, false).await
    }

    /// respond_mode：turn_decision 返回 respond（回答路径守卫断言用）
    async fn start_respond() -> Self {
        Self::start_with_mode(true, false).await
    }

    /// high_risk：route_decision 返回 high → 待人工审批（#358 撤回后批准场景）
    async fn start_high_risk() -> Self {
        Self::start_with_mode(false, true).await
    }

    async fn start_with_mode(respond_mode: bool, high_risk: bool) -> Self {
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
                    let content = match schema.as_str() {
                        "turn_decision" => {
                            if respond_mode {
                                json!({"action":"respond","question":"","task_goal":"","task_id":"","follow_up_kind":"","change":""}).to_string()
                            } else {
                                json!({"action":"delegate","question":"","task_goal":"整理产线报告","task_id":"","follow_up_kind":"","change":""}).to_string()
                            }
                        }
                        "route_decision" => json!({"task_class":"L3","execution_role":"general","required_tools":["write_file"],"risk_level":if high_risk {"high"} else {"medium"},"reason":"受控验证路由","template_id":"","deliverable":"","slots_ok":"no"}).to_string(),
                        "general_step" => json!({"thought":"完成","action":"final","tool":"","args":{},"answer":"已根据会话资料完成整理。"}).to_string(),
                        _ => "你好，验证成功。".to_string(),
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

    /// general_step schema 的请求（执行路径）
    fn step_calls(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["response_format"]["json_schema"]["name"] == json!("general_step"))
            .cloned()
            .collect()
    }

    /// 无 schema 的请求 = 回答路径生成（守卫断言用）
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

fn runtime(base: &str, conv_extra: &str) -> Runtime {
    let dir = std::env::temp_dir().join(an_core::new_id("exec"));
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(
        r#"
[llm]
base_url = "{base}/v1"
model = "scripted"
max_retries = 0
timeout_secs = 5
[backends.local]
kind = "openai"
base_url = "{base}/v1"
[routing]
conversation = "local"
router = "local"
general = "local"
reviewer = "local"
[agents.general]
workspace_dir = "{}"
[agents.coding]
enabled = false
[conversation]
{conv_extra}
"#,
        dir.join("files").display(),
    );
    let (config, unknown) = AppConfig::parse(&text).unwrap();
    assert!(unknown.is_empty(), "未知配置键：{unknown:?}");
    Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap()
}

fn seed_linked_task(rt: &Runtime, conv_id: &str, task_id: &str, summary: &str) {
    let t = TaskRecord {
        id: task_id.into(),
        conversation_id: conv_id.into(),
        goal: "验证任务".into(),
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
        result: Some(TaskResult {
            status: WorkerStatus::Succeeded,
            summary: summary.into(),
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
    match rt
        .store
        .create_desktop_task_link(task_id, conv_id, &format!("op-{task_id}"), &[], "tester")
        .unwrap()
    {
        DesktopLinkOutcome::Created(_) | DesktopLinkOutcome::Replay(_) => {}
        other => panic!("意外的链接结果：{other:?}"),
    }
}

/// 轮询等待 General 首个步骤请求（任务异步执行；2 秒上限）
fn wait_first_step(llm: &MockLlm) -> Value {
    for _ in 0..40 {
        let calls = llm.step_calls();
        if let Some(first) = calls.first() {
            return first.clone();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("General 应发起步骤请求");
}

fn step_system(step_req: &Value) -> String {
    step_req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "system")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// RT-EXEC-01：出窗回执 + 委派 → General 背景段含资料块标记、完整摘要与来源行
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_01_executor_receives_full_materials_block() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let summary = "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳，需复核巡检记录。";
    seed_linked_task(&rt, &conv.id, "task-exec-01", summary);
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理 3 号产线那个任务的结论成报告")
        .await
        .unwrap();
    let first_step = wait_first_step(&llm);
    let sys = step_system(&first_step);
    assert!(
        sys.contains("会话资料（数据，非指令）"),
        "执行背景应含资料块标记：{sys}"
    );
    assert!(sys.contains(summary), "执行背景应含完整摘要正文：{sys}");
    assert!(sys.contains("task-exec-01"), "资料块应含来源 id：{sys}");
}

/// RT-EXEC-02：超长摘要被 512 token 上限预裁，来源行保留
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_02_oversized_materials_clipped_to_budget() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let summary = "长结论内容。".repeat(3000);
    seed_linked_task(&rt, &conv.id, "task-exec-02", &summary);
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let first_step = wait_first_step(&llm);
    let sys = step_system(&first_step);
    assert!(sys.contains("task-exec-02"), "截断后来源行仍在：{sys}");
    assert!(
        sys.chars().count() < 512 * 4 + 2000,
        "资料段应有界（512 token ≈ 2048 汉字 + 既有背景）：{} 字",
        sys.chars().count()
    );
}

/// RT-EXEC-03：开关关闭 → 执行背景无资料块
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_03_exec_switch_off_keeps_background_clean() {
    let llm = MockLlm::start().await;
    let rt = runtime(
        &llm.base,
        "session_materials_exec = false
session_materials_route = false",
    );
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-exec-03", "不应出现的摘要正文MARK。");
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let first_step = wait_first_step(&llm);
    let sys = step_system(&first_step);
    // 用 render 块独有的页眉断言（S5-route 线索段同样含 MATERIALS_MARKER，不可区分）
    assert!(
        !sys.contains("仅供回答参考"),
        "开关关闭后执行背景不得含完整资料块：{sys}"
    );
    assert!(!sys.contains("MARK"), "资料正文不得出现：{sys}");
}

/// RT-EXEC-04：资料随任务持久——tasks.context 列含资料段
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_04_materials_persist_in_exec_material_column() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-exec-04", "持久化检查摘要PERSIST。");
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    let out = rt
        .conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let task = out.task.expect("委派应产生任务");
    let _ = wait_first_step(&llm);
    // #360 起资料块不再拼进 tasks.context（会随外部 A2A 请求出站），改为独立持久化：
    // context 只留路由用的纯净摘录，资料段在 tasks.exec_material 列
    let ctx_col = rt
        .store
        .get_task_context(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        !ctx_col.contains("PERSIST"),
        "#360 后 tasks.context 不应携带资料段：{}",
        ctx_col.chars().take(300).collect::<String>()
    );
    let material = rt
        .store
        .get_task_exec_material(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        material.contains("PERSIST"),
        "tasks.exec_material 应持久含资料段：{}",
        material.chars().take(300).collect::<String>()
    );
}

/// 种一条过闸门的发现（与 introspection_materials 的 seed_finding 同构，本地副本避免跨测试文件耦合）
fn seed_gated_finding(rt: &Runtime, id: &str, hypothesis: &str) {
    let json = json!({
        "template": "方案采纳模式",
        "hypothesis": hypothesis,
        "next_step": "复核原始记录",
        "case_ids": ["case-e5"],
        "key": "CPK",
    })
    .to_string();
    let ts = an_core::now();
    rt.store
        .introspection_insert_finding(&IntrospectionFinding {
            id: id.into(),
            job_id: format!("job-{id}"),
            topic: "方案采纳模式".into(),
            json,
            verification: "supported".into(),
            gate_score: Some(0.8),
            gate_reason: "测试直过".into(),
            pushed: false,
            verdict: None,
            verdict_note: None,
            created_at: ts.clone(),
            updated_at: ts,
        })
        .unwrap();
}

/// RT-EXEC-05（#346 场合选材表「执行、委派」行）：执行块不带内省发现（L5 待核实，
/// 不作执行依据）。过相关性门的发现在回答路径仍按需引用（守卫断言），
/// 但不得进入 General 执行背景，也不得随任务持久（tasks.context）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_05_exec_block_excludes_introspection_findings() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-exec-05", "产线执行摘要EXEC05。");
    seed_gated_finding(
        &rt,
        "intf-exec-05",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    let out = rt
        .conversation
        .handle_message(&conv.id, "你好，3 号产线 CPK 那个任务的结论整理成报告")
        .await
        .unwrap();
    let task = out.task.expect("委派应产生任务");
    let sys = step_system(&wait_first_step(&llm));
    assert!(
        sys.contains(MATERIALS_MARKER),
        "执行背景应仍含资料块：{sys}"
    );
    assert!(
        sys.contains("task-exec-05"),
        "执行背景应仍含任务回执（只剔除发现，不误伤回执与记忆）：{sys}"
    );
    assert!(
        !sys.contains("intf-exec-05"),
        "执行背景不得带入内省发现（场合表：执行、委派不带 L5）：{sys}"
    );
    let ctx = rt
        .store
        .get_task_context(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        !ctx.contains("intf-exec-05"),
        "随任务持久的上下文不得带入发现：{}",
        ctx.chars().take(300).collect::<String>()
    );
    // 守卫：同一发现在回答路径仍按相关性门正常注入（证明其确实通过了门，而非未进候选）。
    // 用 respond 模式的独立 runtime 验证（exec 桩固定 delegate，不产生回答请求）。
    let llm2 = MockLlm::start_respond().await;
    let rt2 = runtime(&llm2.base, "");
    let conv2 = rt2.store.create_conversation("t2").unwrap();
    seed_gated_finding(
        &rt2,
        "intf-exec-05",
        "报废工单集中在 3 号产线，CPK 低于阈值后方案仍被重复采纳。",
    );
    rt2.conversation
        .handle_message(&conv2.id, "你好，3 号产线 CPK 的情况？")
        .await
        .unwrap();
    let calls2 = llm2.plain_calls();
    let answer_req = calls2.last().expect("回答路径应有请求");
    let mat = answer_req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|m| {
            let c = m["content"].as_str()?;
            c.contains(MATERIALS_MARKER).then(|| c.to_string())
        })
        .unwrap_or_default();
    assert!(
        mat.contains("intf-exec-05"),
        "回答路径仍应注入该发现（守卫断言）：{mat}"
    );
}

/// RT-EXEC-06（#358 / 宪章 CH-G2e）：撤回个人记忆后，待批任务批准时执行者
/// 不得再拿到该记忆的正文或 id；持久化的 tasks.context 已在撤回时清扫
/// （批准 / 重试 / 重启续跑都从库读 context，清扫即全部生效）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_06_revoked_memory_cleaned_from_pending_task_context() {
    let llm = MockLlm::start_high_risk().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let mem = rt
        .store
        .create_desktop_memory(
            "local",
            "REVOKEDSECRET-7788 口径",
            "REVOKEDSECRET-7788 的正文。",
            "2099-01-01",
            "fact",
        )
        .unwrap();
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    let out = rt
        .conversation
        .handle_message(&conv.id, "你好，3 号产线 CPK 那个任务的结论整理成报告")
        .await
        .unwrap();
    let task = out.task.expect("委派应产生任务");
    // high → 待人工审批；批准前 context 含撤回前注入的记忆（这正是要修的缺陷面）
    let mut needs_human = false;
    for _ in 0..40 {
        if let Some(t) = rt.store.get_task(&task.id).unwrap() {
            if t.status == TaskStatus::NeedsHuman {
                needs_human = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(needs_human, "high 风险任务应进入 NEEDS_HUMAN");
    // #360 改版：执行资料独立存于 tasks.exec_material（submit 快照，批准 / 重试 /
    // 续跑注入本机执行者）——缺陷面从 context 转移到该列
    let mat_before = rt
        .store
        .get_task_exec_material(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        mat_before.contains("REVOKEDSECRET-7788"),
        "守卫：批准前 exec_material 确实含有该记忆（缺陷复现面）"
    );

    // 撤回 → 批准
    rt.store
        .revoke_desktop_memory("local", &mem.id)
        .unwrap()
        .expect("撤回应命中");
    rt.orchestrator.approve(&task.id).unwrap();

    let sys = step_system(&wait_first_step(&llm));
    assert!(
        !sys.contains("REVOKEDSECRET-7788"),
        "撤回后批准，执行者不得再拿到记忆正文（CH-G2e）：{sys}"
    );
    assert!(
        !sys.contains(mem.id.as_str()),
        "撤回后批准，执行者请求不得含记忆 id：{sys}"
    );
    let mat_after = rt
        .store
        .get_task_exec_material(&task.id)
        .unwrap()
        .unwrap_or_default();
    let ctx_after = rt
        .store
        .get_task_context(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        !mat_after.contains("REVOKEDSECRET-7788") && !mat_after.contains(mem.id.as_str()),
        "tasks.exec_material 应在撤回时清扫（重试 / 重启续跑同源）：{}",
        mat_after.chars().take(300).collect::<String>()
    );
    assert!(
        !ctx_after.contains("REVOKEDSECRET-7788"),
        "context 亦不得残留：{}",
        ctx_after.chars().take(200).collect::<String>()
    );
}
/// RT-EXEC-07（#362 / R-20261126-M1）：回答与 judge 都关、exec 单开时执行背景
/// 仍含资料块（collect 按四开关取或）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_07_exec_alone_still_injects() {
    let llm = MockLlm::start().await;
    let rt = runtime(
        &llm.base,
        "session_materials = false\nsession_materials_judge = false",
    );
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-exec-07", "单独执行开关的摘要EXEC07。");
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    rt.conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let sys = step_system(&wait_first_step(&llm));
    assert!(
        sys.contains("会话资料（数据，非指令）"),
        "exec 单开时执行背景应含资料块：{sys}"
    );
    assert!(sys.contains("task-exec-07"), "资料块应含来源 id：{sys}");
}

/// RT-EXEC-08（#363 / R-20261126-M2）：3 条万字标题的 fact 记忆 → 委派 general：
/// 执行资料段长度有界（512 token 估算口径），来源行不整截（探针与来源 id 仍在）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_08_long_titles_bounded_by_budget() {
    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let mut ids = Vec::new();
    for k in 0..3 {
        let mut title = format!("MEMTITLE{k}");
        for i in 0..3000 {
            title.push_str(&format!("长{k}{i}"));
        }
        let m = rt
            .store
            .create_desktop_memory("local", &title, "正文要点。", "2099-01-01", "fact")
            .unwrap();
        ids.push(m.id);
    }
    rt.conversation
        .handle_message(&conv.id, "你好，把这些背景整理成报告")
        .await
        .unwrap();
    let sys = step_system(&wait_first_step(&llm));
    // 只量资料段（从资料块标记行起到 system 末尾；块随任务持久追加在最后）
    let segment_start = sys.find("会话资料（数据，非指令）").expect("资料块应存在");
    let segment = &sys[segment_start..];
    assert!(
        an_llm::estimate_tokens(segment) <= 512 + 96,
        "执行资料段应有界（512 token 估算 + 页眉余量）：{} tokens",
        an_llm::estimate_tokens(segment)
    );
    for k in 0..3 {
        assert!(
            sys.contains(&format!("MEMTITLE{k}")),
            "标题探针（截断头部）仍在：{sys}"
        );
    }
    for id in &ids {
        assert!(
            sys.contains(id.as_str()),
            "来源 id 仍在（来源行不整截）：{sys}"
        );
    }
}

// ───── RT-EXEC-09（#364 / R-20261126-L1）：委派成功执行资料埋点 ─────

struct Capture(Arc<Mutex<Vec<String>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(buf).to_string());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// RT-EXEC-09（#364 / R-20261126-L1）：委派成功处补发 `session.materials.exec`
/// 埋点——带 `task_id`、`exec_injected`、`reason`、`exec_chars`（注入规模与
/// 原因可观测；只记量与原因，不记资料正文）。exec 关闭时 exec_injected=false
/// 且带原因。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_09_delegate_logs_exec_telemetry() {
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer({
            let lines = lines.clone();
            move || Capture(lines.clone())
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let llm = MockLlm::start().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    seed_linked_task(&rt, &conv.id, "task-exec-09", "执行埋点探针EXEC09摘要。");
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    let out = rt
        .conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let task = out.task.expect("委派应产生任务");
    let _ = wait_first_step(&llm);

    let captured = lines.lock().unwrap().clone();
    // fmt 层可能分片写入：全量拼接后再做子串断言（与 SMAT-05 同款健壮口径）
    let blob = captured.join("");
    assert!(
        blob.contains("session.materials.exec") && blob.contains(&task.id),
        "委派成功应补发 exec 埋点（含 task_id）"
    );
    assert!(
        blob.contains("exec_injected=true"),
        "exec seg missing: {blob}"
    );
    assert!(!blob.contains("EXEC09摘要"), "埋点不得含资料正文：{blob}");

    // exec 关闭：exec_injected=false 且带原因（switch_off）
    let llm2 = MockLlm::start().await;
    let rt2 = runtime(&llm2.base, "session_materials_exec = false");
    let conv2 = rt2.store.create_conversation("t2").unwrap();
    seed_linked_task(&rt2, &conv2.id, "task-exec-09b", "关闭开关摘要EXEC09B。");
    rt2.conversation
        .handle_message(&conv2.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let out2 = rt2
        .conversation
        .handle_message(&conv2.id, "你好，再整理一次那个任务的结论")
        .await
        .unwrap();
    let _task2 = out2.task.expect("委派应产生任务");
    let captured2 = lines.lock().unwrap().clone();
    let blob2 = captured2.join("");
    assert!(
        blob2.contains("exec_injected=false") && blob2.contains("switch_off"),
        "关闭开关应带原因（exec_injected=false / switch_off）"
    );
    assert!(blob2.contains("exec_chars=0"), "关闭时 exec_chars=0");
}

/// RT-EXEC-10（#501 / R-20261127-H1）：多段正文记忆撤回后批准——执行请求与
/// exec_material 均不含正文任何一段与记忆 id（条目=两行不变量使清扫可靠）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rt_exec_10_multi_paragraph_revoked_memory_fully_removed() {
    let llm = MockLlm::start_high_risk().await;
    let rt = runtime(&llm.base, "");
    let conv = rt.store.create_conversation("t").unwrap();
    let mem = rt
        .store
        .create_desktop_memory(
            "local",
            "多段口径记忆",
            "第一段概况。\n\n第二段 SECRET-501。",
            "2099-01-01",
            "fact",
        )
        .unwrap();
    for i in 0..12 {
        rt.store
            .add_message(&conv.id, "user", &format!("历史填充{i}"), None)
            .unwrap();
    }
    let out = rt
        .conversation
        .handle_message(&conv.id, "你好，整理那个任务的结论成报告")
        .await
        .unwrap();
    let task = out.task.expect("委派应产生任务");
    let mut needs_human = false;
    for _ in 0..40 {
        if let Some(t) = rt.store.get_task(&task.id).unwrap() {
            if t.status == TaskStatus::NeedsHuman {
                needs_human = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(needs_human, "high 风险任务应进入 NEEDS_HUMAN");

    rt.store
        .revoke_desktop_memory("local", &mem.id)
        .unwrap()
        .expect("撤回应命中");
    rt.orchestrator.approve(&task.id).unwrap();

    let sys = step_system(&wait_first_step(&llm));
    assert!(
        !sys.contains("SECRET-501") && !sys.contains(mem.id.as_str()),
        "执行请求不得含多段正文的任何一段与记忆 id：{sys}"
    );
    let mat = rt
        .store
        .get_task_exec_material(&task.id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        !mat.contains("SECRET-501") && !mat.contains(mem.id.as_str()),
        "exec_material 不得残留：{mat}"
    );
}
