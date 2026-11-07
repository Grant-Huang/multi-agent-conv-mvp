//! 研究报告：代码控制「规划 → 检索 → 并行写章 → 汇合」。
//!
//! Router 同时要求 `web_search` 与 `write_file` 时由 General 分派到这里。
//! 章节结果使用与 DESIGN 9.6 一致的信封字段（`branch_id` / `status` / `missing` / `payload`），
//! 并行发生在本模块内部，不改编排器。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use an_core::{Artifact, EventSink, Evidence, TaskResult, TaskSpec, WorkerStatus};
use an_llm::{ChatMessage, LlmClient};
use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::tools::ToolRegistry;

const MAX_CHAPTERS: usize = 5;
const CHAPTER_RETRIES: usize = 1;

#[derive(Debug, Deserialize)]
struct Plan {
    title: String,
    #[serde(default)]
    path: String,
    chapters: Vec<PlanChapter>,
}

#[derive(Debug, Deserialize)]
struct PlanChapter {
    id: String,
    title: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    exclude: String,
    #[serde(default)]
    queries: Vec<String>,
}

#[derive(Debug)]
struct JoinDoc {
    abstract_: String,
    conclusion: String,
}

fn join_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "abstract": { "type": "string" },
            "conclusion": { "type": "string" }
        },
        "required": ["abstract", "conclusion"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone)]
struct Source {
    id: usize,
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone)]
struct ChapterOutcome {
    id: String,
    title: String,
    status: String,
    missing: String,
    markdown: String,
    summary: String,
}

/// 跑完整份研究报告。`max_searches` / `parallel` 来自配置。
pub async fn run(
    llm: &LlmClient,
    tools: &ToolRegistry,
    task: &TaskSpec,
    events: &EventSink,
    max_searches: usize,
    parallel: usize,
) -> Result<TaskResult> {
    let mut evidence = Vec::new();
    let max_searches = max_searches.max(1);
    let parallel = parallel.max(1);

    events.progress("规划研究报告大纲");
    let now = match tools.call("current_time", &json!({})).await {
        Ok(out) => out.text,
        Err(_) => String::new(),
    };
    let plan = match plan_report(llm, task, &now).await {
        Ok(p) => p,
        Err(e) => {
            return Ok(TaskResult::failed(format!("研究报告规划失败：{e}")));
        }
    };
    evidence.push(Evidence {
        kind: "plan".into(),
        content: format!(
            "title={} path={} chapters={}",
            plan.title,
            plan.path,
            plan.chapters
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
    });

    events.progress(format!(
        "检索资料（最多 {max_searches} 次，章节 {}）",
        plan.chapters.len()
    ));
    let (sources, search_evidence) = search_all(tools, &plan, max_searches).await;
    evidence.extend(search_evidence);
    if sources.is_empty() {
        return Ok(TaskResult {
            evidence,
            ..TaskResult::failed("检索失败，未生成报告：所有搜索都没有可用结果")
        });
    }

    events.progress(format!("并行撰写章节（同时最多 {parallel} 路）"));
    let outcomes = write_chapters(llm, &plan, &sources, parallel).await;
    for o in &outcomes {
        evidence.push(Evidence {
            kind: "chapter".into(),
            content: format!(
                "{} status={} missing={}",
                o.id,
                o.status,
                if o.missing.is_empty() {
                    "-"
                } else {
                    &o.missing
                }
            ),
        });
    }

    let ok_n = outcomes.iter().filter(|o| o.status == "ok").count();
    let total = outcomes.len().max(1);
    if ok_n * 2 <= total {
        let missing: Vec<_> = outcomes
            .iter()
            .filter(|o| o.status != "ok")
            .map(|o| o.title.as_str())
            .collect();
        return Ok(TaskResult {
            evidence,
            ..TaskResult::failed(format!(
                "研究报告章节失败过多（成功 {ok_n}/{total}），缺失：{}",
                missing.join("、")
            ))
        });
    }

    events.progress("汇合摘要、结论与参考资料");
    let join = join_report(llm, &plan, &outcomes).await;
    let (body, stripped) = assemble(&plan, &outcomes, &sources, &join);
    if !stripped.is_empty() {
        evidence.push(Evidence {
            kind: "citation".into(),
            content: format!("已剔除不存在的引用：{}", stripped.join(", ")),
        });
    }

    let path = if plan.path.trim().is_empty() {
        "reports/report.md"
    } else {
        plan.path.trim()
    };
    let write = tools
        .call(
            "write_file",
            &json!({"path": path, "content": body, "mode": "overwrite"}),
        )
        .await;
    let mut artifacts = Vec::new();
    match write {
        Ok(out) => {
            evidence.push(Evidence {
                kind: "tool_call".into(),
                content: format!(
                    "write_file({}) → {}",
                    json!({"path": path, "mode": "overwrite"}),
                    an_core::truncate_chars(&out.text, 200)
                ),
            });
            artifacts.extend(out.artifacts);
        }
        Err(e) => {
            return Ok(TaskResult {
                evidence,
                ..TaskResult::failed(format!("写入报告失败：{e:#}"))
            });
        }
    }

    let missing: Vec<String> = outcomes
        .iter()
        .filter(|o| o.status != "ok")
        .map(|o| format!("{}（{}）", o.title, o.id))
        .collect();
    let mut summary = format!("报告已写入 {path}");
    if !missing.is_empty() {
        summary.push_str(&format!("；缺失章节：{}", missing.join("、")));
    }

    // 保证产物里有完整正文（write_file 已带 artifact；再兜底一份）
    if artifacts.is_empty() {
        artifacts.push(Artifact {
            kind: "file".into(),
            name: path.into(),
            content: body,
        });
    }

    Ok(TaskResult {
        status: WorkerStatus::Succeeded,
        summary,
        artifacts,
        evidence,
        errors: vec![],
    })
}

async fn plan_report(llm: &LlmClient, task: &TaskSpec, now: &str) -> Result<Plan> {
    let schema = json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "path": { "type": "string" },
            "chapters": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_CHAPTERS,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "scope": { "type": "string" },
                        "exclude": { "type": "string" },
                        "queries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "maxItems": 2
                        }
                    },
                    "required": ["id", "title", "scope", "exclude", "queries"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["title", "path", "chapters"],
        "additionalProperties": false
    });
    let messages = vec![
        ChatMessage::system(
            "你是研究报告规划器。只输出一个 JSON 对象，不要写正文。\n\
             章节 3 到 5 个；每章 1 到 2 个检索查询；查询要具体、彼此不重复。\n\
             path 用相对路径，例如 reports/report.md。\n\
             当前日期以工具给出的时间为准，不要臆造年份。",
        ),
        ChatMessage::user(format!(
            "任务：{}\n当前时间：{}\n上下文：{}",
            task.goal,
            if now.is_empty() { "未知" } else { now },
            if task.context.is_empty() {
                "无"
            } else {
                &task.context
            }
        )),
    ];
    let planned = llm
        .chat_json::<Plan, _>(&messages, "report_plan", &schema, |p| {
            if p.chapters.is_empty() || p.chapters.len() > MAX_CHAPTERS {
                return Err(format!("章节数须在 1..={MAX_CHAPTERS}"));
            }
            let mut ids = BTreeSet::new();
            for c in &p.chapters {
                if c.id.trim().is_empty() || c.title.trim().is_empty() {
                    return Err("章节 id/title 不能为空".into());
                }
                if !ids.insert(c.id.clone()) {
                    return Err(format!("章节 id 重复：{}", c.id));
                }
                if c.queries.iter().all(|q| q.trim().is_empty()) {
                    return Err(format!("章节 {} 至少要有一个查询", c.id));
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(planned.value)
}

async fn search_all(
    tools: &ToolRegistry,
    plan: &Plan,
    max_searches: usize,
) -> (Vec<Source>, Vec<Evidence>) {
    let mut queries = Vec::new();
    let mut seen = BTreeSet::new();
    for ch in &plan.chapters {
        for q in &ch.queries {
            let q = q.trim();
            if q.is_empty() || !seen.insert(q.to_string()) {
                continue;
            }
            queries.push(q.to_string());
            if queries.len() >= max_searches {
                break;
            }
        }
        if queries.len() >= max_searches {
            break;
        }
    }

    let mut sources = Vec::new();
    let mut evidence = Vec::new();
    let mut next_id = 1usize;

    // 并行检索；结果按查询原顺序编号，便于稳定引用
    // Exa 容易 429：同时最多 2 路，失败重试 1 次
    let sem = Arc::new(Semaphore::new(2));
    let mut futs = FuturesUnordered::new();
    for (i, q) in queries.iter().cloned().enumerate() {
        let tools = tools.clone();
        let sem = sem.clone();
        futs.push(async move {
            let _permit = sem.acquire().await.expect("search semaphore");
            // 不截断后可直接 search-fetch：标题/链接/摘要 + 正文，一并给写章用
            let args = json!({"query": q, "max_results": 3});
            let mut result = tools.call("web_search", &args).await;
            if result.is_err() {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                result = tools.call("web_search", &args).await;
            }
            (i, q, args, result)
        });
    }
    let mut ordered: Vec<(
        usize,
        String,
        Value,
        Result<crate::tools::ToolOutput, anyhow::Error>,
    )> = Vec::new();
    while let Some(item) = futs.next().await {
        ordered.push(item);
    }
    ordered.sort_by_key(|(i, _, _, _)| *i);

    for (_, q, args, result) in ordered {
        match result {
            Ok(out) => {
                evidence.push(Evidence {
                    kind: "tool_call".into(),
                    content: format!(
                        "web_search({}) → {}",
                        args,
                        an_core::truncate_chars(&out.text, 500)
                    ),
                });
                for hit in parse_search_hits(&out.text) {
                    sources.push(Source {
                        id: next_id,
                        title: hit.0,
                        url: hit.1,
                        snippet: hit.2,
                    });
                    next_id += 1;
                }
            }
            Err(e) => {
                evidence.push(Evidence {
                    kind: "tool_call".into(),
                    content: format!("web_search({}) → error: {e:#}", json!({"query": q})),
                });
            }
        }
    }
    (sources, evidence)
}

fn parse_search_hits(text: &str) -> Vec<(String, String, String)> {
    let v = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(_) => return extract_urls_loose(text),
    };
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "error" {
        return Vec::new();
    }
    let results = v
        .pointer("/data/results")
        .or_else(|| v.get("results"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let mut hits: Vec<(String, String, String)> = results
        .into_iter()
        .filter_map(|r| {
            let url = r.get("url")?.as_str()?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            let title = r
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("来源")
                .trim()
                .to_string();
            let snippet = r
                .get("snippet")
                .or_else(|| r.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .chars()
                .take(300)
                .collect();
            Some((title, url, snippet))
        })
        .collect();
    // search-fetch 还会带 data.pages 正文；按 url 补进对应来源的摘要后段
    if let Some(pages) = v.pointer("/data/pages").and_then(|p| p.as_array()) {
        let mut by_url: std::collections::BTreeMap<String, usize> = hits
            .iter()
            .enumerate()
            .map(|(i, (_, u, _))| (u.clone(), i))
            .collect();
        for p in pages {
            let url = p.get("url").and_then(|u| u.as_str()).unwrap_or("").trim();
            let content = p
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim();
            if url.is_empty() || content.is_empty() {
                continue;
            }
            if let Some(&i) = by_url.get(url) {
                let extra: String = content.chars().take(1200).collect();
                if !hits[i]
                    .2
                    .contains(&extra.chars().take(40).collect::<String>())
                {
                    if !hits[i].2.is_empty() {
                        hits[i].2.push('\n');
                    }
                    hits[i].2.push_str(&extra);
                }
            } else {
                let title = p
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("来源")
                    .trim()
                    .to_string();
                let snippet: String = content.chars().take(1200).collect();
                by_url.insert(url.to_string(), hits.len());
                hits.push((title, url.to_string(), snippet));
            }
        }
    }
    if hits.is_empty() {
        extract_urls_loose(text)
    } else {
        hits
    }
}

/// 输出被截断成非法 JSON 时，尽量捞出 url 字段，避免整份报告因截断失败
fn extract_urls_loose(text: &str) -> Vec<(String, String, String)> {
    let mut hits = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("\"url\"") {
        rest = &rest[i + 5..];
        let Some(colon) = rest.find(':') else { break };
        rest = rest[colon + 1..].trim_start();
        if !rest.starts_with('"') {
            continue;
        }
        rest = &rest[1..];
        let mut url = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    url.push(n);
                }
                continue;
            }
            if c == '"' {
                break;
            }
            url.push(c);
        }
        rest = chars.as_str();
        if url.starts_with("http://") || url.starts_with("https://") {
            hits.push(("来源".into(), url, String::new()));
        }
    }
    hits
}

async fn write_chapters(
    llm: &LlmClient,
    plan: &Plan,
    sources: &[Source],
    parallel: usize,
) -> Vec<ChapterOutcome> {
    let sem = Arc::new(Semaphore::new(parallel));
    let source_block = sources
        .iter()
        .map(|s| format!("[S{}] {} — {}\n{}\n", s.id, s.title, s.url, s.snippet))
        .collect::<Vec<_>>()
        .join("\n");
    let outline = plan
        .chapters
        .iter()
        .map(|c| {
            format!(
                "- {}（{}）：写「{}」；不写「{}」",
                c.id, c.title, c.scope, c.exclude
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut futs = FuturesUnordered::new();
    for ch in &plan.chapters {
        let sem = sem.clone();
        let llm = llm.clone();
        let id = ch.id.clone();
        let title = ch.title.clone();
        let scope = ch.scope.clone();
        let exclude = ch.exclude.clone();
        let source_block = source_block.clone();
        let outline = outline.clone();
        let report_title = plan.title.clone();
        futs.push(async move {
            let _permit = sem.acquire().await.expect("semaphore");
            let req = ChapterWrite {
                report_title,
                outline,
                id: id.clone(),
                title: title.clone(),
                scope,
                exclude,
                source_block,
            };
            let mut last_err = "空正文".to_string();
            for _ in 0..=CHAPTER_RETRIES {
                match write_one_chapter(&llm, &req).await {
                    Ok(md) => {
                        let summary = md
                            .lines()
                            .find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                            .unwrap_or("（本章）")
                            .chars()
                            .take(80)
                            .collect::<String>();
                        return ChapterOutcome {
                            id,
                            title,
                            status: "ok".into(),
                            missing: String::new(),
                            markdown: md,
                            summary,
                        };
                    }
                    Err(e) => last_err = e,
                }
            }
            ChapterOutcome {
                id,
                title,
                status: "missing".into(),
                missing: last_err,
                markdown: String::new(),
                summary: String::new(),
            }
        });
    }

    let mut by_id = BTreeMap::new();
    while let Some(o) = futs.next().await {
        by_id.insert(o.id.clone(), o);
    }
    plan.chapters
        .iter()
        .filter_map(|c| by_id.remove(&c.id))
        .collect()
}

struct ChapterWrite {
    report_title: String,
    outline: String,
    id: String,
    title: String,
    scope: String,
    exclude: String,
    source_block: String,
}

async fn write_one_chapter(
    llm: &LlmClient,
    req: &ChapterWrite,
) -> std::result::Result<String, String> {
    let messages = vec![
        ChatMessage::system(format!(
            "你在写研究报告《{}》的一章。\n\
             只输出本章 Markdown 正文（章节正文），不要包 JSON，不要写其他章。\n\
             引用资料时使用 [S编号] 形式，只使用下方列出的编号。",
            req.report_title
        )),
        ChatMessage::user(format!(
            "章节标识：{id}\nbranch_id={id}\n章节标题：{title}\n\
             本章应写：{scope}\n本章不写：{exclude}\n\n\
             整份大纲：\n{outline}\n\n\
             可用资料：\n{sources}\n\n\
             请以 `## {title}` 开头输出本章正文。",
            id = req.id,
            title = req.title,
            scope = req.scope,
            exclude = req.exclude,
            outline = req.outline,
            sources = req.source_block,
        )),
    ];
    let text = llm
        .chat(&messages)
        .await
        .map_err(|e| format!("模型调用失败：{e:#}"))?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("章节正文为空".into());
    }
    Ok(text)
}

async fn join_report(llm: &LlmClient, plan: &Plan, outcomes: &[ChapterOutcome]) -> JoinDoc {
    let summaries = outcomes
        .iter()
        .map(|o| {
            if o.status == "ok" {
                format!("- {}：{}", o.title, o.summary)
            } else {
                format!("- {}：缺失（{}）", o.title, o.missing)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        ChatMessage::system("你为研究报告写摘要与结论。只输出 JSON，字段 abstract 与 conclusion。"),
        ChatMessage::user(format!(
            "报告标题：{}\n各章概要：\n{summaries}\n请写摘要与结论。",
            plan.title
        )),
    ];
    match llm
        .chat_json::<Value, _>(&messages, "report_join", &join_schema(), |_| Ok(()))
        .await
    {
        Ok(s) => JoinDoc {
            abstract_: s
                .value
                .get("abstract")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            conclusion: s
                .value
                .get("conclusion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        Err(_) => JoinDoc {
            abstract_: String::new(),
            conclusion: String::new(),
        },
    }
}

fn assemble(
    plan: &Plan,
    outcomes: &[ChapterOutcome],
    sources: &[Source],
    join: &JoinDoc,
) -> (String, Vec<String>) {
    let mut stripped_all = Vec::new();
    let mut parts = Vec::new();
    parts.push(format!("# {}\n", plan.title.trim()));
    if !join.abstract_.trim().is_empty() {
        parts.push(format!("{}\n", join.abstract_.trim()));
    }
    for o in outcomes {
        if o.status != "ok" {
            continue;
        }
        let (md, stripped) = rewrite_citations(&o.markdown, sources);
        stripped_all.extend(stripped);
        parts.push(md);
        if !parts.last().map(|s| s.ends_with('\n')).unwrap_or(false) {
            parts.push(String::new());
        }
    }
    if !join.conclusion.trim().is_empty() {
        parts.push(format!("## 结论\n{}\n", join.conclusion.trim()));
    }
    parts.push("## 参考资料\n".into());
    for s in sources {
        parts.push(format!("- [S{}] [{}]({})\n", s.id, s.title, s.url));
    }
    stripped_all.sort();
    stripped_all.dedup();
    (parts.join("\n"), stripped_all)
}

fn rewrite_citations(md: &str, sources: &[Source]) -> (String, Vec<String>) {
    let mut out = md.to_string();
    for s in sources {
        let needle = format!("[S{}]", s.id);
        let link = format!("[{}]({})", s.title, s.url);
        out = out.replace(&needle, &link);
    }
    let (cleaned, unknown) = strip_unknown_s_refs(&out);
    (cleaned, unknown)
}

fn strip_unknown_s_refs(s: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut unknown = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' && i + 2 < chars.len() && chars[i + 1] == 'S' {
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j < chars.len() && chars[j] == ']' && j > i + 2 {
                unknown.push(chars[i..=j].iter().collect());
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    unknown.sort();
    unknown.dedup();
    (out, unknown)
}
