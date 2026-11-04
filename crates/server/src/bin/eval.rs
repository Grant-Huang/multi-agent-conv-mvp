//! 路由评测工具：只跑 Turn Controller + Router，不执行任务。
//!
//! 用法：`cargo run --bin an-eval -- [config.toml] [evals/routing.jsonl]`
//!
//! 评测集每行一个 JSON：
//! `{"text": "用户消息", "expect_action": "respond|clarify|delegate", "expect_role": "general|coding|reviewer", "expect_class": "L1..L4"}`
//! 其中 expect_role / expect_class 只在 delegate 时有意义，可省略。
//!
//! 输出：逐条结果 + 汇总指标（动作准确率、角色准确率、类别准确率、结构化输出重试/失败、延迟）。
//! 详细结果写入 `evals/results/<时间戳>.jsonl`，方便对比不同模型。

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use an_core::{Message, TurnDecision};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Case {
    text: String,
    expect_action: String,
    expect_role: Option<String>,
    expect_class: Option<String>,
}

#[derive(Serialize)]
struct CaseResult {
    text: String,
    expect_action: String,
    got_action: Option<String>,
    expect_role: Option<String>,
    got_role: Option<String>,
    expect_class: Option<String>,
    got_class: Option<String>,
    /// 结构化调用总尝试次数
    attempts: u32,
    /// 结构化调用次数（Turn Controller 1 次 + Router 0/1 次）；attempts > calls 说明发生过重试
    calls: u32,
    latency_ms: u64,
    error: Option<String>,
    raw_outputs: Vec<String>,
}

fn action_name(d: &TurnDecision) -> &'static str {
    match d {
        TurnDecision::Respond { .. } => "respond",
        TurnDecision::Clarify { .. } => "clarify",
        TurnDecision::Delegate { .. } => "delegate",
        TurnDecision::FollowUp { .. } => "follow_up",
    }
}

/// 计算百分位延迟
fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).init();
    let mut args = std::env::args().skip(1);
    let config_path = PathBuf::from(args.next().unwrap_or_else(|| "config.toml".into()));
    let cases_path = PathBuf::from(args.next().unwrap_or_else(|| "evals/routing.jsonl".into()));

    let config = AppConfig::load(&config_path)?;
    // 评测用内存数据库，不污染正式数据
    let rt = Runtime::new(config.clone(), Store::open(std::path::Path::new(":memory:"))?)?;
    let cases: Vec<Case> = std::fs::read_to_string(&cases_path)
        .with_context(|| format!("无法读取评测集 {}", cases_path.display()))?
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).with_context(|| format!("评测集第 {} 行格式错误", i + 1)))
        .collect::<Result<_>>()?;

    println!("模型服务：{}（模型 {}，结构化模式 {:?}）", config.llm.base_url, config.llm.model, config.llm.structured_mode);
    println!("评测集：{}，共 {} 条\n", cases_path.display(), cases.len());

    let mut results = vec![];
    for (i, c) in cases.iter().enumerate() {
        let started = Instant::now();
        let history = [Message {
            id: "eval".into(),
            conversation_id: "eval".into(),
            role: "user".into(),
            content: c.text.clone(),
            task_id: None,
            created_at: an_core::now(),
        }];
        let mut r = CaseResult {
            text: c.text.clone(),
            expect_action: c.expect_action.clone(),
            got_action: None,
            expect_role: c.expect_role.clone(),
            got_role: None,
            expect_class: c.expect_class.clone(),
            got_class: None,
            attempts: 0,
            calls: 1,
            latency_ms: 0,
            error: None,
            raw_outputs: vec![],
        };

        match rt.conversation.decide_turn(&history, &[]).await {
            Ok(d) => {
                r.attempts += d.attempts;
                r.raw_outputs.extend(d.raw_outputs);
                r.got_action = Some(action_name(&d.value).into());
                // 只有判断为 delegate 时才继续测 Router
                if let TurnDecision::Delegate { task_goal } = &d.value {
                    r.calls += 1;
                    match rt.router.route(task_goal, "").await {
                        Ok(route) => {
                            r.attempts += route.attempts;
                            r.raw_outputs.extend(route.raw_outputs);
                            r.got_role = Some(route.decision.execution_role.as_str().into());
                            r.got_class = Some(format!("{:?}", route.decision.task_class));
                        }
                        Err(f) => {
                            r.attempts += f.attempts;
                            r.raw_outputs.extend(f.raw_outputs);
                            r.got_role = f.decision.as_ref().map(|d| d.execution_role.as_str().into());
                            r.error = Some(f.message);
                        }
                    }
                }
            }
            Err(e) => {
                r.attempts += e.attempts;
                r.raw_outputs.extend(e.raw_outputs.clone());
                r.error = Some(e.message);
            }
        }
        r.latency_ms = started.elapsed().as_millis() as u64;

        let ok_action = r.got_action.as_deref() == Some(&r.expect_action);
        let ok_role = r.expect_role.is_none() || r.got_role == r.expect_role;
        println!(
            "[{:>3}/{}] {} 动作 {:<9} 角色 {:<8} 类别 {:<3} 尝试 {} 次 {:>6}ms  {}",
            i + 1,
            cases.len(),
            if ok_action && ok_role { "✓" } else { "✗" },
            r.got_action.as_deref().unwrap_or("-"),
            r.got_role.as_deref().unwrap_or("-"),
            r.got_class.as_deref().unwrap_or("-"),
            r.attempts,
            r.latency_ms,
            an_core::truncate_chars(&c.text, 30),
        );
        if let Some(e) = &r.error {
            println!("          错误：{e}");
        }
        results.push(r);
    }

    // ───── 汇总 ─────
    let n = results.len().max(1) as f64;
    let action_ok = results.iter().filter(|r| r.got_action.as_deref() == Some(&r.expect_action)).count();
    let role_cases: Vec<_> = results.iter().filter(|r| r.expect_role.is_some()).collect();
    let role_ok = role_cases.iter().filter(|r| r.got_role == r.expect_role).count();
    let class_cases: Vec<_> = results.iter().filter(|r| r.expect_class.is_some()).collect();
    let class_ok = class_cases.iter().filter(|r| r.got_class == r.expect_class).count();
    let retried = results.iter().filter(|r| r.attempts > r.calls).count();
    let failed = results.iter().filter(|r| r.error.is_some()).count();
    let mut lat: Vec<u64> = results.iter().map(|r| r.latency_ms).collect();
    lat.sort_unstable();
    let pct = |a: usize, b: usize| if b == 0 { "-".to_string() } else { format!("{:.1}%（{a}/{b}）", a as f64 * 100.0 / b as f64) };

    println!("\n══════════ 汇总 ══════════");
    println!("动作准确率（respond/clarify/delegate）：{}", pct(action_ok, results.len()));
    println!("角色准确率（有标注的样本）          ：{}", pct(role_ok, role_cases.len()));
    println!("类别准确率（有标注的样本）          ：{}", pct(class_ok, class_cases.len()));
    println!("出现过格式重试的样本                ：{retried}");
    println!("最终出错的样本                      ：{failed}");
    println!("延迟 P50 / P95 / 平均（含两次调用） ：{} / {} / {:.0} ms", percentile(&lat, 0.5), percentile(&lat, 0.95), lat.iter().sum::<u64>() as f64 / n);

    // ───── 写结果文件 ─────
    let dir = PathBuf::from("evals/results");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.jsonl", chrono_like_stamp()));
    let mut f = std::fs::File::create(&file)?;
    for r in &results {
        writeln!(f, "{}", serde_json::to_string(r)?)?;
    }
    println!("\n详细结果（含模型原始输出）：{}", file.display());
    Ok(())
}

/// 用当前时间生成文件名（避免为此引入 chrono 依赖）
fn chrono_like_stamp() -> String {
    an_core::now().replace([':', '.'], "-")
}
