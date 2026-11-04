//! Conversation Runtime：用户唯一的交互入口，对用户负责到底。
//!
//! 每轮对话：
//! 1. Session：把消息写入会话；
//! 2. Context Builder：最近 N 条消息 + 会话中未结束的任务；
//! 3. Turn Controller（LLM 结构化输出）：respond / clarify / delegate / follow_up；
//! 4. delegate → Router → Orchestrator，立即回复"已开始处理"，结果稍后通过事件交付；
//! 5. 记录路由日志（用于调试和积累评测集）。
//!
//! 与 Router 的分工：Conversation 只决定"要不要委派"，Router 决定"派给谁、怎么做"。

use std::sync::Arc;
use std::time::Instant;

use an_core::{truncate_chars, FollowUpKind, Message, RouteLog, TaskRecord, TurnDecision};
use an_llm::{ChatMessage, LlmClient, Structured, StructuredError};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::orchestrator::Orchestrator;
use crate::router::Router;
use crate::store::Store;

pub struct ConversationRuntime {
    llm: LlmClient,
    store: Arc<Store>,
    router: Arc<Router>,
    orchestrator: Arc<Orchestrator>,
    history_messages: usize,
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
    reply: String,
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
            "reply": { "type": "string" },
            "question": { "type": "string" },
            "task_goal": { "type": "string" },
            "task_id": { "type": "string" },
            "follow_up_kind": { "type": "string", "enum": ["supplement", "cancel", ""] },
            "change": { "type": "string" }
        },
        "required": ["action", "reply", "question", "task_goal", "task_id", "follow_up_kind", "change"]
    })
}

/// 把扁平输出转换成 TurnDecision，同时做业务校验（不通过会反馈给模型重试）
fn to_decision(raw: &RawTurn, active: &[TaskRecord]) -> Result<TurnDecision, String> {
    let need = |v: &str, field: &str| if v.trim().is_empty() { Err(format!("action={} 时 {field} 不能为空", raw.action)) } else { Ok(()) };
    match raw.action.as_str() {
        "respond" => need(&raw.reply, "reply").map(|_| TurnDecision::Respond { reply: raw.reply.clone() }),
        "clarify" => need(&raw.question, "question").map(|_| TurnDecision::Clarify { question: raw.question.clone() }),
        "delegate" => need(&raw.task_goal, "task_goal").map(|_| TurnDecision::Delegate { task_goal: raw.task_goal.clone() }),
        "follow_up" => {
            if !active.iter().any(|t| t.id == raw.task_id) {
                return Err(format!(
                    "task_id `{}` 不是进行中的任务；进行中的任务：{}",
                    raw.task_id,
                    if active.is_empty() { "无（不能使用 follow_up）".into() } else { active.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ") }
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
            Ok(TurnDecision::FollowUp { task_id: raw.task_id.clone(), kind, change: raw.change.clone() })
        }
        other => Err(format!("未知 action：{other}")),
    }
}

impl ConversationRuntime {
    pub fn new(llm: LlmClient, store: Arc<Store>, router: Arc<Router>, orchestrator: Arc<Orchestrator>, history_messages: usize) -> Self {
        Self { llm, store, router, orchestrator, history_messages }
    }

    fn system_prompt(active: &[TaskRecord]) -> String {
        let tasks = if active.is_empty() {
            "无".to_string()
        } else {
            active
                .iter()
                .map(|t| format!("- task_id={}，状态={}，角色={}，目标：{}", t.id, t.status.as_str(), t.role.as_str(), truncate_chars(&t.goal, 120)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "你是 AgentNexus 的会话负责人（Conversation Agent），代表用户协调后台的执行 Agent。\n\
             后台能力：通用任务（分析、计算、读写文件、生成文档）、代码开发（修改代码仓库）、独立审查。\n\n\
             针对用户的最新消息，决定这一轮的 action：\n\
             - respond：闲聊、常识性问答、解释概念等你能直接可靠回答的，把回答写在 reply；\n\
             - clarify：需求含糊、缺少关键信息、无法执行时，把要追问的问题写在 question；\n\
             - delegate：需要计算、读写文件、生成文档报告、写代码改代码、审查内容等需要后台执行的，\n\
               把整理清楚、可独立执行的任务描述写在 task_goal（要包含对话中的必要细节，执行者看不到对话）；\n\
             - follow_up：用户的消息是针对下面某个进行中任务的补充要求（supplement）或取消（cancel），\n\
               填写 task_id、follow_up_kind，补充要求写在 change。\n\
             不适用的字段填空字符串。\n\n\
             ## 进行中的任务\n{tasks}"
        )
    }

    /// Turn Controller：只做决策，不产生副作用（评测脚本也直接调用它）
    pub async fn decide_turn(&self, history: &[Message], active: &[TaskRecord]) -> std::result::Result<Structured<TurnDecision>, StructuredError> {
        let mut messages = vec![ChatMessage::system(Self::system_prompt(active))];
        for m in history {
            messages.push(if m.role == "user" { ChatMessage::user(&m.content) } else { ChatMessage::assistant(&m.content) });
        }
        // 先拿到扁平结构，再转换（转换中的校验错误会触发重试）
        let active_owned = active.to_vec();
        let raw = self
            .llm
            .chat_json::<RawTurn, _>(&messages, "turn_decision", &turn_schema(), move |r| to_decision(r, &active_owned).map(|_| ()))
            .await?;
        let value = to_decision(&raw.value, active).expect("已在校验中通过");
        Ok(Structured { value, raw_outputs: raw.raw_outputs, attempts: raw.attempts, latency_ms: raw.latency_ms })
    }

    /// Context Builder（MVP 版）：把最近几条消息整理成文字，交给执行 Agent 作背景
    fn context_text(history: &[Message]) -> String {
        history
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|m| format!("{}：{}", if m.role == "user" { "用户" } else { "助手" }, truncate_chars(&m.content, 500)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 处理用户的一条消息
    pub async fn handle_message(&self, conversation_id: &str, text: &str) -> Result<TurnOutcome> {
        let started = Instant::now();
        let conv = self.store.get_conversation(conversation_id)?.context("会话不存在")?;

        // 1. 写入用户消息；第一条消息作为会话标题
        let is_first = self.store.list_messages(conversation_id, Some(1))?.is_empty();
        self.store.add_message(conversation_id, "user", text, None)?;
        if is_first && conv.title == "新会话" {
            self.store.rename_conversation(conversation_id, &truncate_chars(text, 24))?;
        }

        // 2. 构建上下文
        let history = self.store.list_messages(conversation_id, Some(self.history_messages))?;
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
                return self.finish(log, started, None, None, &format!("抱歉，我没能理解这条消息（模型输出格式错误：{}）。可以换个说法再试一次。", e.message));
            }
        };
        log.turn_decision = Some(decision.clone());

        // 4. 按决策执行
        let ctx = Self::context_text(&history);
        match &decision {
            TurnDecision::Respond { reply } => self.finish(log, started, Some(decision.clone()), None, reply),
            TurnDecision::Clarify { question } => self.finish(log, started, Some(decision.clone()), None, question),
            TurnDecision::Delegate { task_goal } => {
                let goal = task_goal.clone();
                self.delegate(log, started, decision, &goal, &ctx).await
            }
            TurnDecision::FollowUp { task_id, kind, change } => {
                let old = self.store.get_task(task_id)?.context("任务不存在")?;
                match kind {
                    FollowUpKind::Cancel => {
                        let cancelled = self.orchestrator.cancel(task_id)?;
                        let msg = if cancelled { format!("已取消任务：{}", truncate_chars(&old.goal, 60)) } else { "这个任务已经结束，无需取消。".into() };
                        log.task_id = Some(task_id.clone());
                        self.finish(log, started, Some(decision.clone()), None, &msg)
                    }
                    FollowUpKind::Supplement => {
                        // MVP 做法：取消旧任务，用合并后的目标重新路由和派发
                        self.orchestrator.cancel(task_id)?;
                        let goal = format!("{}\n\n补充要求：{}", old.goal, change);
                        self.delegate(log, started, decision.clone(), &goal, &ctx).await
                    }
                }
            }
        }
    }

    /// 委派：Router 路由 + Policy 校验 + Orchestrator 派发
    async fn delegate(&self, mut log: RouteLog, started: Instant, decision: TurnDecision, goal: &str, ctx: &str) -> Result<TurnOutcome> {
        let conversation_id = log.conversation_id.clone();
        match self.router.route(goal, ctx).await {
            Ok(route) => {
                log.raw_outputs.extend(route.raw_outputs);
                log.attempts += route.attempts;
                log.route_decision = Some(route.decision.clone());
                log.policy = Some(route.policy.clone());
                let task = self.orchestrator.submit(&conversation_id, goal, ctx, route.decision.clone(), route.policy.clone())?;
                log.task_id = Some(task.id.clone());
                let reply = if route.policy.auto_execute {
                    format!("收到，已交给 {} 处理（{}）。完成后我会在这里给你结果。", task.role.as_str(), route.decision.reason)
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
                self.finish(log, started, Some(decision), None, &format!("这个任务暂时无法派发：{}", f.message))
            }
        }
    }

    /// 收尾：写入助手回复和路由日志
    fn finish(&self, mut log: RouteLog, started: Instant, decision: Option<TurnDecision>, task: Option<TaskRecord>, reply: &str) -> Result<TurnOutcome> {
        let msg = self.store.add_message(&log.conversation_id, "assistant", reply, task.as_ref().map(|t| t.id.as_str()))?;
        log.latency_ms = started.elapsed().as_millis() as u64;
        if let Err(e) = self.store.insert_route_log(&log) {
            tracing::error!(error = %e, "写入路由日志失败");
        }
        Ok(TurnOutcome { reply: msg, decision, task })
    }
}
