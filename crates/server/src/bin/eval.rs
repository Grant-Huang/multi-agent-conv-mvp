//! 路由评测工具：只跑"前台判断 + Router 分类"，不执行任务。用于对比不同模型、不同提示词的准确率。
//!
//! 用法：
//! ```text
//! cargo run --release --bin an-eval -- [config.toml] [evals/routing.jsonl] [选项]
//! cargo run --bin an-eval -- --export-feedback data/agentnexus.db .tmp/feedback-draft.jsonl
//! cargo run --bin an-eval -- --dream [logs 目录] [日期]
//!   --review-mode reviewer|direct|auto   覆盖配置中的审查模式
//!   --tag 标签                           只跑带这个标签的样本（可重复）
//!   --limit N                            只跑前 N 条
//!   --label 名称                         结果文件名里的标记，例如模型名
//!   --agent-management                   打开"对话里管理助手"（AG.4，只在内存里覆盖配置）；评测 evals/agents.jsonl 时用
//!   --variant E0..E10                    FT 前台提速实验的配置组合（只在内存里覆盖，不改配置文件）：
//!       E0 开思考（基线）  E1 关思考  E2 关思考+A  E3 关思考+A+B  E4 关思考+A+B+D  E5 开思考+B
//!       E6 关思考+Q  E7 开思考+Q  E10 关思考+T（没有 E8、E9）
//!       A = front_reason_first，B = [conversation] risk_rules，D = front_thinking_escalation，
//!       Q = [conversation] route_class_questions（类别拆成两个是/否问题，FT.5），
//!       T = [conversation] route_class_from_tools（general 任务的类别由工具清单算出，FT.5b）
//! ```
//!
//! 数据集格式（每行一个 JSON，`//` 开头的行是注释）：
//! ```json
//! {"id": "calc-01", "text": "用户消息", "tags": ["tool", "calc"],
//!  "history": [{"role": "user", "content": "..."}, {"role": "assistant", "content": "..."}],
//!  "active_tasks": [{"id": "task-a", "goal": "...", "role": "coding", "status": "RUNNING"}],
//!  "expect_action": "delegate", "expect_role": "general", "expect_class": "L2",
//!  "expect_tools": ["calculator"], "expect_risk": "low",
//!  "expect_task_id": "task-a", "expect_follow_up_kind": "cancel",
//!  "by_review_mode": {"direct": {"expect_action": "respond", "expect_role": null}},
//!  "note": "说明"}
//! ```
//! 除 `text` 和 `expect_action` 外都可省略；省略的期望不检查。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use an_core::{
    Message, PolicyVerdict, Risk, Role, RouteDecision, TaskClass, TaskRecord, TaskStatus,
    TurnDecision,
};
use an_runtime::config::AppConfig;
use an_runtime::prompts::ReviewMode;
use an_runtime::store::Store;
use an_runtime::Runtime;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ───────────────────────── 数据集 ─────────────────────────

#[derive(Deserialize, Clone)]
struct Turn {
    role: String,
    content: String,
}

#[derive(Deserialize, Clone)]
struct ActiveTask {
    id: String,
    goal: String,
    #[serde(default = "d_role")]
    role: String,
    #[serde(default = "d_status")]
    status: String,
}
fn d_role() -> String {
    "general".into()
}
fn d_status() -> String {
    "RUNNING".into()
}

/// 期望（可被 by_review_mode 覆盖）
#[derive(Deserialize, Clone, Default, Serialize)]
struct Expect {
    expect_action: Option<String>,
    /// 显式写 null 表示"这个模式下不检查角色"
    #[serde(default, deserialize_with = "nullable")]
    expect_role: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    expect_class: Option<Option<String>>,
    expect_tools: Option<Vec<String>>,
    expect_risk: Option<String>,
    expect_task_id: Option<String>,
    expect_follow_up_kind: Option<String>,
    /// AG.4：manage_agents 的操作（connect / set_default / disable / enable / list / undo）
    expect_agent_op: Option<String>,
    /// AG.4：manage_agents 的助手 id；空字符串表示"没说是哪个"
    expect_agent_id: Option<String>,
}

/// 区分"字段不存在"（None）和"字段为 null"（Some(None)）
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

impl Expect {
    /// 用 other 中出现的字段覆盖自己
    fn overlay(&mut self, other: &Expect) {
        if other.expect_action.is_some() {
            self.expect_action = other.expect_action.clone();
        }
        if other.expect_agent_op.is_some() {
            self.expect_agent_op = other.expect_agent_op.clone();
        }
        if other.expect_agent_id.is_some() {
            self.expect_agent_id = other.expect_agent_id.clone();
        }
        if other.expect_role.is_some() {
            self.expect_role = other.expect_role.clone();
        }
        if other.expect_class.is_some() {
            self.expect_class = other.expect_class.clone();
        }
        if other.expect_tools.is_some() {
            self.expect_tools = other.expect_tools.clone();
        }
        if other.expect_risk.is_some() {
            self.expect_risk = other.expect_risk.clone();
        }
    }
}

#[derive(Deserialize)]
struct Case {
    #[serde(default)]
    id: String,
    text: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    history: Vec<Turn>,
    #[serde(default)]
    active_tasks: Vec<ActiveTask>,
    #[serde(flatten)]
    expect: Expect,
    #[serde(default)]
    by_review_mode: BTreeMap<String, Expect>,
    #[serde(default)]
    note: String,
}

// ───────────────────────── 结果 ─────────────────────────

#[derive(Serialize, Default)]
struct CaseResult {
    id: String,
    text: String,
    tags: Vec<String>,
    expect: Expect,
    got_action: Option<String>,
    got_role: Option<String>,
    got_class: Option<String>,
    /// FT.5：路由回答的两个是/否问题（只在 route_class_questions 打开时有值）
    got_needs_data_tools: Option<bool>,
    got_needs_document: Option<bool>,
    got_tools: Vec<String>,
    got_risk: Option<String>,
    got_task_id: Option<String>,
    got_follow_up_kind: Option<String>,
    got_task_goal: Option<String>,
    got_question: Option<String>,
    /// AG.4：manage_agents 的操作和助手 id
    got_agent_op: Option<String>,
    got_agent_id: Option<String>,
    /// 每项检查的结果：名称 → 是否通过
    checks: BTreeMap<String, bool>,
    pass: bool,
    /// 误判类型：over_delegate / under_delegate / over_clarify / missed_follow_up / wrong_follow_up / wrong_role ...
    error_type: Option<String>,
    turn_ms: u64,
    route_ms: u64,
    /// FT 实验 D：前台判断 / 路由是否开思考重判，值为触发原因
    turn_escalated: Option<String>,
    route_escalated: Option<String>,
    attempts: u32,
    calls: u32,
    error: Option<String>,
    raw_outputs: Vec<String>,
    note: String,
}

fn action_name(d: &TurnDecision) -> &'static str {
    match d {
        TurnDecision::Respond { .. } => "respond",
        TurnDecision::Clarify { .. } => "clarify",
        TurnDecision::Delegate { .. } => "delegate",
        TurnDecision::FollowUp { .. } => "follow_up",
        TurnDecision::ManageAgents { .. } => "manage_agents",
    }
}

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

/// 把数据集里的进行中任务转成 TaskRecord（只有 Turn Controller 会看 id、状态、角色、目标）
fn fake_task(t: &ActiveTask) -> TaskRecord {
    TaskRecord {
        id: t.id.clone(),
        conversation_id: "eval".into(),
        goal: t.goal.clone(),
        role: Role::parse(&t.role).unwrap_or(Role::General),
        status: TaskStatus::parse(&t.status).unwrap_or(TaskStatus::Running),
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
        result: None,
        review: None,
        assignment: None,
        parent_id: None,
        fanout_kind: None,
        branch_id: None,
        template_id: None,
        created_at: an_core::now(),
        updated_at: an_core::now(),
    }
}

fn msg(role: &str, content: &str) -> Message {
    Message {
        id: "eval".into(),
        conversation_id: "eval".into(),
        role: role.into(),
        content: content.into(),
        task_id: None,
        created_at: an_core::now(),
    }
}

/// 根据期望和实际动作判断误判类型
fn classify_error(expected: &str, got: Option<&str>) -> Option<String> {
    let got = got.unwrap_or("error");
    if expected == got {
        return None;
    }
    Some(
        match (expected, got) {
            (_, "error") => "error",
            ("respond" | "clarify", "delegate") => "over_delegate",
            ("delegate", "respond") => "under_delegate",
            (_, "clarify") => "over_clarify",
            ("follow_up", _) => "missed_follow_up",
            (_, "follow_up") => "wrong_follow_up",
            ("clarify", "respond") => "under_clarify",
            _ => "other",
        }
        .to_string(),
    )
}

struct Args {
    config: PathBuf,
    dataset: PathBuf,
    review_mode: Option<ReviewMode>,
    tags: Vec<String>,
    limit: Option<usize>,
    label: Option<String>,
    variant: Option<String>,
    agent_management: bool,
}

/// FT 实验组合：(关思考, A 先写理由, B 规则判风险, D 开思考重判, Q 类别拆成两个是/否问题, T 类别由工具清单算出)
fn variant_flags(v: &str) -> Option<(bool, bool, bool, bool, bool, bool)> {
    Some(match v {
        "E0" => (false, false, false, false, false, false),
        "E1" => (true, false, false, false, false, false),
        "E2" => (true, true, false, false, false, false),
        "E3" => (true, true, true, false, false, false),
        "E4" => (true, true, true, true, false, false),
        "E5" => (false, false, true, false, false, false),
        // FT.5：E6 与 E1 对照（关思考），E7 与 E0 对照（开思考）
        "E6" => (true, false, false, false, true, false),
        "E7" => (false, false, false, false, true, false),
        // FT.5b：E10 与 E1 对照（关思考）。不用 E8、E9：已有测试把它们当作"不存在的变体"的例子
        "E10" => (true, false, false, false, false, true),
        _ => return None,
    })
}

/// 在内存里按实验组合覆盖配置：作用于全部后端（这几个开关只影响 conversation / router 两个角色）。
/// 没写 [backends] 时，按运行时同样的方式用 [llm] 合成 default 后端再覆盖。
fn apply_variant(config: &mut AppConfig, v: &str) -> Result<()> {
    let (off, a, b, d, q, t) =
        variant_flags(v).with_context(|| format!("--variant 只能是 E0 到 E7 或 E10，收到 {v}"))?;
    if config.backends.is_empty() {
        let u = &config.models.base_urls;
        if [
            &u.conversation,
            &u.reply,
            &u.router,
            &u.general,
            &u.reviewer,
        ]
        .iter()
        .any(|x| x.is_some())
        {
            bail!("--variant 需要 [backends] 写法；当前配置用的是旧写法 [models.base_urls]");
        }
        config.backends.insert(
            "default".into(),
            an_llm::BackendConfig {
                base_url: config.llm.base_url.clone(),
                ..Default::default()
            },
        );
    }
    for spec in config.backends.values_mut() {
        spec.front_structured_thinking = off.then_some(false);
        spec.front_reason_first = a.then_some(true);
        spec.front_thinking_escalation = d.then_some(true);
    }
    config.conversation.risk_rules = b;
    config.conversation.route_class_questions = q;
    config.conversation.route_class_from_tools = t;
    Ok(())
}

/// FT.5：从路由的原始输出里读出 needs_data_tools / needs_document，用于离线分析两个答案。
/// 取最后一次同时含两个布尔字段的输出（重试或升级重判时以最后一次为准）；没有时为 None。
fn class_answers(raw: &[String]) -> (Option<bool>, Option<bool>) {
    raw.iter()
        .rev()
        .filter_map(|o| serde_json::from_str::<serde_json::Value>(o).ok())
        .find_map(|v| {
            let t = v.get("needs_data_tools")?.as_bool()?;
            let d = v.get("needs_document")?.as_bool()?;
            Some((Some(t), Some(d)))
        })
        .unwrap_or((None, None))
}

fn local_date() -> String {
    std::process::Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 10)
        .unwrap_or_else(|| an_core::now().chars().take(10).collect())
}

fn dream_once() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--dream") {
        return Ok(false);
    }
    let log_dir = PathBuf::from(args.next().unwrap_or_else(|| "logs".into()));
    let date = args.next().unwrap_or_else(local_date);
    let skill =
        std::fs::read_to_string("tools/websearch/skills/websearch/SKILL.md").unwrap_or_default();
    let out = PathBuf::from(".tmp/dream");
    let wrote = an_runtime::dream::run(&log_dir, &date, &skill, &out)?;
    if wrote {
        println!("已写入草稿 {}/draft.md", out.display());
    } else {
        println!("当天没有失败路由，未写草稿");
    }
    Ok(true)
}

fn export_feedback_draft() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--export-feedback") {
        return Ok(false);
    }
    let db = args
        .next()
        .context("--export-feedback 需要数据库路径和草稿路径")?;
    let out = args.next().context("--export-feedback 需要草稿输出路径")?;
    let out_path = PathBuf::from(&out);
    if out_path.file_name().and_then(|n| n.to_str()) == Some("routing.jsonl") {
        bail!("不能写入 routing.jsonl。草稿确认后应改 evals/make_dataset.py");
    }
    let store = Store::open(std::path::Path::new(&db))?;
    if let Some(dir) = out_path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(&out_path, store.feedback_draft_jsonl()?)?;
    println!("已写入草稿 {}", out_path.display());
    Ok(true)
}

fn parse_args() -> Result<Args> {
    let mut pos = vec![];
    let mut a = Args {
        config: "config.toml".into(),
        dataset: "evals/routing.jsonl".into(),
        review_mode: None,
        tags: vec![],
        limit: None,
        label: None,
        variant: None,
        agent_management: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--review-mode" => {
                let v = it.next().context("--review-mode 需要一个值")?;
                a.review_mode = Some(ReviewMode::parse(&v).with_context(|| {
                    format!("审查模式只能是 reviewer / direct / auto，收到 {v}")
                })?);
            }
            "--tag" => a.tags.push(it.next().context("--tag 需要一个值")?),
            "--limit" => a.limit = Some(it.next().context("--limit 需要一个值")?.parse()?),
            "--label" => a.label = Some(it.next().context("--label 需要一个值")?),
            "--variant" => a.variant = Some(it.next().context("--variant 需要一个值")?),
            "--agent-management" => a.agent_management = true,
            o if o.starts_with("--") => bail!("无法识别的参数 {o}"),
            _ => pos.push(x),
        }
    }
    if let Some(c) = pos.first() {
        a.config = c.into();
    }
    if let Some(d) = pos.get(1) {
        a.dataset = d.into();
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "error".into()),
        )
        .init();
    if export_feedback_draft()? {
        return Ok(());
    }
    if dream_once()? {
        return Ok(());
    }
    let args = parse_args()?;
    let mut config = AppConfig::load(&args.config)?;
    if let Some(m) = args.review_mode {
        config.conversation.review_requests = m;
    }
    if let Some(v) = &args.variant {
        apply_variant(&mut config, v)?;
        println!("实验组合：{v}");
    }
    if args.agent_management {
        config.conversation.agent_management = true;
        println!("对话里管理助手：开（只在内存里覆盖）");
    }
    let mode = config.conversation.review_requests;
    let rt = Runtime::with_mcp_diagnostics(
        config.clone(),
        Store::open(std::path::Path::new(":memory:"))?,
        an_observe::diagnostics_only(),
    )
    .await?;

    let mut cases: Vec<Case> = std::fs::read_to_string(&args.dataset)
        .with_context(|| format!("无法读取评测集 {}", args.dataset.display()))?
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|(i, l)| {
            serde_json::from_str(l).with_context(|| format!("评测集第 {} 行格式错误", i + 1))
        })
        .collect::<Result<_>>()?;
    if !args.tags.is_empty() {
        cases.retain(|c| c.tags.iter().any(|t| args.tags.contains(t)));
    }
    if let Some(n) = args.limit {
        cases.truncate(n);
    }
    let model = config
        .models
        .conversation
        .clone()
        .unwrap_or_else(|| config.llm.model.clone());
    println!(
        "模型服务：{}（前台模型 {model}，结构化模式 {:?}）",
        config.llm.base_url, config.llm.structured_mode
    );
    println!("审查模式：{}", mode.as_str());
    println!(
        "评测集：{}，共 {} 条\n",
        args.dataset.display(),
        cases.len()
    );

    let mut results = vec![];
    for (i, c) in cases.iter().enumerate() {
        // 期望：基础期望 + 当前审查模式下的覆盖
        let mut expect = c.expect.clone();
        if let Some(o) = c.by_review_mode.get(mode.as_str()) {
            expect.overlay(o);
        }
        let mut r = CaseResult {
            id: c.id.clone(),
            text: c.text.clone(),
            tags: c.tags.clone(),
            note: c.note.clone(),
            calls: 1,
            ..Default::default()
        };
        let mut history: Vec<Message> =
            c.history.iter().map(|t| msg(&t.role, &t.content)).collect();
        history.push(msg("user", &c.text));
        let active: Vec<TaskRecord> = c.active_tasks.iter().map(fake_task).collect();

        let t0 = Instant::now();
        // 评测直驱不走会话资料绑定（标准 E0 提示词保持不变）；材料化评测集另卡
        match rt.conversation.decide_turn(&history, &active, None).await {
            Ok(d) => {
                r.turn_ms = t0.elapsed().as_millis() as u64;
                r.attempts += d.attempts;
                r.turn_escalated = d.escalated;
                r.raw_outputs.extend(d.raw_outputs);
                r.got_action = Some(action_name(&d.value).into());
                match &d.value {
                    TurnDecision::Clarify { question } => r.got_question = Some(question.clone()),
                    TurnDecision::FollowUp { task_id, kind, .. } => {
                        r.got_task_id = Some(task_id.clone());
                        r.got_follow_up_kind = Some(enum_str(kind));
                    }
                    TurnDecision::Delegate { task_goal } => {
                        r.got_task_goal = Some(task_goal.clone());
                        r.calls += 1;
                        let t1 = Instant::now();
                        let examples = rt.store.similar_feedback(task_goal, 5).unwrap_or_default();
                        match rt.router.route(task_goal, "", &examples).await {
                            Ok(route) => {
                                r.attempts += route.attempts;
                                r.route_escalated = route.escalated;
                                r.raw_outputs.extend(route.raw_outputs);
                                r.got_role = Some(route.decision.execution_role.as_str().into());
                                r.got_class = Some(format!("{:?}", route.decision.task_class));
                                r.got_tools = route.decision.required_tools.clone();
                                r.got_risk = Some(enum_str(&route.decision.risk_level));
                            }
                            Err(f) => {
                                r.attempts += f.attempts;
                                r.route_escalated = f.escalated;
                                r.raw_outputs.extend(f.raw_outputs);
                                // Policy 拒绝时模型的选择仍然有效（例如 coding 没有可用 agent），照常评分
                                if let Some(d) = f.decision {
                                    r.got_role = Some(d.execution_role.as_str().into());
                                    r.got_class = Some(format!("{:?}", d.task_class));
                                    r.got_tools = d.required_tools.clone();
                                    r.got_risk = Some(enum_str(&d.risk_level));
                                } else {
                                    r.error = Some(f.message);
                                }
                            }
                        }
                        r.route_ms = t1.elapsed().as_millis() as u64;
                        (r.got_needs_data_tools, r.got_needs_document) =
                            class_answers(&r.raw_outputs);
                    }
                    TurnDecision::Respond { .. } => {}
                    TurnDecision::ManageAgents { op, agent_id, .. } => {
                        r.got_agent_op = Some(op.as_str().into());
                        r.got_agent_id = Some(agent_id.clone());
                    }
                }
            }
            Err(e) => {
                r.turn_ms = t0.elapsed().as_millis() as u64;
                r.attempts += e.attempts;
                r.raw_outputs.extend(e.raw_outputs.clone());
                r.error = Some(e.message);
            }
        }

        // ───── 逐项检查 ─────
        let exp_action = expect.expect_action.clone().unwrap_or_default();
        r.checks.insert(
            "action".into(),
            r.got_action.as_deref() == Some(exp_action.as_str()),
        );
        let delegated_ok = r.got_action.as_deref() == Some("delegate") && exp_action == "delegate";
        if delegated_ok {
            if let Some(Some(role)) = &expect.expect_role {
                r.checks
                    .insert("role".into(), r.got_role.as_ref() == Some(role));
            }
            if let Some(Some(class)) = &expect.expect_class {
                r.checks
                    .insert("class".into(), r.got_class.as_ref() == Some(class));
            }
            if let Some(tools) = &expect.expect_tools {
                r.checks.insert(
                    "tools".into(),
                    tools.iter().all(|t| r.got_tools.contains(t)),
                );
            }
            if let Some(risk) = &expect.expect_risk {
                r.checks
                    .insert("risk".into(), r.got_risk.as_ref() == Some(risk));
            }
        }
        // AG.4：动作、操作、助手 id 都对才算对
        if exp_action == "manage_agents" && r.got_action.as_deref() == Some("manage_agents") {
            if let Some(op) = &expect.expect_agent_op {
                r.checks
                    .insert("agent_op".into(), r.got_agent_op.as_ref() == Some(op));
            }
            if let Some(id) = &expect.expect_agent_id {
                r.checks
                    .insert("agent_id".into(), r.got_agent_id.as_ref() == Some(id));
            }
        }
        if exp_action == "follow_up" && r.got_action.as_deref() == Some("follow_up") {
            if let Some(id) = &expect.expect_task_id {
                r.checks
                    .insert("task_id".into(), r.got_task_id.as_ref() == Some(id));
            }
            if let Some(k) = &expect.expect_follow_up_kind {
                r.checks.insert(
                    "follow_up_kind".into(),
                    r.got_follow_up_kind.as_ref() == Some(k),
                );
            }
        }
        r.pass = r.checks.values().all(|v| *v);
        r.error_type = classify_error(&exp_action, r.got_action.as_deref()).or_else(|| {
            r.checks
                .iter()
                .find(|(_, ok)| !**ok)
                .map(|(k, _)| format!("wrong_{k}"))
        });
        r.expect = expect;

        let detail = match r.got_action.as_deref() {
            Some("delegate") => format!(
                "{}/{}/{}",
                r.got_role.as_deref().unwrap_or("-"),
                r.got_class.as_deref().unwrap_or("-"),
                r.got_risk.as_deref().unwrap_or("-")
            ),
            Some("follow_up") => format!(
                "{} {}",
                r.got_follow_up_kind.as_deref().unwrap_or("-"),
                r.got_task_id.as_deref().unwrap_or("-")
            ),
            _ => String::new(),
        };
        println!(
            "[{:>3}/{}] {} {:<14} {:<9} {:<22} {:>6}ms  {}{}",
            i + 1,
            cases.len(),
            if r.pass { "✓" } else { "✗" },
            an_core::truncate_chars(&r.id, 14),
            r.got_action.as_deref().unwrap_or("error"),
            detail,
            r.turn_ms + r.route_ms,
            an_core::truncate_chars(&c.text, 28),
            r.error_type
                .as_ref()
                .map(|e| format!("  ← {e}"))
                .unwrap_or_default(),
        );
        if let Some(e) = &r.error {
            println!("          错误：{e}");
        }
        results.push(r);
    }

    // ───────────────────────── 汇总 ─────────────────────────
    let n = results.len();
    let rate = |ok: usize, total: usize| {
        if total == 0 {
            Value::Null
        } else {
            serde_json::json!({ "ok": ok, "total": total, "rate": (ok as f64 / total as f64 * 1000.0).round() / 10.0 })
        }
    };
    let check_rate = |name: &str| {
        let with: Vec<_> = results.iter().filter_map(|r| r.checks.get(name)).collect();
        rate(with.iter().filter(|v| ***v).count(), with.len())
    };
    let pct = |v: &Value| {
        if v.is_null() {
            "-".to_string()
        } else {
            format!("{}%（{}/{}）", v["rate"], v["ok"], v["total"])
        }
    };

    // 动作混淆矩阵：期望 → 实际
    let labels = ["respond", "clarify", "delegate", "follow_up", "error"];
    let mut matrix: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for r in &results {
        let e = r.expect.expect_action.clone().unwrap_or_default();
        let g = r.got_action.clone().unwrap_or_else(|| "error".into());
        *matrix.entry(e).or_default().entry(g).or_default() += 1;
    }
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    for r in &results {
        if let Some(e) = &r.error_type {
            *errors.entry(e.clone()).or_default() += 1;
        }
    }
    let mut by_tag: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in &results {
        for t in &r.tags {
            let e = by_tag.entry(t.clone()).or_default();
            e.1 += 1;
            if r.pass {
                e.0 += 1;
            }
        }
    }
    let mut turn: Vec<u64> = results.iter().map(|r| r.turn_ms).collect();
    turn.sort_unstable();
    let mut route: Vec<u64> = results
        .iter()
        .filter(|r| r.route_ms > 0)
        .map(|r| r.route_ms)
        .collect();
    route.sort_unstable();
    let retried = results.iter().filter(|r| r.attempts > r.calls).count();
    let failed = results.iter().filter(|r| r.error.is_some()).count();
    let passed = results.iter().filter(|r| r.pass).count();

    println!("\n══════════ 汇总（审查模式 {}）══════════", mode.as_str());
    println!("整体通过率（所有检查项都对）：{}", pct(&rate(passed, n)));
    println!("动作准确率：{}", pct(&check_rate("action")));
    println!(
        "角色准确率：{}    类别：{}    工具：{}    风险：{}",
        pct(&check_rate("role")),
        pct(&check_rate("class")),
        pct(&check_rate("tools")),
        pct(&check_rate("risk"))
    );
    println!(
        "跟进任务 id：{}    跟进类型：{}",
        pct(&check_rate("task_id")),
        pct(&check_rate("follow_up_kind"))
    );
    println!("\n动作混淆矩阵（行 = 期望，列 = 实际）：");
    println!(
        "{:<11}{}",
        "",
        labels
            .iter()
            .map(|l| format!("{l:>10}"))
            .collect::<String>()
    );
    for e in labels.iter().filter(|l| matrix.contains_key(**l)) {
        let row = &matrix[*e];
        println!(
            "{:<11}{}",
            e,
            labels
                .iter()
                .map(|g| format!("{:>10}", row.get(*g).copied().unwrap_or(0)))
                .collect::<String>()
        );
    }
    println!("\n误判类型：");
    if errors.is_empty() {
        println!("  无");
    }
    let explain = |k: &str| match k {
        "over_delegate" => "多派：该直接回答或追问，却交给了后台",
        "under_delegate" => "漏派：该交给后台，却直接回答了",
        "over_clarify" => "多问：不该追问却追问",
        "under_clarify" => "少问：该追问却直接回答",
        "missed_follow_up" => "漏跟进：是对进行中任务的补充或取消，却当成了新请求",
        "wrong_follow_up" => "错跟进：新请求被当成了对进行中任务的跟进",
        "error" => "出错：格式重试后仍失败等",
        "wrong_role" => "动作对，但交给了错误的角色",
        "wrong_class" => "动作对，但 L1-L4 类别错",
        "wrong_tools" => "动作对，但漏了应使用的工具",
        "wrong_risk" => "动作对，但风险等级错",
        "wrong_task_id" => "跟进了错误的任务（代价高）",
        "wrong_follow_up_kind" => "跟进类型错（补充 / 取消搞反）",
        _ => "",
    };
    for (k, v) in &errors {
        println!("  {k:<20} {v:>3}  {}", explain(k));
    }
    println!("\n按标签：");
    for (t, (ok, total)) in &by_tag {
        println!(
            "  {t:<16} {:>5.1}%（{ok}/{total}）",
            *ok as f64 * 100.0 / *total as f64
        );
    }
    println!("\n格式重试样本：{retried}    最终出错样本：{failed}");
    // FT 实验 D：开思考重判的样本数，以及重判 / 未重判样本各自的总延迟（判断 + 路由）
    let mut esc_ms: Vec<u64> = vec![];
    let mut plain_ms: Vec<u64> = vec![];
    let mut esc_reasons: BTreeMap<String, usize> = BTreeMap::new();
    for r in &results {
        let reasons: Vec<&String> = [&r.turn_escalated, &r.route_escalated]
            .into_iter()
            .flatten()
            .collect();
        if reasons.is_empty() {
            plain_ms.push(r.turn_ms + r.route_ms);
        } else {
            esc_ms.push(r.turn_ms + r.route_ms);
            for x in reasons {
                *esc_reasons.entry(x.clone()).or_default() += 1;
            }
        }
    }
    esc_ms.sort_unstable();
    plain_ms.sort_unstable();
    let mut all_ms: Vec<u64> = results.iter().map(|r| r.turn_ms + r.route_ms).collect();
    all_ms.sort_unstable();
    println!(
        "开思考重判样本：{}（{:.1}%）  原因：{:?}",
        esc_ms.len(),
        if n == 0 {
            0.0
        } else {
            esc_ms.len() as f64 * 100.0 / n as f64
        },
        esc_reasons
    );
    println!(
        "整轮（判断 + 路由）P50 / P95：{} / {} ms    其中重判样本 {} / {} ms，未重判样本 {} / {} ms",
        percentile(&all_ms, 0.5),
        percentile(&all_ms, 0.95),
        percentile(&esc_ms, 0.5),
        percentile(&esc_ms, 0.95),
        percentile(&plain_ms, 0.5),
        percentile(&plain_ms, 0.95)
    );
    println!(
        "判断延迟 P50 / P95：{} / {} ms    路由延迟 P50 / P95：{} / {} ms",
        percentile(&turn, 0.5),
        percentile(&turn, 0.95),
        percentile(&route, 0.5),
        percentile(&route, 0.95)
    );

    // ───── 写结果文件 ─────
    let dir = PathBuf::from("evals/results");
    std::fs::create_dir_all(&dir)?;
    let stamp = an_core::now().replace([':', '.'], "-");
    let label = args
        .label
        .clone()
        .or_else(|| args.variant.as_ref().map(|v| format!("FT-{v}")))
        .unwrap_or_else(|| model.replace(['/', ' ', ':'], "_"));
    let base = dir.join(format!("{stamp}-{label}-{}", mode.as_str()));
    let mut f = std::fs::File::create(base.with_extension("jsonl"))?;
    for r in &results {
        writeln!(f, "{}", serde_json::to_string(r)?)?;
    }
    let summary = serde_json::json!({
        "time": an_core::now(),
        "model": model,
        "label": label,
        "review_mode": mode.as_str(),
        "variant": args.variant,
        "dataset": args.dataset,
        "cases": n,
        "pass": rate(passed, n),
        "action": check_rate("action"),
        "role": check_rate("role"),
        "class": check_rate("class"),
        "tools": check_rate("tools"),
        "risk": check_rate("risk"),
        "follow_up_task_id": check_rate("task_id"),
        "follow_up_kind": check_rate("follow_up_kind"),
        "agent_op": check_rate("agent_op"),
        "agent_id": check_rate("agent_id"),
        "agent_management": args.agent_management,
        "confusion": matrix,
        "error_types": errors,
        "by_tag": by_tag.iter().map(|(t, (ok, total))| (t.clone(), rate(*ok, *total))).collect::<BTreeMap<_, _>>(),
        "format_retried": retried,
        "failed": failed,
        "turn_ms": { "p50": percentile(&turn, 0.5), "p95": percentile(&turn, 0.95) },
        "route_ms": { "p50": percentile(&route, 0.5), "p95": percentile(&route, 0.95) },
        "total_ms": { "p50": percentile(&all_ms, 0.5), "p95": percentile(&all_ms, 0.95) },
        "escalated": {
            "cases": esc_ms.len(),
            "reasons": esc_reasons,
            "ms": { "p50": percentile(&esc_ms, 0.5), "p95": percentile(&esc_ms, 0.95) },
            "plain_ms": { "p50": percentile(&plain_ms, 0.5), "p95": percentile(&plain_ms, 0.95) },
        },
    });
    std::fs::write(
        base.with_extension("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!(
        "\n逐条结果（含模型原始输出）：{}",
        base.with_extension("jsonl").display()
    );
    println!(
        "汇总（便于对比不同模型）：{}",
        base.with_extension("summary.json").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_classification() {
        assert_eq!(
            classify_error("respond", Some("delegate")).as_deref(),
            Some("over_delegate")
        );
        assert_eq!(
            classify_error("delegate", Some("respond")).as_deref(),
            Some("under_delegate")
        );
        assert_eq!(
            classify_error("delegate", Some("clarify")).as_deref(),
            Some("over_clarify")
        );
        assert_eq!(
            classify_error("follow_up", Some("delegate")).as_deref(),
            Some("missed_follow_up")
        );
        assert_eq!(
            classify_error("delegate", Some("follow_up")).as_deref(),
            Some("wrong_follow_up")
        );
        assert_eq!(classify_error("delegate", None).as_deref(), Some("error"));
        assert_eq!(classify_error("respond", Some("respond")), None);
    }

    #[test]
    fn ft_variants_apply_to_all_backends() {
        let (mut c, _) =
            AppConfig::parse("[llm]\nbase_url = \"http://x/v1\"\nmodel = \"m\"").unwrap();
        apply_variant(&mut c, "E4").unwrap();
        let s = &c.backends["default"];
        assert_eq!(s.front_structured_thinking, Some(false));
        assert_eq!(s.front_reason_first, Some(true));
        assert_eq!(s.front_thinking_escalation, Some(true));
        assert!(c.conversation.risk_rules);
        apply_variant(&mut c, "E0").unwrap();
        let s = &c.backends["default"];
        assert_eq!(s.front_structured_thinking, None);
        assert_eq!(s.front_thinking_escalation, None);
        assert!(!c.conversation.risk_rules);
        assert!(apply_variant(&mut c, "E9").is_err());
    }

    /// FT.5：E6 = 关思考 + 两问，E7 = 开思考 + 两问；其余组合不开两问
    #[test]
    fn ft_class_question_variants() {
        let (mut c, _) =
            AppConfig::parse("[llm]\nbase_url = \"http://x/v1\"\nmodel = \"m\"").unwrap();
        apply_variant(&mut c, "E6").unwrap();
        let s = &c.backends["default"];
        assert_eq!(s.front_structured_thinking, Some(false));
        assert_eq!(s.front_reason_first, None);
        assert_eq!(s.front_thinking_escalation, None);
        assert!(!c.conversation.risk_rules);
        assert!(c.conversation.route_class_questions);
        apply_variant(&mut c, "E7").unwrap();
        let s = &c.backends["default"];
        assert_eq!(s.front_structured_thinking, None);
        assert!(c.conversation.route_class_questions);
        apply_variant(&mut c, "E1").unwrap();
        assert!(!c.conversation.route_class_questions);
        assert!(apply_variant(&mut c, "E8").is_err());
    }

    /// FT.5：从路由的原始输出里读出两个答案（取最后一次合格的输出）；没有时为 None
    #[test]
    fn class_answers_read_from_raw_outputs() {
        let raw = vec![
            r#"{"action":"delegate","task_goal":"x"}"#.to_string(),
            r#"{"needs_data_tools":false,"needs_document":true}"#.to_string(),
            r#"{"needs_data_tools":true,"needs_document":false,"execution_role":"general"}"#
                .to_string(),
        ];
        assert_eq!(class_answers(&raw), (Some(true), Some(false)));
        let none = vec![
            r#"{"task_class":"L2"}"#.to_string(),
            "不是 JSON".to_string(),
        ];
        assert_eq!(class_answers(&none), (None, None));
    }

    /// FT.5b：E10 = 关思考 + 类别由工具清单算出（对照 E1）；其余组合不开
    #[test]
    fn ft_class_from_tools_variant() {
        let (mut c, _) =
            AppConfig::parse("[llm]\nbase_url = \"http://x/v1\"\nmodel = \"m\"").unwrap();
        apply_variant(&mut c, "E10").unwrap();
        let s = &c.backends["default"];
        assert_eq!(s.front_structured_thinking, Some(false));
        assert!(c.conversation.route_class_from_tools);
        assert!(!c.conversation.route_class_questions);
        apply_variant(&mut c, "E1").unwrap();
        assert!(!c.conversation.route_class_from_tools);
        assert!(apply_variant(&mut c, "E11").is_err());
    }

    #[test]
    fn review_mode_overlay() {
        let c: Case = serde_json::from_str(
            r#"{"text":"x","expect_action":"delegate","expect_role":"reviewer","by_review_mode":{"direct":{"expect_action":"respond","expect_role":null}}}"#,
        )
        .unwrap();
        let mut e = c.expect.clone();
        e.overlay(&c.by_review_mode["direct"]);
        assert_eq!(e.expect_action.as_deref(), Some("respond"));
        assert_eq!(e.expect_role, Some(None));
    }
}
