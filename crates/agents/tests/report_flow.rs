//! 研究报告流程：写完即收尾、逐步循环先搜后写、以及代码控制的规划→检索→并行写章→汇合。

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use an_agents::{GeneralAgent, ToolRegistry};
use an_core::{
    AgentAdapter, EventPublisher, EventSink, TaskEvent, TaskResult, TaskSpec, WorkerStatus,
};
use an_llm::LlmClient;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use tokio::sync::Mutex;

struct Noop;
impl EventPublisher for Noop {
    fn publish(&self, _: TaskEvent) {}
}

/// 按顺序返回脚本里的每一步，记录每次请求体（逐步循环用例）
struct ScriptedLlm {
    base: String,
    calls: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Value>>>,
    server: tokio::task::JoinHandle<()>,
}

fn sse_text(text: &str) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}]})
        ),
    )
}

fn sse(step: Value) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    sse_text(&step.to_string())
}

async fn scripted(steps: Vec<Value>) -> ScriptedLlm {
    let calls = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let (seen, saved) = (calls.clone(), bodies.clone());
    let steps = Arc::new(steps);
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(req): Json<Value>| {
            let (seen, saved, steps) = (seen.clone(), saved.clone(), steps.clone());
            async move {
                saved.lock().await.push(req);
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let step = steps[n.min(steps.len() - 1)].clone();
                sse(step)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    ScriptedLlm {
        base,
        calls,
        bodies,
        server,
    }
}

/// 研究报告受控模型：规划 / 各章写作 / 汇合；可延迟、可失败、可统计并发
struct ReportMock {
    base: String,
    calls: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Value>>>,
    max_in_flight: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}

struct ReportScript {
    plan: Value,
    /// 章 id → (正文, 延迟毫秒, 前几次失败次数)
    chapters: Vec<(String, String, u64, usize)>,
    join: Value,
    hang_chapters: bool,
}

async fn report_mock(script: ReportScript) -> ReportMock {
    let calls = Arc::new(AtomicUsize::new(0));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let chapter_fails = Arc::new(Mutex::new(
        script
            .chapters
            .iter()
            .map(|(id, _, _, fails)| (id.clone(), *fails))
            .collect::<HashMap<String, usize>>(),
    ));
    let chapters = Arc::new(script.chapters);
    let plan = Arc::new(script.plan);
    let join = Arc::new(script.join);
    let hang = script.hang_chapters;

    let (c0, b0, f0, m0) = (
        calls.clone(),
        bodies.clone(),
        in_flight.clone(),
        max_in_flight.clone(),
    );
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(req): Json<Value>| {
            let (calls, bodies, in_flight, max_in_flight, chapter_fails, chapters, plan, join) = (
                c0.clone(),
                b0.clone(),
                f0.clone(),
                m0.clone(),
                chapter_fails.clone(),
                chapters.clone(),
                plan.clone(),
                join.clone(),
            );
            async move {
                bodies.lock().await.push(req.clone());
                calls.fetch_add(1, Ordering::SeqCst);
                let prompt = joined(&req);
                let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(cur, Ordering::SeqCst);

                let is_join = prompt.contains("摘要与结论");
                let is_chapter = prompt.contains("章节正文")
                    || prompt.contains("branch_id=")
                    || prompt.contains("章节标识：");
                let out = if is_join {
                    sse((*join).clone())
                } else if is_chapter {
                    let mut hit = None;
                    for (id, body, delay_ms, _) in chapters.iter() {
                        if prompt.contains(&format!("branch_id={id}"))
                            || prompt.contains(&format!("章节标识：{id}"))
                        {
                            hit = Some((id.clone(), body.clone(), *delay_ms));
                            break;
                        }
                    }
                    if hit.is_none() {
                        for (id, body, delay_ms, _) in chapters.iter() {
                            let title = body
                                .lines()
                                .next()
                                .unwrap_or("")
                                .trim_start_matches('#')
                                .trim();
                            if !title.is_empty() && prompt.contains(title) {
                                hit = Some((id.clone(), body.clone(), *delay_ms));
                                break;
                            }
                        }
                    }
                    let Some((id, body, delay_ms)) = hit else {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        return sse_text("## 未知章节\n");
                    };
                    if hang {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    } else if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let mut fails = chapter_fails.lock().await;
                    let left = fails.get(&id).copied().unwrap_or(0);
                    if left > 0 {
                        *fails.get_mut(&id).unwrap() = left - 1;
                        drop(fails);
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        return sse_text("");
                    }
                    drop(fails);
                    sse_text(&body)
                } else {
                    sse((*plan).clone())
                };
                in_flight.fetch_sub(1, Ordering::SeqCst);
                out
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    ReportMock {
        base,
        calls,
        bodies,
        max_in_flight,
        server,
    }
}

fn workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("an-report-flow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 假的 websearch：按查询返回带唯一链接的结果；`fail=true` 时非零退出
#[cfg(unix)]
fn fake_websearch(dir: &Path, fail: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(if fail {
        "fake-websearch-fail"
    } else {
        "fake-websearch"
    });
    let script = if fail {
        "#!/bin/sh\necho '{\"status\":\"error\",\"data\":null,\"message\":\"search unavailable\"}' >&2\nexit 1\n"
            .to_string()
    } else {
        r#"#!/bin/sh
q=$(printf '%s' "$*" | tr ' ' '_')
printf '%s' "{\"status\":\"success\",\"data\":{\"results\":[{\"title\":\"Result for $q\",\"url\":\"https://example.com/src/$q\",\"snippet\":\"Evidence about $q for 2026 midterms\"}]},\"message\":\"\"}"
"#
        .to_string()
    };
    std::fs::write(&path, script).unwrap();
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&path, perm).unwrap();
    path
}

fn task(required_tools: &[&str]) -> TaskSpec {
    serde_json::from_value(json!({"task_id":"report","conversation_id":"c",
        "goal":"写一份引用公开资料的 Markdown 报告", "context":"",
        "route":{"task_class":"L4","execution_role":"general",
            "required_tools":required_tools,"risk_level":"low","reason":"test"}}))
    .unwrap()
}

fn llm(base: &str) -> LlmClient {
    LlmClient::new(serde_json::from_value(json!({"base_url":base,"model":"mock"})).unwrap())
        .unwrap()
}

async fn run(agent: &GeneralAgent, spec: TaskSpec) -> TaskResult {
    agent
        .run(spec, EventSink::new(Arc::new(Noop), "c", "report"))
        .await
        .unwrap()
}

fn write(path: &str, content: &str, mode: &str, answer: &str) -> Value {
    json!({"thought":"写入","action":"tool_call","tool":"write_file",
        "args":{"path":path,"content":content,"mode":mode},"answer":answer})
}

fn search(query: &str) -> Value {
    json!({"thought":"检索","action":"tool_call","tool":"web_search",
        "args":{"query":query,"max_results":3},"answer":""})
}

fn joined(body: &Value) -> String {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn plan3() -> Value {
    json!({
        "title": "美国中期选举与科技股",
        "path": "reports/midterms-ai.md",
        "chapters": [
            {
                "id": "ch1",
                "title": "历史规律",
                "scope": "历次中期后科技股表现",
                "exclude": "不做情景分析",
                "queries": ["US midterm elections tech stocks history"]
            },
            {
                "id": "ch2",
                "title": "政策预期",
                "scope": "2026 政策与芯片法案",
                "exclude": "不写历史回顾",
                "queries": ["2026 midterm AI chip policy"]
            },
            {
                "id": "ch3",
                "title": "情景分析",
                "scope": "红蓝与分裂政府",
                "exclude": "不写历史数据表",
                "queries": ["split government tech stocks scenarios"]
            }
        ]
    })
}

fn join_doc() -> Value {
    json!({
        "abstract": "本报告梳理中期选举对科技与 AI 板块的影响。",
        "conclusion": "分裂政府下波动加大，需跟踪立法进展。"
    })
}

fn chapters_ok() -> Vec<(String, String, u64, usize)> {
    vec![
        (
            "ch1".into(),
            "## 历史规律\n依据 [S1] 可见中期后科技股常先抑后扬。\n".into(),
            0,
            0,
        ),
        (
            "ch2".into(),
            "## 政策预期\n依据 [S2] ，芯片与 AI 监管是主线。\n".into(),
            0,
            0,
        ),
        (
            "ch3".into(),
            "## 情景分析\n依据 [S3] ，分裂政府波动加大。\n".into(),
            0,
            0,
        ),
    ]
}

#[cfg(unix)]
async fn report_agent(dir: &Path, base: &str, parallel: usize, searches: usize) -> GeneralAgent {
    let mut tools = ToolRegistry::with_builtin(dir).unwrap();
    assert!(tools.register_websearch(
        fake_websearch(dir, false).to_str().unwrap(),
        Duration::from_secs(5)
    ));
    GeneralAgent::new(llm(base), tools, 6, 1)
        .with_report_max_searches(searches)
        .with_report_parallel(parallel)
}

#[tokio::test]
async fn rt_rep_01_write_with_answer_finishes_without_another_model_call() {
    let m = scripted(vec![write(
        "reports/r.md",
        "# 报告\n完整正文",
        "overwrite",
        "报告已写入 reports/r.md",
    )])
    .await;
    let dir = workspace("finish");
    let agent = GeneralAgent::new(
        llm(&m.base),
        ToolRegistry::with_builtin(&dir).unwrap(),
        6,
        1,
    );
    let result = run(&agent, task(&["write_file"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "{:?}",
        result.errors
    );
    assert_eq!(result.summary, "报告已写入 reports/r.md");
    assert_eq!(
        m.calls.load(Ordering::SeqCst),
        1,
        "写完带 answer 就不该再调模型"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("reports/r.md")).unwrap(),
        "# 报告\n完整正文"
    );
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_02_write_before_required_web_search_is_sent_back_to_the_model() {
    let m = scripted(vec![
        write("reports/r.md", "# 凭记忆写的报告", "overwrite", "完成"),
        search("2026 midterm election AI stocks"),
        write(
            "reports/r.md",
            "# 报告\n来源：https://example.com/midterms-ai",
            "overwrite",
            "完成",
        ),
    ])
    .await;
    let dir = workspace("search-first");
    let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
    assert!(tools.register_websearch(
        fake_websearch(&dir, false).to_str().unwrap(),
        Duration::from_secs(5)
    ));
    let agent = GeneralAgent::new(llm(&m.base), tools, 6, 1);
    // 只要求 web_search：不进入研究报告流程，仍走逐步循环的「先搜后写」校验
    let result = run(&agent, task(&["web_search"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "{:?}",
        result.errors
    );

    let bodies = m.bodies.lock().await;
    assert_eq!(bodies.len(), 3);
    assert!(
        joined(&bodies[1]).contains("先调用 web_search"),
        "提前写文件要被驳回并说明原因"
    );
    let tools_used: Vec<&str> = result
        .evidence
        .iter()
        .filter(|e| e.kind == "tool_call")
        .map(|e| e.content.split('(').next().unwrap_or(""))
        .collect();
    assert_eq!(tools_used, ["web_search", "write_file"]);
    let disk = std::fs::read_to_string(dir.join("reports/r.md")).unwrap();
    assert!(!disk.contains("凭记忆"));
    assert!(disk.contains("https://example.com/midterms-ai"));
    drop(bodies);
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_03_research_report_writes_outline_then_each_chapter_after_its_search() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: chapters_ok(),
        join: join_doc(),
        hang_chapters: false,
    })
    .await;
    let dir = workspace("chapters");
    let agent = report_agent(&dir, &m.base, 3, 6).await;
    let result = run(&agent, task(&["web_search", "write_file"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "{:?}",
        result.errors
    );
    assert_eq!(
        m.calls.load(Ordering::SeqCst),
        5,
        "1 规划 + 3 章 + 1 汇合"
    );

    let searches: Vec<String> = result
        .evidence
        .iter()
        .filter(|e| e.kind == "tool_call" && e.content.starts_with("web_search"))
        .map(|e| e.content.clone())
        .collect();
    assert!(searches.len() <= 6, "检索不得超过 6 次：{}", searches.len());
    assert_eq!(
        searches.len(),
        searches.iter().collect::<BTreeSet<_>>().len(),
        "不得重复同一查询：{searches:?}"
    );

    let disk = std::fs::read_to_string(dir.join("reports/midterms-ai.md")).unwrap();
    for title in ["## 历史规律", "## 政策预期", "## 情景分析"] {
        assert_eq!(disk.matches(title).count(), 1, "{title} 应恰好一次");
    }
    assert!(
        disk.contains("https://example.com/src/"),
        "正文/参考资料链接须来自检索：{disk}"
    );
    assert!(!disk.contains("正文已省略"), "不得把历史占位写进正文");
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_05_parallel_caps_in_flight_and_keeps_outline_order() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: vec![
            ("ch1".into(), "## 历史规律\n慢章。\n".into(), 200, 0),
            ("ch2".into(), "## 政策预期\n快章。\n".into(), 20, 0),
            ("ch3".into(), "## 情景分析\n中章。\n".into(), 80, 0),
        ],
        join: join_doc(),
        hang_chapters: false,
    })
    .await;
    let dir = workspace("parallel");
    let agent = report_agent(&dir, &m.base, 2, 6).await;
    let result = run(&agent, task(&["web_search", "write_file"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "{:?}",
        result.errors
    );
    assert!(
        m.max_in_flight.load(Ordering::SeqCst) <= 2,
        "同时写作不得超过 report_parallel=2，实际 {}",
        m.max_in_flight.load(Ordering::SeqCst)
    );
    let disk = std::fs::read_to_string(dir.join("reports/midterms-ai.md")).unwrap();
    let i1 = disk.find("## 历史规律").expect("ch1");
    let i2 = disk.find("## 政策预期").expect("ch2");
    let i3 = disk.find("## 情景分析").expect("ch3");
    assert!(i1 < i2 && i2 < i3, "须按大纲顺序落盘，即使完成顺序打乱");
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_06_all_searches_failed_does_not_write() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: chapters_ok(),
        join: join_doc(),
        hang_chapters: false,
    })
    .await;
    let dir = workspace("search-fail");
    let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
    assert!(tools.register_websearch(
        fake_websearch(&dir, true).to_str().unwrap(),
        Duration::from_secs(5)
    ));
    let agent = GeneralAgent::new(llm(&m.base), tools, 6, 1)
        .with_report_max_searches(6)
        .with_report_parallel(3);
    let result = run(&agent, task(&["web_search", "write_file"])).await;
    assert_eq!(result.status, WorkerStatus::Failed, "{:?}", result.errors);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.contains("检索") || e.contains("搜索")),
        "错误须说明检索失败：{:?}",
        result.errors
    );
    assert!(
        m.calls.load(Ordering::SeqCst) <= 1,
        "检索全失败后不得写章，实际调用 {}",
        m.calls.load(Ordering::SeqCst)
    );
    assert!(
        !dir.join("reports/midterms-ai.md").exists(),
        "不得落盘报告文件"
    );
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_07_majority_chapters_ok_marks_missing() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: vec![
            ("ch1".into(), "## 历史规律\n依据 [S1]。\n".into(), 0, 0),
            ("ch2".into(), "## 政策预期\n不应出现。\n".into(), 0, 2),
            ("ch3".into(), "## 情景分析\n依据 [S3]。\n".into(), 0, 0),
        ],
        join: join_doc(),
        hang_chapters: false,
    })
    .await;
    let dir = workspace("partial");
    let agent = report_agent(&dir, &m.base, 3, 6).await;
    let result = run(&agent, task(&["web_search", "write_file"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "过半成功应完成：{:?}",
        result.errors
    );
    assert!(
        result.summary.contains("政策预期") || result.summary.contains("ch2"),
        "交付说明须写明缺失章节：{}",
        result.summary
    );
    let disk = std::fs::read_to_string(dir.join("reports/midterms-ai.md")).unwrap();
    assert!(disk.contains("## 历史规律"));
    assert!(disk.contains("## 情景分析"));
    assert!(
        !disk.contains("## 政策预期"),
        "失败章不得用别章填补：{disk}"
    );
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_08_cancel_stops_new_model_calls_and_file_growth() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: chapters_ok(),
        join: join_doc(),
        hang_chapters: true,
    })
    .await;
    let dir = workspace("cancel");
    let agent = Arc::new(report_agent(&dir, &m.base, 3, 6).await);
    let handle = tokio::spawn({
        let agent = agent.clone();
        async move { run(&agent, task(&["web_search", "write_file"])).await }
    });
    let mut saw_chapter = false;
    for _ in 0..100 {
        let bodies = m.bodies.lock().await;
        saw_chapter = bodies.iter().any(|b| {
            let p = joined(b);
            p.contains("章节正文") || p.contains("branch_id=") || p.contains("章节标识：")
        });
        drop(bodies);
        if saw_chapter {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        saw_chapter,
        "取消前必须已进入章节写作阶段（否则测的不是新流程）"
    );
    handle.abort();
    let _ = handle.await;
    let calls_after_abort = m.calls.load(Ordering::SeqCst);
    let path = dir.join("reports/midterms-ai.md");
    let size_after = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        m.calls.load(Ordering::SeqCst),
        calls_after_abort,
        "abort 后不得再发模型请求"
    );
    let size_later = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    assert_eq!(size_later, size_after, "abort 后报告文件不得继续增长");
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn rt_rep_09_strips_unknown_citation_and_keeps_real_links() {
    let m = report_mock(ReportScript {
        plan: plan3(),
        chapters: vec![
            (
                "ch1".into(),
                "## 历史规律\n依据 [S1] 与伪造 [S9]。\n".into(),
                0,
                0,
            ),
            ("ch2".into(), "## 政策预期\n依据 [S2]。\n".into(), 0, 0),
            ("ch3".into(), "## 情景分析\n依据 [S3]。\n".into(), 0, 0),
        ],
        join: join_doc(),
        hang_chapters: false,
    })
    .await;
    let dir = workspace("cite");
    let agent = report_agent(&dir, &m.base, 3, 6).await;
    let result = run(&agent, task(&["web_search", "write_file"])).await;
    assert_eq!(
        result.status,
        WorkerStatus::Succeeded,
        "{:?}",
        result.errors
    );
    let disk = std::fs::read_to_string(dir.join("reports/midterms-ai.md")).unwrap();
    assert!(!disk.contains("[S9]"), "不存在的引用须剔除：{disk}");
    assert!(
        disk.contains("https://example.com/src/"),
        "参考资料只能是检索到的真实链接"
    );
    m.server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}
