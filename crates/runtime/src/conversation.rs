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
}

/// 构造 ConversationRuntime 需要的参数
pub struct ConversationDeps {
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
}

fn turn_schema() -> Value {
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
fn to_decision(raw: &RawTurn, active: &[TaskRecord]) -> Result<TurnDecision, String> {
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
        let mut messages = vec![ChatMessage::system(turn_system_prompt(
            &self.prompt_info,
            active,
        ))];
        for m in history {
            messages.push(if m.role == "user" {
                ChatMessage::user(&m.content)
            } else {
                ChatMessage::assistant(&m.content)
            });
        }
        // 先拿到扁平结构，再转换（转换中的校验错误会触发重试）
        let active_owned = active.to_vec();
        let raw = self
            .llm
            .chat_json::<RawTurn, _>(&messages, "turn_decision", &turn_schema(), move |r| {
                to_decision(r, &active_owned).map(|_| ())
            })
            .await?;
        let value = to_decision(&raw.value, active).expect("已在校验中通过");
        Ok(Structured {
            value,
            raw_outputs: raw.raw_outputs,
            attempts: raw.attempts,
            latency_ms: raw.latency_ms,
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
            TurnDecision::Clarify { question } => {
                self.finish(log, started, Some(decision.clone()), None, question)
            }
            TurnDecision::Delegate { task_goal } => {
                let goal = task_goal.clone();
                self.delegate(log, started, decision, &goal, &ctx).await
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
                        // MVP 做法：取消旧任务，用合并后的目标重新路由和派发
                        self.orchestrator.cancel(task_id)?;
                        let goal = format!("{}\n\n补充要求：{}", old.goal, change);
                        self.delegate(log, started, decision.clone(), &goal, &ctx)
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
    ) -> Result<TurnOutcome> {
        let conversation_id = log.conversation_id.clone();
        match self.router.route(goal, ctx).await {
            Ok(route) => {
                log.raw_outputs.extend(route.raw_outputs);
                log.attempts += route.attempts;
                log.route_decision = Some(route.decision.clone());
                log.policy = Some(route.policy.clone());
                let task = self.orchestrator.submit(
                    &conversation_id,
                    goal,
                    ctx,
                    route.decision.clone(),
                    route.policy.clone(),
                )?;
                log.task_id = Some(task.id.clone());
                let reply = if route.policy.auto_execute {
                    let intent = goal.split_whitespace().collect::<Vec<_>>().join(" ");
                    format!("明白，我来根据您的要求做：{}", truncate_chars(&intent, 80))
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
                log.route_decision = f.decision;
                log.error = Some(f.message.clone());
                self.finish(
                    log,
                    started,
                    Some(decision),
                    None,
                    &format!("这个任务暂时无法派发：{}", f.message),
                )
            }
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
    fn finish_with_id(
        &self,
        mut log: RouteLog,
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
