//! Conversation Runtime：用户唯一的交互入口，对用户负责到底。
//!
//! 每轮对话：
//! 1. Session：把消息写入会话；
//! 2. Context Builder：最近 N 条消息 + 会话中未结束的任务；
//! 3. Turn Controller（LLM 结构化输出）：respond / clarify / delegate / follow_up。
//!    **只输出动作，不写回答内容**——输出很短，判断很快；
//! 4. respond → 单独生成回答，边生成边推送到界面（流式）；
//!    delegate → Router → Orchestrator，立即回复已理解的意图，结果稍后通过事件交付；
//! 5. 记录路由日志（用于调试和积累评测集）。
//!
//! 提示词集中在 [`crate::prompts`]。
//!
//! 与 Router 的分工：Conversation 只决定"要不要委派"，Router 决定"派给谁、怎么做"。

use std::sync::Arc;
use std::time::Instant;

use an_core::{truncate_chars, FollowUpKind, Message, RouteLog, TaskRecord, TurnDecision};
use an_llm::{ChatMessage, LlmClient, Structured, StructuredError};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::bus::Bus;
use crate::conv_lock::ConvLocks;
use crate::orchestrator::Orchestrator;
use crate::prompts::{reply_system_prompt, turn_system_prompt, ReviewMode, TurnPromptInfo};
use crate::router::Router;
use crate::store::Store;

pub struct ConversationRuntime {
    /// 判断"这一轮做什么"用的模型
    llm: LlmClient,
    /// 直接回答时生成回复用的模型
    reply_llm: LlmClient,
    store: Arc<Store>,
    bus: Arc<Bus>,
    router: Arc<Router>,
    orchestrator: Arc<Orchestrator>,
    history_messages: usize,
    prompt_info: TurnPromptInfo,
    stream_replies: bool,
    /// 同一会话同一时刻只处理一条消息
    locks: ConvLocks,
    /// 对话里管理助手（AG.4）；开关关闭时为 None，前台判断与以前完全相同
    agents: Option<Arc<crate::agent_chat::AgentAdmin>>,
}

/// 构造 ConversationRuntime 需要的参数
pub struct ConversationDeps {
    /// 对话里管理助手（AG.4）；开关关闭时为 None
    pub agents: Option<Arc<crate::agent_chat::AgentAdmin>>,
    pub llm: LlmClient,
    pub reply_llm: LlmClient,
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    pub router: Arc<Router>,
    pub orchestrator: Arc<Orchestrator>,
    pub history_messages: usize,
    /// General 工具清单（给 Turn Controller 看）
    pub tools: String,
    pub review_mode: ReviewMode,
    pub stream_replies: bool,
}

/// 一轮对话的处理结果（HTTP 接口直接返回它）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TurnOutcome {
    /// 本轮立即回复给用户的消息
    pub reply: Message,
    pub decision: Option<TurnDecision>,
    /// 如果本轮创建了任务
    pub task: Option<TaskRecord>,
}

/// 模型输出的扁平结构（所有字段必填，不适用的填空字符串），再转换成 TurnDecision
#[derive(Debug, Deserialize)]
struct RawTurn {
    action: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    task_goal: String,
    #[serde(default)]
    task_id: String,
    #[serde(default)]
    follow_up_kind: String,
    #[serde(default)]
    change: String,
    /// AG.4 管理助手：操作 / 助手 id / 角色（开关关闭时 schema 里没有这三个字段）
    #[serde(default)]
    agent_op: String,
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    agent_role: String,
}

/// 开关打开时给前台判断的 schema 加上 manage_agents 和三个枚举字段（AG.4）。
/// `candidates` 是本轮可选的助手 id（每轮按本机情况生成），模型只能从中选。
fn add_manage_agents(mut schema: Value, candidates: &[String]) -> Value {
    if let Some(actions) = schema["properties"]["action"]["enum"].as_array_mut() {
        actions.push(json!("manage_agents"));
    }
    let mut ops: Vec<Value> = an_core::AgentOp::ALL
        .iter()
        .map(|o| json!(o.as_str()))
        .collect();
    ops.push(json!(""));
    let mut ids: Vec<Value> = candidates.iter().map(|c| json!(c)).collect();
    ids.push(json!(""));
    let props = schema["properties"]
        .as_object_mut()
        .expect("schema 有 properties");
    props.insert("agent_op".into(), json!({ "type": "string", "enum": ops }));
    props.insert("agent_id".into(), json!({ "type": "string", "enum": ids }));
    props.insert(
        "agent_role".into(),
        json!({ "type": "string", "enum": ["coding", "general", "reviewer", ""] }),
    );
    if let Some(req) = schema["required"].as_array_mut() {
        req.extend([json!("agent_op"), json!("agent_id"), json!("agent_role")]);
    }
    schema
}

/// `reason_first`：FT 实验 A，不思考时让模型先写简短的判断依据（放在第一个字段）
fn turn_schema(reason_first: bool) -> Value {
    if reason_first {
        return json!({
            "type": "object",
            "properties": {
                "reason": { "type": "string", "maxLength": crate::router::REASON_MAX_CHARS },
                "action": { "type": "string", "enum": ["respond", "clarify", "delegate", "follow_up"] },
                "question": { "type": "string" },
                "task_goal": { "type": "string" },
                "task_id": { "type": "string" },
                "follow_up_kind": { "type": "string", "enum": ["supplement", "cancel", ""] },
                "change": { "type": "string" }
            },
            "required": ["reason", "action", "question", "task_goal", "task_id", "follow_up_kind", "change"]
        });
    }
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": ["respond", "clarify", "delegate", "follow_up"] },
            "question": { "type": "string" },
            "task_goal": { "type": "string" },
            "task_id": { "type": "string" },
            "follow_up_kind": { "type": "string", "enum": ["supplement", "cancel", ""] },
            "change": { "type": "string" }
        },
        "required": ["action", "question", "task_goal", "task_id", "follow_up_kind", "change"]
    })
}

/// 把扁平输出转换成 TurnDecision，同时做业务校验（不通过会反馈给模型重试）
fn to_decision(
    raw: &RawTurn,
    active: &[TaskRecord],
    agents: Option<&[String]>,
) -> Result<TurnDecision, String> {
    let need = |v: &str, field: &str| {
        if v.trim().is_empty() {
            Err(format!("action={} 时 {field} 不能为空", raw.action))
        } else {
            Ok(())
        }
    };
    match raw.action.as_str() {
        // 回答内容在下一步单独生成，这里先留空
        "respond" => Ok(TurnDecision::Respond {
            reply: String::new(),
        }),
        "clarify" => need(&raw.question, "question").map(|_| TurnDecision::Clarify {
            question: raw.question.clone(),
        }),
        "delegate" => need(&raw.task_goal, "task_goal").map(|_| TurnDecision::Delegate {
            task_goal: raw.task_goal.clone(),
        }),
        "follow_up" => {
            if !active.iter().any(|t| t.id == raw.task_id) {
                return Err(format!(
                    "task_id `{}` 不是进行中的任务；进行中的任务：{}",
                    raw.task_id,
                    if active.is_empty() {
                        "无（不能使用 follow_up）".into()
                    } else {
                        active
                            .iter()
                            .map(|t| t.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ));
            }
            let kind = match raw.follow_up_kind.as_str() {
                "supplement" => FollowUpKind::Supplement,
                "cancel" => FollowUpKind::Cancel,
                _ => return Err("follow_up 时 follow_up_kind 必须是 supplement 或 cancel".into()),
            };
            if kind == FollowUpKind::Supplement {
                need(&raw.change, "change")?;
            }
            Ok(TurnDecision::FollowUp {
                task_id: raw.task_id.clone(),
                kind,
                change: raw.change.clone(),
            })
        }
        // AG.4：只有开关打开（agents 为 Some）时才接受；助手 id 必须在本轮候选里
        "manage_agents" if agents.is_some() => {
            let Some(op) = an_core::AgentOp::parse(&raw.agent_op) else {
                return Err(format!(
                    "action=manage_agents 时 agent_op 必须是 connect / set_default / disable / enable / list / undo，收到 `{}`",
                    raw.agent_op
                ));
            };
            let candidates = agents.unwrap_or_default();
            if !raw.agent_id.is_empty() && !candidates.iter().any(|c| c == &raw.agent_id) {
                return Err(format!(
                    "agent_id `{}` 不在候选里；可选：{}（没说是哪个就填空字符串）",
                    raw.agent_id,
                    candidates.join(", ")
                ));
            }
            Ok(TurnDecision::ManageAgents {
                op,
                agent_id: raw.agent_id.clone(),
                role: an_core::Role::parse(&raw.agent_role),
            })
        }
        other => Err(format!("未知 action：{other}")),
    }
}

impl ConversationRuntime {
    pub fn new(d: ConversationDeps) -> Self {
        Self {
            llm: d.llm,
            reply_llm: d.reply_llm,
            store: d.store,
            bus: d.bus,
            router: d.router,
            orchestrator: d.orchestrator,
            history_messages: d.history_messages,
            prompt_info: TurnPromptInfo {
                tools: d.tools,
                review_mode: d.review_mode,
            },
            stream_replies: d.stream_replies,
            locks: ConvLocks::new(),
            agents: d.agents,
        }
    }

    pub fn review_mode(&self) -> ReviewMode {
        self.prompt_info.review_mode
    }

    /// 评测用：换一种审查模式（不影响其他配置）
    pub fn set_review_mode(&mut self, mode: ReviewMode) {
        self.prompt_info.review_mode = mode;
    }

    /// Turn Controller：只做决策，不产生副作用（评测脚本也直接调用它）
    pub async fn decide_turn(
        &self,
        history: &[Message],
        active: &[TaskRecord],
    ) -> std::result::Result<Structured<TurnDecision>, StructuredError> {
        let query = history
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let examples = self.store.similar_feedback(&query, 5).unwrap_or_default();
        // AG.4：开关打开时，追加"管理助手"一节，并给出本轮候选助手
        let candidates = self.agents.as_ref().map(|a| a.candidate_ids());
        let mut system = turn_system_prompt(&self.prompt_info, active, &examples);
        if let Some(a) = &self.agents {
            system.push_str(&a.prompt_section());
        }
        let mut messages = vec![ChatMessage::system(system)];
        for m in history {
            messages.push(if m.role == "user" {
                ChatMessage::user(&m.content)
            } else {
                ChatMessage::assistant(&m.content)
            });
        }
        // 先拿到扁平结构，再转换（转换中的校验错误会触发重试）
        let validate = |r: &RawTurn| to_decision(r, active, candidates.as_deref()).map(|_| ());
        let front = self.llm.front_options();
        let schema = match &candidates {
            Some(c) => add_manage_agents(turn_schema(front.reason_first), c),
            None => turn_schema(front.reason_first),
        };
        let raw = if front.thinking_escalation {
            // FT 实验 D：不思考只试一次；格式或业务校验不合格（例如 delegate 但目标为空）就开思考重判一次
            let started = std::time::Instant::now();
            let first = self
                .llm
                .with_max_retries(0)
                .chat_json::<RawTurn, _>(&messages, "turn_decision", &schema, validate)
                .await;
            match first {
                Err(e) if e.code == an_core::codes::LLM_FORMAT => {
                    let think = self
                        .llm
                        .with_thinking()
                        .chat_json::<RawTurn, _>(&messages, "turn_decision", &schema, validate)
                        .await;
                    let total_ms = started.elapsed().as_millis() as u64;
                    tracing::info!(
                        event = "llm.escalated",
                        schema = "turn_decision",
                        reason = "format",
                        fast_ms = e.latency_ms,
                        total_ms,
                        ok = think.is_ok(),
                        "前台判断可疑，已开思考重判"
                    );
                    let mut raw_outputs = e.raw_outputs;
                    match think {
                        Ok(s) => {
                            raw_outputs.extend(s.raw_outputs);
                            Structured {
                                value: s.value,
                                raw_outputs,
                                attempts: e.attempts + s.attempts,
                                latency_ms: total_ms,
                                escalated: Some("format".into()),
                            }
                        }
                        Err(mut t) => {
                            raw_outputs.extend(t.raw_outputs);
                            t.raw_outputs = raw_outputs;
                            t.attempts += e.attempts;
                            t.latency_ms = total_ms;
                            return Err(t);
                        }
                    }
                }
                other => other?,
            }
        } else {
            self.llm
                .chat_json::<RawTurn, _>(&messages, "turn_decision", &schema, validate)
                .await?
        };
        let value = to_decision(&raw.value, active, candidates.as_deref()).expect("已在校验中通过");
        Ok(Structured {
            value,
            raw_outputs: raw.raw_outputs,
            attempts: raw.attempts,
            latency_ms: raw.latency_ms,
            escalated: raw.escalated,
        })
    }

    /// Context Builder（MVP 版）：把最近几条消息整理成文字，交给执行 Agent 作背景
    fn context_text(history: &[Message]) -> String {
        history
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|m| {
                format!(
                    "{}：{}",
                    if m.role == "user" { "用户" } else { "助手" },
                    truncate_chars(&m.content, 500)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 处理用户的一条消息
    pub async fn handle_message(&self, conversation_id: &str, text: &str) -> Result<TurnOutcome> {
        // 同一会话的消息排队处理；排队时间不计入本轮耗时
        let turn_guard = self.locks.lock(conversation_id).await;
        if turn_guard.waited_ms >= 200 {
            tracing::info!(event = "conversation.queued", conv_id = %conversation_id, waited_ms = turn_guard.waited_ms, "同一会话上一条消息还在处理，已排队等待");
        }
        let started = Instant::now();
        let conv = self
            .store
            .get_conversation(conversation_id)?
            .context("会话不存在")?;

        // 1. 写入用户消息；第一条消息作为会话标题
        let is_first = self
            .store
            .list_messages(conversation_id, Some(1))?
            .is_empty();
        self.store
            .add_message(conversation_id, "user", text, None)?;
        if is_first && conv.title == "新会话" {
            self.store
                .rename_conversation(conversation_id, &truncate_chars(text, 24))?;
        }

        // 2. 构建上下文
        let history = self
            .store
            .list_messages(conversation_id, Some(self.history_messages))?;
        let active = self.store.active_tasks(conversation_id)?;
        let mut log = RouteLog {
            id: an_core::new_id("route"),
            conversation_id: conversation_id.into(),
            user_text: text.into(),
            turn_decision: None,
            route_decision: None,
            policy: None,
            task_id: None,
            raw_outputs: vec![],
            attempts: 0,
            latency_ms: 0,
            error: None,
            created_at: an_core::now(),
        };

        // 3. Turn Controller
        let decision = match self.decide_turn(&history, &active).await {
            Ok(d) => {
                log.raw_outputs.extend(d.raw_outputs);
                log.attempts += d.attempts;
                d.value
            }
            Err(e) => {
                log.raw_outputs.extend(e.raw_outputs.clone());
                log.attempts += e.attempts;
                log.error = Some(e.to_string());
                return self.finish(
                    log,
                    started,
                    None,
                    None,
                    &format!(
                        "抱歉，我没能理解这条消息（模型输出格式错误：{}）。可以换个说法再试一次。",
                        e.message
                    ),
                );
            }
        };
        log.turn_decision = Some(decision.clone());

        // 4. 按决策执行
        let ctx = Self::context_text(&history);
        match &decision {
            TurnDecision::Respond { .. } => {
                let reply_id = an_core::new_id("msg");
                let reply = self
                    .generate_reply(conversation_id, &reply_id, &history)
                    .await;
                let decision = TurnDecision::Respond {
                    reply: reply.clone(),
                };
                self.finish_with_id(log, started, Some(decision), None, &reply, Some(&reply_id))
            }
            TurnDecision::ManageAgents { op, agent_id, role } => {
                // 后续全部由代码完成：生成卡片，不再调用模型
                let (content, card) = match &self.agents {
                    Some(a) => a.wizard(*op, agent_id, *role),
                    None => (
                        "现在没有打开对话里管理助手的功能，可以在右侧「助手」页签里操作。".into(),
                        None,
                    ),
                };
                self.finish_with_card(log, started, decision.clone(), &content, card)
            }
            TurnDecision::Clarify { question } => {
                self.finish(log, started, Some(decision.clone()), None, question)
            }
            TurnDecision::Delegate { task_goal } => {
                let goal = task_goal.clone();
                self.delegate(log, started, decision, &goal, &ctx, None)
                    .await
            }
            TurnDecision::FollowUp {
                task_id,
                kind,
                change,
            } => {
                let old = self.store.get_task(task_id)?.context("任务不存在")?;
                match kind {
                    FollowUpKind::Cancel => {
                        let cancelled = self.orchestrator.cancel(task_id)?;
                        let msg = if cancelled {
                            format!("已取消任务：{}", truncate_chars(&old.goal, 60))
                        } else {
                            "这个任务已经结束，无需取消。".into()
                        };
                        log.task_id = Some(task_id.clone());
                        self.finish(log, started, Some(decision.clone()), None, &msg)
                    }
                    FollowUpKind::Supplement => {
                        // 先路由并校验补充要求，成功后才取消旧任务。
                        let goal = format!("{}\n\n补充要求：{}", old.goal, change);
                        self.delegate(log, started, decision.clone(), &goal, &ctx, Some(task_id))
                            .await
                    }
                }
            }
        }
    }

    /// 委派：Router 路由 + Policy 校验 + Orchestrator 派发
    async fn delegate(
        &self,
        mut log: RouteLog,
        started: Instant,
        decision: TurnDecision,
        goal: &str,
        ctx: &str,
        replaces_task: Option<&str>,
    ) -> Result<TurnOutcome> {
        let conversation_id = log.conversation_id.clone();
        let examples = self.store.similar_feedback(goal, 5).unwrap_or_default();
        match self.router.route(goal, ctx, &examples).await {
            Ok(route) => {
                log.raw_outputs.extend(route.raw_outputs);
                log.attempts += route.attempts;
                log.route_decision = Some(route.decision.clone());
                log.policy = Some(route.policy.clone());
                if let Some(task_id) = replaces_task {
                    self.orchestrator.cancel(task_id)?;
                }
                let task = self.orchestrator.submit(
                    &conversation_id,
                    goal,
                    ctx,
                    route.decision.clone(),
                    route.policy.clone(),
                )?;
                log.task_id = Some(task.id.clone());
                let reply = if route.policy.auto_execute {
                    // 完整显示用户意图（只压缩连续空白），不再截断：用户要求能看到自己的完整要求
                    let intent = goal.split_whitespace().collect::<Vec<_>>().join(" ");
                    format!("明白，我来根据您的要求做：{}", intent)
                } else {
                    format!(
                        "这个任务被判定为需要人工审批（{}）。请在任务面板中确认后我再开始执行。",
                        route.policy.notes.join("；")
                    )
                };
                self.finish(log, started, Some(decision), Some(task), &reply)
            }
            Err(f) => {
                log.raw_outputs.extend(f.raw_outputs);
                log.attempts += f.attempts;
                let role = f.decision.as_ref().map(|d| d.execution_role);
                log.route_decision = f.decision.map(|d| *d);
                log.error = Some(f.message.clone());
                // 没有可用的执行者：不改派给别的角色，由前台模型用大白话说明并给出替代办法（AG.2）
                let reply = if f.message.contains(an_core::codes::ROUTE_NO_AGENT) {
                    self.no_agent_reply(goal, role, &f.message).await
                } else {
                    format!("这个任务暂时无法派发：{}", f.message)
                };
                self.finish(log, started, Some(decision), None, &reply)
            }
        }
    }

    /// 没有可用执行者时的回复（AG.2）：由前台模型生成，面向非专业用户；模型不可用时用大白话模板兜底
    async fn no_agent_reply(
        &self,
        goal: &str,
        role: Option<an_core::Role>,
        detail: &str,
    ) -> String {
        let label = role.map(crate::registry::role_label).unwrap_or("这类任务");
        let system = format!(
            "你是 AgentNexus 的助手。用户刚提出的任务现在没有可用的执行者，所以还没有开始做。\n\
             请用非专业用户能看懂的大白话回复，要求：\n\
             1. 先用一句话说明：现在没有能做「{label}」的助手，这件事还没有开始；\n\
             2. 简单说明可能的原因：对应的助手被停用了，或者还没有安装；\n\
             3. 告诉用户可以怎么办：接入或恢复启用一个能做「{label}」的助手后再试；如果用户愿意，也可以在对话里直接给出建议或示例（例如修改思路、示例代码），由用户自己动手；\n\
             4. 不要堆技术术语，不要把错误码放在开头；如需提到错误码，放在最后的括号里；\n\
             5. 不要假装任务已经完成，不要编造结果；简洁，使用中文。\n\n\
             技术细节（帮助你理解，不要原样照抄给用户）：{detail}"
        );
        let messages = [ChatMessage::system(system), ChatMessage::user(goal)];
        match self.reply_llm.chat(&messages).await {
            // 先大白话，错误码只放在最后的括号里：便于用户截图求助、便于排查
            Ok(text) if !text.trim().is_empty() => {
                if text.contains(an_core::codes::ROUTE_NO_AGENT) {
                    text
                } else {
                    format!(
                        "{}\n\n（技术信息：{}）",
                        text.trim_end(),
                        an_core::codes::ROUTE_NO_AGENT
                    )
                }
            }
            _ => format!(
                "现在没有能做「{label}」的助手（可能被停用了，或者还没有安装），所以这件事还没有开始做。\n\
                 你可以：\n\
                 1. 接入或恢复启用一个能做「{label}」的助手，然后再说一次；\n\
                 2. 也可以让我在对话里直接给出建议或示例，由你自己动手。\n\
                 （技术信息：{}）",
                an_core::codes::ROUTE_NO_AGENT
            ),
        }
    }

    /// 直接回答：单独生成回复。开启流式时边生成边推送到界面（片段的 stream_id = 最终消息 id）；
    /// 流式失败时退回一次性生成；都失败时给出带错误码的说明。
    async fn generate_reply(
        &self,
        conversation_id: &str,
        reply_id: &str,
        history: &[Message],
    ) -> String {
        let mut messages = vec![ChatMessage::system(reply_system_prompt(&self.prompt_info))];
        for m in history {
            messages.push(if m.role == "user" {
                ChatMessage::user(&m.content)
            } else {
                ChatMessage::assistant(&m.content)
            });
        }
        let started = Instant::now();
        if self.stream_replies {
            let bus = self.bus.clone();
            let result = self
                .reply_llm
                .chat_stream(&messages, |delta| {
                    bus.assistant_delta(conversation_id, reply_id, delta)
                })
                .await;
            match result {
                Ok(text) => {
                    tracing::info!(event = "conversation.reply", conv_id = %conversation_id, streamed = true, chars = text.chars().count(), elapsed_ms = started.elapsed().as_millis() as u64);
                    return text;
                }
                Err(e) => {
                    tracing::warn!(event = "conversation.reply_stream_failed", code = an_core::codes::LLM_HTTP, conv_id = %conversation_id, error = %format!("{e:#}"), "流式生成失败，改用一次性生成")
                }
            }
        }
        match self.reply_llm.chat(&messages).await {
            Ok(text) if !text.trim().is_empty() => {
                tracing::info!(event = "conversation.reply", conv_id = %conversation_id, streamed = false, chars = text.chars().count(), elapsed_ms = started.elapsed().as_millis() as u64);
                text
            }
            Ok(_) => format!(
                "[{}] 抱歉，模型没有返回内容，请再试一次。",
                an_core::codes::LLM_FORMAT
            ),
            Err(e) => {
                tracing::error!(event = "conversation.reply_failed", code = an_core::codes::LLM_UNREACHABLE, conv_id = %conversation_id, error = %format!("{e:#}"));
                format!(
                    "[{}] 抱歉，生成回答失败：{e:#}",
                    an_core::codes::LLM_UNREACHABLE
                )
            }
        }
    }

    /// 收尾：写入助手回复和路由日志
    fn finish(
        &self,
        log: RouteLog,
        started: Instant,
        decision: Option<TurnDecision>,
        task: Option<TaskRecord>,
        reply: &str,
    ) -> Result<TurnOutcome> {
        self.finish_with_id(log, started, decision, task, reply, None)
    }

    /// 同上，可指定回复消息的 id（流式回复用）
    /// 结束一轮并写一条带卡片的回复（AG.4）。卡片里有确认卡时，记下它来自哪条消息，确认后结果写回这里
    fn finish_with_card(
        &self,
        log: RouteLog,
        started: Instant,
        decision: TurnDecision,
        reply: &str,
        card: Option<an_core::ChatCard>,
    ) -> Result<TurnOutcome> {
        let Some(card) = card else {
            return self.finish(log, started, Some(decision), None, reply);
        };
        let msg = match &self.agents {
            Some(a) => a.post_card(&log.conversation_id, reply, &card)?.message,
            None => {
                self.store
                    .add_message_with_card(&log.conversation_id, "assistant", reply, &card)?
            }
        };
        self.log_turn(log, started, Some(decision), None, msg)
    }

    fn finish_with_id(
        &self,
        log: RouteLog,
        started: Instant,
        decision: Option<TurnDecision>,
        task: Option<TaskRecord>,
        reply: &str,
        reply_id: Option<&str>,
    ) -> Result<TurnOutcome> {
        let task_ref = task.as_ref().map(|t| t.id.as_str());
        let msg = match reply_id {
            Some(id) => self.store.add_message_with_id(
                id,
                &log.conversation_id,
                "assistant",
                reply,
                task_ref,
            )?,
            None => self
                .store
                .add_message(&log.conversation_id, "assistant", reply, task_ref)?,
        };
        self.log_turn(log, started, decision, task, msg)
    }

    /// 记录本轮的埋点和路由日志，返回结果
    fn log_turn(
        &self,
        mut log: RouteLog,
        started: Instant,
        decision: Option<TurnDecision>,
        task: Option<TaskRecord>,
        msg: Message,
    ) -> Result<TurnOutcome> {
        log.latency_ms = started.elapsed().as_millis() as u64;
        // 埋点：每轮对话一条，记录决策、路由、重试次数和耗时
        let action = log
            .turn_decision
            .as_ref()
            .map(|d| {
                serde_json::to_value(d)
                    .ok()
                    .and_then(|v| v["action"].as_str().map(String::from))
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| "none".into());
        let role = log
            .route_decision
            .as_ref()
            .map(|r| r.execution_role.as_str())
            .unwrap_or("-");
        match &log.error {
            Some(err) => {
                tracing::warn!(event = "conversation.turn", conv_id = %log.conversation_id, action = %action, role, task_id = ?log.task_id, attempts = log.attempts, elapsed_ms = log.latency_ms, error = %err)
            }
            None => {
                tracing::info!(event = "conversation.turn", conv_id = %log.conversation_id, action = %action, role, task_id = ?log.task_id, attempts = log.attempts, elapsed_ms = log.latency_ms)
            }
        }
        if let Err(e) = self.store.insert_route_log(&log) {
            tracing::error!(error = %e, "写入路由日志失败");
        }
        Ok(TurnOutcome {
            reply: msg,
            decision,
            task,
        })
    }
}
