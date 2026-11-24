//! Orchestrator：创建任务、派发给 Agent、回收结果、组织审查与返工、处理取消和审批。
//!
//! 单个任务的执行流程：
//! ```text
//! submit ─┬─ Policy 不允许自动执行 → NEEDS_HUMAN（等待 approve）
//!         └─ QUEUED（等空闲 agent）→ 拿到 agent → RUNNING → Worker 返回
//!                        ├─ Worker 失败                 → FAILED
//!                        ├─ 不需要审查                   → COMPLETED
//!                        └─ VERIFYING → Reviewer 意见
//!                              ├─ pass                    → COMPLETED
//!                              ├─ needs_human             → NEEDS_HUMAN
//!                              └─ fail → 还有返工次数？ → 带着问题清单回到 QUEUED，再次拿到 agent 后 RUNNING
//!                                                   否   → FAILED
//! 最后由 Composer 生成交付消息，写入会话并推送给前端。
//! ```
//!
//! 状态守卫：每一步状态变化都声明"从哪些状态来"，由数据库用一条带条件的 UPDATE 完成
//! （[`Store::transition_task`]）。取消、完成、批准同时发生时只有一个能成功，进入终态后不会再变。
//!
//! 超时：从第一次拿到 agent 开始计时（排队等 agent 的时间不算），返工时的排队计入。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_agents::ReviewerAgent;
use an_core::{
    codes, now, AgentError, Assignment, EventSink, Evidence, PolicyVerdict, ReviewVerdict,
    RouteDecision, TaskRecord, TaskResult, TaskSpec, TaskStatus, Verdict, WorkerStatus,
};
use anyhow::{bail, Context, Result};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use crate::bus::Bus;
use crate::composer::Composer;
use crate::registry::{display_name, AgentRegistry};
use crate::run_progress::{run_phase_label, spawn_orchestrator_heartbeat};
use crate::store::{Store, ACTIVE_STATUSES};
use crate::template::{
    assemble_join, decide_split, validate_envelope, BranchEnvelope, JoinInput, RouteSlots,
    SplitDecision, Template, TemplateRegistry, STATUS_FAILED, STATUS_MISSING, STATUS_OK,
};
use serde::{Deserialize, Serialize};

/// 写入 join 任务 context 的汇合快照（由 [`JoinInput`] 生成）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JoinInputCtx {
    complete: bool,
    missing_branches: Vec<String>,
    summary: String,
}

fn join_input_for_ctx(j: &JoinInput) -> JoinInputCtx {
    let mut parts = Vec::new();
    for b in &j.branches {
        if b.status == STATUS_OK {
            let fields: Vec<_> = b.payload.iter().map(|(k, v)| format!("{k}={v}")).collect();
            parts.push(format!("[{}] {}", b.branch_id, fields.join(", ")));
        } else {
            let reason = if b.missing.is_empty() {
                b.status.clone()
            } else {
                format!("{}: {}", b.status, b.missing)
            };
            parts.push(format!("[{}] {reason}", b.branch_id));
        }
    }
    JoinInputCtx {
        complete: j.complete,
        missing_branches: j.missing_branches.clone(),
        summary: parts.join("\n"),
    }
}

/// 结束任务时允许的起点：执行中、审查中，或排队时就出错（例如没有可用 agent）
const FINISH_FROM: [TaskStatus; 3] = [
    TaskStatus::Queued,
    TaskStatus::Running,
    TaskStatus::Verifying,
];

/// 一次执行（含审查与返工）的结果
enum Outcome {
    /// 正常走完，按这个状态结束任务
    Finished(TaskStatus, Option<TaskResult>, Option<ReviewVerdict>),
    /// 状态已被别处改掉（通常是被取消），不再写结果、不再交付
    Stopped,
}

/// 单轮执行的结果
enum Step {
    Done(TaskResult),
    /// 超过任务期限
    TimedOut,
    Stopped,
}

/// 撤权结果
#[derive(Debug, Clone, PartialEq)]
pub struct RevokeReport {
    pub epoch: u64,
    pub aborted: Vec<String>,
}

pub struct Orchestrator {
    store: Arc<Store>,
    bus: Arc<Bus>,
    registry: Arc<AgentRegistry>,
    /// 验收用的 Reviewer（未启用时为 None）
    reviewer: Option<Arc<ReviewerAgent>>,
    composer: Composer,
    max_review_rounds: u32,
    task_timeout: Duration,
    /// WP4：已注册模板；空表则永不拆分
    templates: Arc<TemplateRegistry>,
    /// 正在执行的任务，用于取消
    running: Mutex<HashMap<String, AbortHandle>>,
    /// 任务 → 正在执行它的 agent（撤权时据此中止）。拿到 agent 后、开始执行前登记，执行返回后移除
    running_agents: Mutex<HashMap<String, String>>,
    /// 准入账本（开启 `[admission]` 时设置一次）
    admission: std::sync::OnceLock<Arc<crate::admission::AdmissionLedger>>,
    /// 状态写库和推送状态事件在这把锁里一起完成，保证前端收到的状态顺序与数据库一致
    /// （否则取消和执行两个线程交错时，前端可能先收到 CANCELLED 再收到 RUNNING）
    status_lock: Mutex<()>,
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<Store>,
        bus: Arc<Bus>,
        registry: Arc<AgentRegistry>,
        reviewer: Option<Arc<ReviewerAgent>>,
        composer: Composer,
        max_review_rounds: u32,
        task_timeout: Duration,
        templates: Arc<TemplateRegistry>,
    ) -> Self {
        Self {
            store,
            bus,
            registry,
            reviewer,
            composer,
            max_review_rounds,
            task_timeout,
            templates,
            running: Mutex::new(HashMap::new()),
            running_agents: Mutex::new(HashMap::new()),
            admission: std::sync::OnceLock::new(),
            status_lock: Mutex::new(()),
        }
    }

    /// 开启准入（只能设置一次）
    pub fn set_admission(&self, ledger: Arc<crate::admission::AdmissionLedger>) {
        let _ = self.admission.set(ledger);
    }

    /// 撤销某个 agent：作废凭证（epoch 加 1），唤醒排队任务改由其他成员接手，
    /// 并中止它正在执行的任务（状态改为 FAILED、错误码 E_AGENT_REVOKED，不交付）。
    /// 撤权前已被接受的结果不回滚。配置文件里写的 agent 先补一条预准入记录再撤销。
    pub fn revoke_agent(&self, agent_id: &str, reason: &str) -> Result<RevokeReport> {
        let ledger = self
            .admission
            .get()
            .context("准入未开启（[admission] enabled = false）")?;
        if ledger.get(agent_id).is_none() {
            let known = self.registry.cards().iter().any(|c| c.id == agent_id);
            anyhow::ensure!(known, "未知 agent：{agent_id}");
            ledger.preadmit(agent_id)?;
        }
        let started = std::time::Instant::now();
        let epoch = ledger.revoke(agent_id, reason)?;
        self.registry.wake();
        let assigned: Vec<String> = self
            .running_agents
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| a.as_str() == agent_id)
            .map(|(t, _)| t.clone())
            .collect();
        let mut aborted = vec![];
        for id in assigned {
            let Some(task) = self.store.get_task(&id)? else {
                continue;
            };
            let result = TaskResult::failed_code(
                codes::AGENT_REVOKED,
                format!("执行该任务的 agent 已被撤权：{reason}"),
            );
            // 与完成走同一把状态锁和条件更新：谁先改成终态谁算数，不会互相覆盖
            if self.finish(
                &task,
                &[TaskStatus::Queued, TaskStatus::Running],
                TaskStatus::Failed,
                Some(&result),
                "执行的 agent 已被撤权",
            ) {
                if let Some(h) = self.running.lock().unwrap().remove(&id) {
                    h.abort();
                }
                self.running_agents.lock().unwrap().remove(&id);
                aborted.push(id);
            }
        }
        tracing::warn!(
            event = "admission.tasks_aborted",
            code = codes::AGENT_REVOKED,
            agent_id,
            epoch,
            aborted = aborted.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "agent 已撤权，中止其进行中的任务"
        );
        Ok(RevokeReport { epoch, aborted })
    }

    /// 外部 agent 的结果被接受后，记入试用期（按配置比例抽检）
    fn note_probation(&self, agent_id: &str, task_id: &str) {
        let Some(l) = self.admission.get() else {
            return;
        };
        let sampled = l.should_sample(agent_id, task_id);
        if let Err(e) = l.note_completed(agent_id, task_id, sampled) {
            tracing::warn!(event = "admission.check", agent_id, task_id, error = %format!("{e:#}"), "记录试用期失败");
        }
    }

    /// 带条件的状态转换：当前状态属于 `from` 才改成 `to`，并推送状态事件。
    /// 返回 false 表示状态已被别处改掉（例如已取消），调用方应停止后续动作。
    fn transition(
        &self,
        task: &TaskRecord,
        from: &[TaskStatus],
        to: TaskStatus,
        msg: impl Into<String>,
    ) -> bool {
        self.finish(task, from, to, None, msg)
    }

    /// 同 [`Self::transition`]，`result` 有值时与状态一起写入
    fn finish(
        &self,
        task: &TaskRecord,
        from: &[TaskStatus],
        to: TaskStatus,
        result: Option<&TaskResult>,
        msg: impl Into<String>,
    ) -> bool {
        let guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
        let changed = self.store.finish_task(&task.id, from, to, result);
        if let Ok(true) = changed {
            self.bus.status(&task.conversation_id, &task.id, to, msg);
        }
        drop(guard);
        match changed {
            Ok(true) => true,
            Ok(false) => {
                self.log_conflict(task, from, to);
                false
            }
            Err(e) => {
                tracing::error!(error = %format!("{e:#}"), task_id = %task.id, "更新任务状态失败");
                false
            }
        }
    }

    fn log_conflict(&self, task: &TaskRecord, from: &[TaskStatus], to: TaskStatus) {
        let actual = self
            .store
            .get_task(&task.id)
            .ok()
            .flatten()
            .map(|t| t.status.as_str())
            .unwrap_or("不存在");
        let expected = from
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("|");
        tracing::warn!(event = "task.status_conflict", task_id = %task.id, expected = %expected, actual, to = to.as_str(), "任务状态已被改变，放弃这次状态变化");
    }

    /// 排队提示：前面还有几个同角色的任务在等
    fn queue_message(&self, task_id: &str) -> String {
        match self.store.queued_ahead(task_id).unwrap_or(0) {
            0 => "排队中，等待空闲 agent".to_string(),
            n => format!("排队中，前面还有 {n} 个同类任务"),
        }
    }

    /// 创建任务并（在 Policy 允许时）开始执行；命中模板则走 WP4 拆分。
    pub fn submit(
        self: &Arc<Self>,
        conversation_id: &str,
        goal: &str,
        context: &str,
        route: RouteDecision,
        policy: PolicyVerdict,
    ) -> Result<TaskRecord> {
        let slots = RouteSlots {
            template_id: route.template_id.clone(),
            deliverable: route.deliverable.clone(),
            slots_ok: route.slots_ok.clone(),
        };
        match decide_split(&slots, &self.templates) {
            SplitDecision::Split { template_id } => {
                self.submit_split(conversation_id, goal, context, route, policy, &template_id)
            }
            SplitDecision::Single { reason } => {
                if !route.template_id.is_empty() {
                    tracing::info!(
                        event = "task.template_split",
                        decision = "single",
                        reason = %reason,
                        "模板未拆分，整单执行"
                    );
                }
                self.submit_single(conversation_id, goal, context, route, policy)
            }
        }
    }

    fn submit_single(
        self: &Arc<Self>,
        conversation_id: &str,
        goal: &str,
        context: &str,
        route: RouteDecision,
        policy: PolicyVerdict,
    ) -> Result<TaskRecord> {
        let ts = now();
        let task = TaskRecord {
            id: an_core::new_id("task"),
            conversation_id: conversation_id.into(),
            goal: goal.into(),
            role: route.execution_role,
            status: TaskStatus::Created,
            route,
            policy,
            result: None,
            review: None,
            assignment: None,
            parent_id: None,
            fanout_kind: None,
            branch_id: None,
            template_id: None,
            created_at: ts.clone(),
            updated_at: ts,
        };
        self.store.insert_task(&task, context)?;
        self.bus.status(
            &task.conversation_id,
            &task.id,
            TaskStatus::Created,
            format!("任务已创建，执行角色：{}", task.role.as_str()),
        );

        if task.policy.auto_execute {
            if self.transition(
                &task,
                &[TaskStatus::Created],
                TaskStatus::Queued,
                self.queue_message(&task.id),
            ) {
                self.spawn(task.clone());
            }
        } else {
            self.transition(
                &task,
                &[TaskStatus::Created],
                TaskStatus::NeedsHuman,
                format!("等待人工审批：{}", task.policy.notes.join("；")),
            );
        }
        Ok(TaskRecord {
            status: self
                .store
                .get_task(&task.id)?
                .map(|t| t.status)
                .unwrap_or(task.status),
            ..task
        })
    }

    fn submit_split(
        self: &Arc<Self>,
        conversation_id: &str,
        goal: &str,
        context: &str,
        route: RouteDecision,
        policy: PolicyVerdict,
        template_id: &str,
    ) -> Result<TaskRecord> {
        let tmpl = self
            .templates
            .get(template_id)
            .context("拆分时模板应已注册")?
            .clone();
        let ts = now();
        let parent = TaskRecord {
            id: an_core::new_id("task"),
            conversation_id: conversation_id.into(),
            goal: goal.into(),
            role: route.execution_role,
            status: TaskStatus::Created,
            route,
            policy,
            result: None,
            review: None,
            assignment: None,
            parent_id: None,
            fanout_kind: Some("root".into()),
            branch_id: None,
            template_id: Some(template_id.into()),
            created_at: ts.clone(),
            updated_at: ts,
        };
        self.store.insert_task(&parent, context)?;
        self.bus.status(
            &parent.conversation_id,
            &parent.id,
            TaskStatus::Created,
            format!(
                "模板任务已创建：{template_id}（{} 路）",
                tmpl.branches.len()
            ),
        );
        tracing::info!(
            event = "task.template_split",
            task_id = %parent.id,
            parent_task_id = %parent.id,
            template_id = %template_id,
            branches = tmpl.branches.len(),
            "决定按模板拆分"
        );

        if parent.policy.auto_execute {
            self.transition(
                &parent,
                &[TaskStatus::Created],
                TaskStatus::Running,
                format!("模板 {template_id} 并行执行中"),
            );
            self.start_fanout(&parent, &tmpl, context)?;
        } else {
            self.transition(
                &parent,
                &[TaskStatus::Created],
                TaskStatus::NeedsHuman,
                format!("等待人工审批后再拆分：{}", parent.policy.notes.join("；")),
            );
        }
        Ok(TaskRecord {
            status: self
                .store
                .get_task(&parent.id)?
                .map(|t| t.status)
                .unwrap_or(parent.status),
            ..parent
        })
    }

    fn start_fanout(
        self: &Arc<Self>,
        parent: &TaskRecord,
        tmpl: &Template,
        context: &str,
    ) -> Result<()> {
        let template_id = tmpl.id.as_str();
        for b in &tmpl.branches {
            let ts = now();
            let steps = b.serial_steps.join(" → ");
            let branch_ctx = format!(
                "{context}\n\n## 模板分支 `{branch}`（能力 {cap}）\n请按步骤完成：{steps}\n\
                 完成后**只**输出一个 JSON 信封（不要 Markdown 围栏），字段：\
                 branch_id, status, missing, payload（字符串值）。\n\
                 branch_id 必须是 `{branch}`；成功时 status=ok 且 payload 含：{outs}",
                branch = b.id,
                cap = b.capability.as_str(),
                outs = b.output_required.join(", "),
            );
            let mut route = parent.route.clone();
            route.execution_role = b.capability;
            route.required_tools.clear();
            let mut policy = parent.policy.clone();
            policy.needs_review = false;
            policy.auto_execute = true;
            policy.candidates = self.registry.candidates(b.capability);
            let child = TaskRecord {
                id: an_core::new_id("task"),
                conversation_id: parent.conversation_id.clone(),
                goal: format!("{} · 分支 {}", parent.goal, b.id),
                role: b.capability,
                status: TaskStatus::Created,
                route,
                policy,
                result: None,
                review: None,
                assignment: None,
                parent_id: Some(parent.id.clone()),
                fanout_kind: Some("branch".into()),
                branch_id: Some(b.id.clone()),
                template_id: Some(template_id.into()),
                created_at: ts.clone(),
                updated_at: ts,
            };
            self.store.insert_task(&child, &branch_ctx)?;
            self.bus.status(
                &child.conversation_id,
                &child.id,
                TaskStatus::Created,
                format!("分支 {} 已创建", b.id),
            );
            if self.transition(
                &child,
                &[TaskStatus::Created],
                TaskStatus::Queued,
                self.queue_message(&child.id),
            ) {
                self.spawn(child);
            }
        }
        Ok(())
    }

    /// 人工审批通过后开始执行。父模板任务则拉起分支；普通任务 spawn 执行。
    pub fn approve(self: &Arc<Self>, task_id: &str) -> Result<()> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        let is_root = task.fanout_kind.as_deref() == Some("root");
        let approved = {
            let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
            let approved = self.store.approve_task(task_id)?;
            if approved {
                let next = if is_root {
                    TaskStatus::Running
                } else {
                    TaskStatus::Queued
                };
                let msg = if is_root {
                    format!(
                        "已获批准，开始模板拆分：{}",
                        task.template_id.as_deref().unwrap_or("?")
                    )
                } else {
                    self.queue_message(task_id)
                };
                // approve_task 已写成 QUEUED；root 再升到 RUNNING
                if is_root {
                    let _ = self.store.set_task_status(task_id, TaskStatus::Running);
                }
                self.bus.status(&task.conversation_id, &task.id, next, msg);
            }
            approved
        };
        if !approved {
            let current = self
                .store
                .get_task(task_id)?
                .map(|t| t.status)
                .unwrap_or(task.status);
            bail!(
                "只有等待审批（尚未执行）的任务可以批准，当前状态 {}",
                current.as_str()
            );
        }
        EventSink::new(self.bus.clone(), &task.conversation_id, &task.id)
            .progress("已获人工批准，开始执行");
        if is_root {
            let tid = task
                .template_id
                .as_deref()
                .context("root 缺少 template_id")?;
            let tmpl = self.templates.get(tid).context("模板未注册")?.clone();
            let ctx = self.store.get_task_context(task_id)?.unwrap_or_default();
            self.start_fanout(&task, &tmpl, &ctx)?;
        } else {
            self.spawn(TaskRecord {
                status: TaskStatus::Queued,
                ..task
            });
        }
        Ok(())
    }

    /// 审查后的人工验收：`accept` → COMPLETED 并交付；`reject` → 有返工额度则入队，否则 FAILED。
    /// 仅 `NEEDS_HUMAN` 且已有 `result`（待验收）可调用；否则返回错误（由 API 映为 409）。
    pub async fn human_review_verdict(
        self: &Arc<Self>,
        task_id: &str,
        accept: bool,
        note: &str,
    ) -> Result<()> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        if task.status != TaskStatus::NeedsHuman || task.result.is_none() {
            bail!(
                "只有审查后待验收的任务可以判定，当前状态 {}（result={}）",
                task.status.as_str(),
                if task.result.is_some() {
                    "present"
                } else {
                    "null"
                }
            );
        }
        let sink = EventSink::new(self.bus.clone(), &task.conversation_id, &task.id);
        let note = note.trim();

        if accept {
            let changed = {
                let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
                let changed = self.store.accept_human_review(task_id)?;
                if changed {
                    self.bus.status(
                        &task.conversation_id,
                        &task.id,
                        TaskStatus::Completed,
                        "人工验收通过",
                    );
                }
                changed
            };
            if !changed {
                bail!("任务已不再处于待验收，无法判定通过");
            }
            tracing::info!(
                event = "task.human_review_verdict",
                task_id = %task_id,
                verdict = "accept",
                note = %note,
                "人工验收通过"
            );
            sink.progress(if note.is_empty() {
                "人工验收通过".into()
            } else {
                format!("人工验收通过：{note}")
            });
            // WP4：分支不向用户交付，改走汇合；顶层任务照常交付
            if let Some(pid) = &task.parent_id {
                let _ = self.try_join(pid).await;
            } else {
                self.deliver(
                    &task,
                    TaskStatus::Completed,
                    task.result.as_ref(),
                    task.review.as_ref(),
                )
                .await;
            }
            return Ok(());
        }

        // reject：人工退回次数记在 context 标题里（跨 spawn 不丢；无 DDL）
        let old_ctx = self.store.get_task_context(task_id)?.unwrap_or_default();
        let human_rejects = old_ctx.matches("## 人工验收未通过").count() as u32;
        if human_rejects < self.max_review_rounds {
            let issues = task
                .review
                .as_ref()
                .map(|r| {
                    r.issues
                        .iter()
                        .map(|i| format!("- {i}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let summary = task
                .result
                .as_ref()
                .map(|r| an_core::truncate_chars(&r.summary, 2000))
                .unwrap_or_default();
            let note_line = if note.is_empty() {
                String::new()
            } else {
                format!("- {note}\n")
            };
            let new_ctx = format!(
                "{old_ctx}\n\n## 人工验收未通过，请修正以下问题\n{note_line}{issues}\n\n## 上一轮结果摘要\n{summary}"
            );
            let next_round = human_rejects + 1;
            let changed = {
                let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
                let changed = self.store.reject_human_review_rework(task_id, &new_ctx)?;
                if changed {
                    self.bus.status(
                        &task.conversation_id,
                        &task.id,
                        TaskStatus::Queued,
                        format!(
                            "人工退回第 {next_round} 轮返工，{}",
                            self.queue_message(task_id)
                        ),
                    );
                }
                changed
            };
            if !changed {
                bail!("任务已不再处于待验收，无法退回返工");
            }
            tracing::info!(
                event = "task.human_review_verdict",
                task_id = %task_id,
                verdict = "reject",
                rework = true,
                note = %note,
                "人工验收退回返工"
            );
            sink.progress(if note.is_empty() {
                "人工验收未通过，开始返工".into()
            } else {
                format!("人工验收未通过：{note}")
            });
            self.spawn(TaskRecord {
                status: TaskStatus::Queued,
                ..task
            });
            return Ok(());
        }

        let changed = {
            let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
            let changed = self.store.reject_human_review_fail(task_id)?;
            if changed {
                self.bus.status(
                    &task.conversation_id,
                    &task.id,
                    TaskStatus::Failed,
                    "人工验收未通过且返工次数已用完",
                );
            }
            changed
        };
        if !changed {
            bail!("任务已不再处于待验收，无法判定失败");
        }
        tracing::info!(
            event = "task.human_review_verdict",
            task_id = %task_id,
            verdict = "reject",
            rework = false,
            note = %note,
            "人工验收未通过（无返工额度）"
        );
        sink.progress(if note.is_empty() {
            "人工验收未通过，任务失败".into()
        } else {
            format!("人工验收未通过：{note}")
        });
        if let Some(pid) = &task.parent_id {
            // 分支失败终态后尝试汇合（该路记 missing/failed）
            let _ = self.try_join(pid).await;
        } else {
            self.deliver(
                &task,
                TaskStatus::Failed,
                task.result.as_ref(),
                task.review.as_ref(),
            )
            .await;
        }
        Ok(())
    }

    /// 生成交付消息并写入会话（人工验收与自动结束共用）
    /// WP7 方案 C：重启后把未起跑的单任务（CREATED/QUEUED，非模板家族）重新排队执行。
    /// 有副作用风险的状态（RUNNING/VERIFYING）由 `store::recover_interrupted_tasks` 置 FAILED；
    /// 模板家族（root/branch/join）不重驱（家族一致性无法安全重建），沿 T-81 失败恢复路径。
    pub fn resume_queued_on_restart(self: &Arc<Self>) -> Result<Vec<String>> {
        let tasks = self.store.list_resumable_tasks()?;
        let mut resumed = Vec::new();
        for mut task in tasks {
            if task.status == TaskStatus::Created {
                let _ = self.store.set_task_status(&task.id, TaskStatus::Queued);
                task.status = TaskStatus::Queued;
            }
            EventSink::new(self.bus.clone(), &task.conversation_id, &task.id)
                .progress("服务重启，任务重新排队执行");
            tracing::info!(
                event = "task.resume_queued",
                task_id = %task.id,
                "重启后重新排队执行未起跑任务"
            );
            resumed.push(task.id.clone());
            self.spawn(task);
        }
        Ok(resumed)
    }

    async fn deliver(
        &self,
        task: &TaskRecord,
        status: TaskStatus,
        result: Option<&TaskResult>,
        review: Option<&ReviewVerdict>,
    ) {
        let reply = self.composer.compose(task, status, result, review).await;
        match self
            .store
            .add_message(&task.conversation_id, "assistant", &reply, Some(&task.id))
        {
            Ok(_) => self
                .bus
                .assistant_message(&task.conversation_id, Some(&task.id), &reply),
            Err(e) => tracing::error!(error = %e, "写入交付消息失败"),
        }
    }

    /// 取消任务。返回 true 表示确实取消了一个未结束的任务。
    /// 父模板任务取消时，一并取消 ACTIVE 子任务（WP4）。
    pub fn cancel(self: &Arc<Self>, task_id: &str) -> Result<bool> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        let child_ids: Vec<String> = if task.fanout_kind.as_deref() == Some("root") {
            self.store
                .list_children(task_id)?
                .into_iter()
                .filter(|c| !c.status.is_terminal())
                .map(|c| c.id)
                .collect()
        } else {
            vec![]
        };
        {
            let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
            if !self
                .store
                .transition_task(task_id, &ACTIVE_STATUSES, TaskStatus::Cancelled)?
            {
                return Ok(false);
            }
            self.bus.status(
                &task.conversation_id,
                &task.id,
                TaskStatus::Cancelled,
                "任务已取消",
            );
            for cid in &child_ids {
                if self
                    .store
                    .transition_task(cid, &ACTIVE_STATUSES, TaskStatus::Cancelled)?
                {
                    self.bus.status(
                        &task.conversation_id,
                        cid,
                        TaskStatus::Cancelled,
                        "父任务取消，分支一并取消",
                    );
                }
            }
        }
        if let Some(h) = self.running.lock().unwrap().remove(task_id) {
            h.abort();
        }
        for cid in &child_ids {
            if let Some(h) = self.running.lock().unwrap().remove(cid) {
                h.abort();
            }
        }
        Ok(true)
    }

    /// 启动后台执行（持有锁时 spawn，保证 abort handle 一定先登记，再被执行完的任务移除）。
    /// 调用前任务必须已经是 QUEUED，且只有把它改成 QUEUED 的那一方会调用这里。
    fn spawn(self: &Arc<Self>, task: TaskRecord) {
        let mut running = self.running.lock().unwrap();
        let this = self.clone();
        let id = task.id.clone();
        let handle = tokio::spawn(async move {
            let id = task.id.clone();
            this.execute(task).await;
            this.running.lock().unwrap().remove(&id);
        });
        running.insert(id, handle.abort_handle());
    }

    async fn execute(self: &Arc<Self>, task: TaskRecord) {
        // WP4 join：不跑 agent，直接物化汇合结果
        if task.fanout_kind.as_deref() == Some("join") {
            self.execute_join(task).await;
            return;
        }

        let (status, result, review) = match self.run_with_review(&task).await {
            Ok(Outcome::Finished(s, r, v)) => (s, r, v),
            Ok(Outcome::Stopped) => return,
            Err(e) => (
                TaskStatus::Failed,
                Some(TaskResult::failed(format!("{e:#}"))),
                None,
            ),
        };
        if !self.finish(
            &task,
            &FINISH_FROM,
            status,
            result.as_ref(),
            format!("任务结束：{}", status.as_str()),
        ) {
            return;
        }

        let is_child = task.parent_id.is_some();
        if is_child {
            if let Some(bid) = &task.branch_id {
                tracing::info!(
                    event = "task.template_branch_done",
                    task_id = %task.id,
                    branch_id = %bid,
                    status = status.as_str(),
                    "模板分支结束"
                );
            }
            // 子任务不向用户交付；尝试汇合
            if let Some(pid) = &task.parent_id {
                let _ = self.try_join(pid).await;
            }
            return;
        }

        let reply = self
            .composer
            .compose(&task, status, result.as_ref(), review.as_ref())
            .await;
        match self
            .store
            .add_message(&task.conversation_id, "assistant", &reply, Some(&task.id))
        {
            Ok(_) => self
                .bus
                .assistant_message(&task.conversation_id, Some(&task.id), &reply),
            Err(e) => tracing::error!(error = %e, "写入交付消息失败"),
        }
    }

    /// 从分支结果解析信封；失败则记 missing
    fn branch_envelope(tmpl: &Template, child: &TaskRecord) -> BranchEnvelope {
        let bid = child.branch_id.clone().unwrap_or_default();
        let spec = tmpl.branches.iter().find(|b| b.id == bid);
        let Some(spec) = spec else {
            return BranchEnvelope {
                branch_id: bid,
                status: STATUS_MISSING.into(),
                missing: "未知分支".into(),
                payload: Default::default(),
            };
        };
        if child.status == TaskStatus::Cancelled {
            return BranchEnvelope {
                branch_id: bid,
                status: STATUS_MISSING.into(),
                missing: "整路未返回".into(),
                payload: Default::default(),
            };
        }
        if child.status == TaskStatus::Failed {
            return BranchEnvelope {
                branch_id: bid,
                status: STATUS_FAILED.into(),
                missing: String::new(),
                payload: Default::default(),
            };
        }
        let summary = child
            .result
            .as_ref()
            .map(|r| r.summary.trim())
            .unwrap_or("");
        // 允许答案前后有说明：取第一个 { ... } JSON 对象
        let json_slice = summary
            .find('{')
            .and_then(|i| summary[i..].rfind('}').map(|j| &summary[i..=i + j]));
        let parsed = json_slice.and_then(|s| serde_json::from_str::<BranchEnvelope>(s).ok());
        match parsed {
            Some(env) => match validate_envelope(spec, &env) {
                Ok(()) => env,
                Err(reason) => BranchEnvelope {
                    branch_id: bid,
                    status: STATUS_MISSING.into(),
                    missing: reason,
                    payload: Default::default(),
                },
            },
            None => BranchEnvelope {
                branch_id: bid,
                status: STATUS_MISSING.into(),
                missing: "无法解析分支信封 JSON".into(),
                payload: Default::default(),
            },
        }
    }

    /// 全部 branch 终态且无 NEEDS_HUMAN 时创建 join 并 spawn
    async fn try_join(self: &Arc<Self>, parent_id: &str) -> Result<()> {
        let parent = self.store.get_task(parent_id)?.context("父任务不存在")?;
        if parent.status.is_terminal() {
            return Ok(());
        }
        if parent.fanout_kind.as_deref() != Some("root") {
            return Ok(());
        }
        let children = self.store.list_children(parent_id)?;
        let branches: Vec<_> = children
            .iter()
            .filter(|c| c.fanout_kind.as_deref() == Some("branch"))
            .cloned()
            .collect();
        if branches.is_empty() {
            return Ok(());
        }
        if branches.iter().any(|c| c.status == TaskStatus::NeedsHuman) {
            return Ok(()); // 等人验收（WP2）
        }
        if branches.iter().any(|c| !c.status.is_terminal()) {
            return Ok(());
        }
        if children
            .iter()
            .any(|c| c.fanout_kind.as_deref() == Some("join"))
        {
            return Ok(()); // 已建 join
        }
        let tid = parent
            .template_id
            .as_deref()
            .context("root 无 template_id")?;
        let tmpl = self.templates.get(tid).context("模板未注册")?.clone();
        let envs: Vec<_> = branches
            .iter()
            .map(|c| Self::branch_envelope(&tmpl, c))
            .collect();
        let join_input = match assemble_join(&tmpl, &envs) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(
                    event = "task.template_join",
                    task_id = %parent_id,
                    error = %e,
                    "汇合失败"
                );
                let _ = self.finish(
                    &parent,
                    &[
                        TaskStatus::Running,
                        TaskStatus::Queued,
                        TaskStatus::NeedsHuman,
                    ],
                    TaskStatus::Failed,
                    Some(&TaskResult::failed(format!("模板汇合失败：{e}"))),
                    format!("模板汇合失败：{e}"),
                );
                return Ok(());
            }
        };
        tracing::info!(
            event = "task.template_join",
            task_id = %parent_id,
            complete = join_input.complete,
            missing = ?join_input.missing_branches,
            "开始汇合"
        );
        let ts = now();
        let join_ctx = serde_json::to_string(&join_input_for_ctx(&join_input))?;
        let mut route = parent.route.clone();
        route.execution_role = tmpl.join.capability;
        let mut policy = parent.policy.clone();
        policy.needs_review = false;
        policy.auto_execute = true;
        let join_task = TaskRecord {
            id: an_core::new_id("task"),
            conversation_id: parent.conversation_id.clone(),
            goal: format!("{} · 汇合", parent.goal),
            role: tmpl.join.capability,
            status: TaskStatus::Created,
            route,
            policy,
            result: None,
            review: None,
            assignment: None,
            parent_id: Some(parent.id.clone()),
            fanout_kind: Some("join".into()),
            branch_id: Some("__join__".into()),
            template_id: Some(tid.into()),
            created_at: ts.clone(),
            updated_at: ts,
        };
        self.store.insert_task(&join_task, &join_ctx)?;
        if self.transition(
            &join_task,
            &[TaskStatus::Created],
            TaskStatus::Queued,
            "汇合任务排队",
        ) {
            self.spawn(join_task);
        }
        Ok(())
    }

    async fn execute_join(self: &Arc<Self>, task: TaskRecord) {
        let ctx = self
            .store
            .get_task_context(&task.id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let join: JoinInputCtx = serde_json::from_str(&ctx).unwrap_or(JoinInputCtx {
            complete: false,
            missing_branches: vec!["汇合上下文损坏".into()],
            summary: "汇合失败".into(),
        });
        let mut summary = join.summary.clone();
        if !join.complete {
            summary.push_str("\n\n（有缺失分支：");
            summary.push_str(&join.missing_branches.join(", "));
            summary.push(')');
        }
        let result = TaskResult {
            status: WorkerStatus::Succeeded,
            summary,
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        };
        if !self.transition(
            &task,
            &[TaskStatus::Queued, TaskStatus::Running],
            TaskStatus::Running,
            "汇合中",
        ) {
            // 可能已是 Queued；继续 finish
        }
        if !self.finish(
            &task,
            &[
                TaskStatus::Queued,
                TaskStatus::Running,
                TaskStatus::Verifying,
            ],
            TaskStatus::Completed,
            Some(&result),
            "汇合完成",
        ) {
            return;
        }
        tracing::info!(
            event = "task.template_join",
            task_id = %task.id,
            complete = join.complete,
            "汇合结束"
        );
        if let Some(pid) = &task.parent_id {
            let _ = self.complete_parent(pid, &result, join.complete).await;
        }
    }

    async fn complete_parent(
        self: &Arc<Self>,
        parent_id: &str,
        join_result: &TaskResult,
        complete: bool,
    ) -> Result<()> {
        let parent = self.store.get_task(parent_id)?.context("父任务不存在")?;
        if parent.status.is_terminal() {
            return Ok(());
        }
        // 先写入交付消息，再标 COMPLETED：避免观测到终态时会话尚无助理交付
        // （Composer 对不可达 LLM 回退有连接耗时，finish→deliver 会竞态）
        self.deliver(&parent, TaskStatus::Completed, Some(join_result), None)
            .await;
        let msg = if complete {
            "模板任务完成（各路齐全）"
        } else {
            "模板任务完成（有缺失分支）"
        };
        let _ = self.finish(
            &parent,
            &[
                TaskStatus::Running,
                TaskStatus::Queued,
                TaskStatus::NeedsHuman,
            ],
            TaskStatus::Completed,
            Some(join_result),
            msg,
        );
        Ok(())
    }

    /// 从执行组中分配一个 agent 执行一轮。
    /// - 组内 agent 都在忙时排队等待，任务保持 QUEUED；拿到 agent 后才改成 RUNNING；
    /// - `deadline` 在第一次拿到 agent 时设定，之后各轮共用；
    /// - 执行出错如实报告（带错误码），不会换给别的 agent 重做。
    async fn run_on_group(
        &self,
        task: &TaskRecord,
        spec: TaskSpec,
        sink: &EventSink,
        round: u32,
        deadline: &mut Option<Instant>,
    ) -> Step {
        let lease = match self
            .registry
            .acquire_for(task.role, task.route.risk_level, |msg| sink.progress(msg))
            .await
        {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(event = "task.no_agent", code = e.code, task_id = %task.id, role = task.role.as_str(), error = %e.message);
                return Step::Done(TaskResult::failed_code(e.code, e.message));
            }
        };
        let card = lease.agent.card().clone();
        // 先记下执行者，再改成 RUNNING：前端收到 RUNNING 事件刷新任务时就能看到是谁在做。
        // 写库失败只记日志，不影响执行（和写交付消息失败的处理一致）。
        let assignment = Assignment {
            agent_id: card.id.clone(),
            agent_name: card.name.clone(),
            kind: card.kind.clone(),
            location: card.location.clone(),
            member_index: lease.member_index as u32,
            group_size: lease.group_size as u32,
            waited_ms: lease.waited.as_millis() as u64,
            round,
        };
        if let Err(e) = self.store.set_task_assignment(&task.id, &assignment) {
            tracing::error!(task_id = %task.id, error = %e, "写入执行者失败");
        }
        // 先登记"谁在执行"，再改成 RUNNING：看到 RUNNING 时撤权一定找得到它
        let lease_epoch = lease.epoch;
        self.running_agents
            .lock()
            .unwrap()
            .insert(task.id.clone(), lease.agent.card().id.clone());
        let msg = if round == 0 {
            "开始执行".to_string()
        } else {
            format!("第 {round} 轮返工")
        };
        if !self.transition(task, &[TaskStatus::Queued], TaskStatus::Running, msg) {
            self.running_agents.lock().unwrap().remove(&task.id);
            return Step::Stopped;
        }
        let deadline = *deadline.get_or_insert_with(|| Instant::now() + self.task_timeout);

        // 节点 agent 的名称里已经带了机器名，不再重复
        let shown = display_name(&card);
        sink.progress(format!("由 {shown} 执行"));
        tracing::info!(event = "agent.run_start", task_id = %task.id, agent_id = %card.id, kind = %card.kind, location = %card.location, member_index = lease.member_index, group_size = lease.group_size, waited_ms = assignment.waited_ms);
        let started = std::time::Instant::now();
        let phase = run_phase_label(round);
        let heartbeat = spawn_orchestrator_heartbeat(
            sink.clone(),
            shown.clone(),
            phase,
            started,
            self.task_timeout,
        );
        let outcome = tokio::time::timeout_at(deadline, lease.agent.run(spec, sink.clone())).await;
        heartbeat.abort();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        // 名额在审查之前归还，让排队的任务尽早开始
        drop(lease);
        self.running_agents.lock().unwrap().remove(&task.id);
        // 凭证核对：执行期间被降级或撤权（epoch 变化）时，丢弃交回的结果，不交付
        if !self.registry.lease_valid(&card, lease_epoch) {
            tracing::warn!(event = "admission.result_rejected", code = codes::AGENT_REVOKED, task_id = %task.id, agent_id = %card.id, epoch = ?lease_epoch, "凭证已作废，丢弃该 agent 交回的结果");
            let result = TaskResult::failed_code(
                codes::AGENT_REVOKED,
                format!("{} 的凭证已作废（降级或撤权），结果不被接受", card.name),
            );
            self.finish(
                task,
                &[TaskStatus::Running],
                TaskStatus::Failed,
                Some(&result),
                "执行的 agent 凭证已作废",
            );
            return Step::Stopped;
        }
        let mut result = match outcome {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => match e.downcast_ref::<AgentError>() {
                Some(ae) => TaskResult::failed_code(ae.code, &ae.message),
                None => {
                    TaskResult::failed_code(codes::AGENT_RUNTIME, format!("{}：{e:#}", card.name))
                }
            },
            Err(_) => {
                tracing::error!(event = "agent.run_failed", task_id = %task.id, agent_id = %card.id, elapsed_ms, code = codes::TASK_TIMEOUT);
                return Step::TimedOut;
            }
        };
        if result.status == WorkerStatus::Failed {
            tracing::error!(event = "agent.run_failed", task_id = %task.id, agent_id = %card.id, elapsed_ms, errors = %result.errors.join(" | "));
        } else {
            tracing::info!(event = "agent.run_end", task_id = %task.id, agent_id = %card.id, elapsed_ms);
            // 一次真实任务成功完成：该 agent 在一段时间内保持绿色（见 link.rs 的 ACTIVE_WINDOW）
            self.registry.links.mark_success(&card.id);
            if card.kind == crate::admission::A2A_KIND {
                self.note_probation(&card.id, &task.id);
            }
        }
        result.evidence.insert(
            0,
            Evidence {
                kind: "executed_by".into(),
                content: shown,
            },
        );
        Step::Done(result)
    }

    fn timeout_result(&self) -> TaskResult {
        TaskResult::failed_code(
            codes::TASK_TIMEOUT,
            format!("任务超时（{} 秒）", self.task_timeout.as_secs()),
        )
    }

    /// 执行 Worker，并按 Policy 组织审查与返工
    async fn run_with_review(&self, task: &TaskRecord) -> Result<Outcome> {
        // 执行上下文存在 tasks 表里：服务重启后（例如等待审批的任务被批准时）依然可用
        let mut context = self.store.get_task_context(&task.id)?.unwrap_or_default();
        let sink = EventSink::new(self.bus.clone(), &task.conversation_id, &task.id);
        let mut deadline = None;

        let mut round = 0;
        loop {
            let spec = TaskSpec {
                task_id: task.id.clone(),
                conversation_id: task.conversation_id.clone(),
                goal: task.goal.clone(),
                context: context.clone(),
                route: task.route.clone(),
            };
            let result = match self
                .run_on_group(task, spec, &sink, round, &mut deadline)
                .await
            {
                Step::Done(r) => r,
                Step::TimedOut => {
                    return Ok(Outcome::Finished(
                        TaskStatus::Failed,
                        Some(self.timeout_result()),
                        None,
                    ))
                }
                Step::Stopped => return Ok(Outcome::Stopped),
            };

            // Worker 自己失败：不进入审查
            if result.status == WorkerStatus::Failed {
                return Ok(Outcome::Finished(TaskStatus::Failed, Some(result), None));
            }
            // 不需要审查：Worker 成功即视为完成
            let (true, Some(reviewer)) = (task.policy.needs_review, &self.reviewer) else {
                return Ok(Outcome::Finished(TaskStatus::Completed, Some(result), None));
            };

            // 中间结果先写库：审查期间也能在任务详情里看到执行结果
            self.store.set_task_result(&task.id, &result)?;
            if !self.transition(
                task,
                &[TaskStatus::Running],
                TaskStatus::Verifying,
                "Reviewer 审查中",
            ) {
                return Ok(Outcome::Stopped);
            }
            // deadline 一定已在 run_on_group 里设定
            let review = match deadline {
                Some(d) => tokio::time::timeout_at(d, reviewer.review(&task.goal, &result)).await,
                None => Ok(reviewer.review(&task.goal, &result).await),
            };
            let verdict = match review {
                Ok(Ok(v)) => v,
                Err(_) => {
                    return Ok(Outcome::Finished(
                        TaskStatus::Failed,
                        Some(self.timeout_result()),
                        None,
                    ))
                }
                // 审查本身失败：不能当作通过，交给人工
                Ok(Err(e)) => {
                    tracing::error!(event = "review.failed", code = codes::REVIEW_FAILED, task_id = %task.id, error = %format!("{e:#}"));
                    let v = ReviewVerdict {
                        verdict: Verdict::NeedsHuman,
                        issues: vec![],
                        comment: format!("[{}] 审查失败：{e:#}", codes::REVIEW_FAILED),
                    };
                    self.store.set_task_review(&task.id, &v)?;
                    return Ok(Outcome::Finished(
                        TaskStatus::NeedsHuman,
                        Some(result),
                        Some(v),
                    ));
                }
            };
            self.store.set_task_review(&task.id, &verdict)?;
            tracing::info!(event = "review.verdict", task_id = %task.id, verdict = ?verdict.verdict, issues = verdict.issues.len(), round);
            sink.progress(format!(
                "审查结论：{:?}。{}",
                verdict.verdict, verdict.comment
            ));

            match verdict.verdict {
                Verdict::Pass => {
                    return Ok(Outcome::Finished(
                        TaskStatus::Completed,
                        Some(result),
                        Some(verdict),
                    ))
                }
                Verdict::NeedsHuman => {
                    return Ok(Outcome::Finished(
                        TaskStatus::NeedsHuman,
                        Some(result),
                        Some(verdict),
                    ))
                }
                Verdict::Fail if round < self.max_review_rounds => {
                    // 闭环返工：把审查问题清单作为上下文，再执行一轮（同时写回数据库，重启后返工上下文也不丢）
                    round += 1;
                    context = format!(
                        "{context}\n\n## 上一轮结果未通过审查，请修正以下问题\n{}\n\n## 上一轮结果摘要\n{}",
                        verdict.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"),
                        an_core::truncate_chars(&result.summary, 2000)
                    );
                    self.store.set_task_context(&task.id, &context)?;
                    // 返工重新排队等 agent；排队时间计入任务期限
                    if !self.transition(
                        task,
                        &[TaskStatus::Verifying],
                        TaskStatus::Queued,
                        format!("第 {round} 轮返工，{}", self.queue_message(&task.id)),
                    ) {
                        return Ok(Outcome::Stopped);
                    }
                }
                Verdict::Fail => {
                    return Ok(Outcome::Finished(
                        TaskStatus::Failed,
                        Some(result),
                        Some(verdict),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! 编排器的状态机测试：取消 / 完成竞态、并发批准、排队状态、超时计时。
    //! 用内存库 + 假 agent；交付消息的模型指向一个不可连接的地址，Composer 会立即退回模板。
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use an_core::{AgentAdapter, AgentCard, EventKind, Risk, Role, TaskClass};
    use an_llm::{LlmClient, LlmConfig};

    /// 假 agent：每次执行先等 `hold`，再返回成功；`runs` 记录被执行了几次
    struct FakeAgent {
        card: AgentCard,
        hold: Duration,
        runs: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl AgentAdapter for FakeAgent {
        fn card(&self) -> &AgentCard {
            &self.card
        }
        async fn run(&self, _: TaskSpec, _: EventSink) -> Result<TaskResult> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.hold).await;
            Ok(TaskResult {
                status: WorkerStatus::Succeeded,
                summary: "做完了".into(),
                artifacts: vec![],
                evidence: vec![],
                errors: vec![],
            })
        }
    }

    struct Env {
        orch: Arc<Orchestrator>,
        store: Arc<Store>,
        bus: Arc<Bus>,
        agent: Arc<FakeAgent>,
    }

    /// `max_concurrent`：假 agent 的并发上限；`hold`：每次执行耗时；`timeout`：任务超时
    fn env(max_concurrent: u32, hold: Duration, timeout: Duration) -> Env {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
        let bus = Arc::new(Bus::new(store.clone()));
        let registry = AgentRegistry::new(BTreeMap::new());
        let agent = Arc::new(FakeAgent {
            card: AgentCard {
                id: "fake".into(),
                kind: "test".into(),
                agent_type: "fake".into(),
                location: "本机".into(),
                max_concurrent,
                role: Role::General,
                name: "假 agent".into(),
                description: String::new(),
                tools: vec![],
                enabled: true,
                disabled_reason: None,
            },
            hold,
            runs: AtomicUsize::new(0),
        });
        registry.register(agent.clone());
        // 端口 1 不可连接：交付消息立即退回模板，不依赖真实模型
        let llm = LlmClient::new(LlmConfig {
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "none".into(),
            max_retries: 0,
            ..Default::default()
        })
        .unwrap();
        let orch = Arc::new(Orchestrator::new(
            store.clone(),
            bus.clone(),
            registry,
            None,
            Composer::new(llm),
            0,
            timeout,
            Arc::new(TemplateRegistry::default()),
        ));
        Env {
            orch,
            store,
            bus,
            agent,
        }
    }

    fn route() -> RouteDecision {
        RouteDecision {
            task_class: TaskClass::L1,
            execution_role: Role::General,
            required_tools: vec![],
            risk_level: Risk::Low,
            reason: String::new(),
            template_id: String::new(),
            deliverable: String::new(),
            slots_ok: String::new(),
        }
    }

    fn policy(auto_execute: bool) -> PolicyVerdict {
        PolicyVerdict {
            auto_execute,
            needs_review: false,
            notes: vec!["测试".into()],
            candidates: vec![],
        }
    }

    fn status_of(store: &Store, id: &str) -> TaskStatus {
        store.get_task(id).unwrap().unwrap().status
    }

    /// 等到任务进入 `want` 中的某个状态，最多等 `secs` 秒
    async fn wait_for(store: &Store, id: &str, want: &[TaskStatus], secs: u64) -> TaskStatus {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let s = status_of(store, id);
            if want.contains(&s) {
                return s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "等待 {want:?} 超时，当前状态 {s:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// 某任务的交付消息条数（assistant 消息且关联了该任务）
    fn deliveries(store: &Store, conv: &str, task_id: &str) -> usize {
        store
            .list_messages(conv, None)
            .unwrap()
            .iter()
            .filter(|m| m.role == "assistant" && m.task_id.as_deref() == Some(task_id))
            .count()
    }

    /// 第 1 条：取消和完成同时发生时，状态只能有一个终态，进入终态后不能再变。
    /// 同时：取消返回 true ⇒ 没有交付消息；最终 COMPLETED ⇒ 一定有交付消息。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancel_and_completion_never_overwrite_each_other() {
        let e = env(64, Duration::from_millis(0), Duration::from_secs(10));
        let conv = e.store.create_conversation("t").unwrap().id;
        let mut rx = e.bus.subscribe();
        let mut cases = vec![];
        for i in 0..200u64 {
            let task = e
                .orch
                .submit(&conv, "g", "", route(), policy(true))
                .unwrap();
            let orch = e.orch.clone();
            let id = task.id.clone();
            // 取消时机分散在执行前后，制造竞态
            let canceller = tokio::spawn(async move {
                for _ in 0..(i % 8) {
                    tokio::task::yield_now().await;
                }
                orch.cancel(&id).unwrap()
            });
            cases.push((task.id, canceller));
        }
        let mut results = vec![];
        for (id, h) in cases {
            let cancelled = h.await.unwrap();
            let final_status = wait_for(
                &e.store,
                &id,
                &[
                    TaskStatus::Completed,
                    TaskStatus::Cancelled,
                    TaskStatus::Failed,
                ],
                10,
            )
            .await;
            results.push((id, cancelled, final_status));
        }
        // 等交付消息都写完
        tokio::time::sleep(Duration::from_millis(500)).await;

        // 每个任务的状态事件：进入终态后不能再有状态事件
        let mut statuses: std::collections::HashMap<String, Vec<TaskStatus>> = Default::default();
        while let Ok(ev) = rx.try_recv() {
            if let (EventKind::StatusChanged, Some(tid), Some(s)) =
                (ev.kind, ev.task_id.clone(), ev.status)
            {
                statuses.entry(tid).or_default().push(s);
            }
        }
        for (id, cancelled, final_status) in results {
            let seq = statuses.get(&id).cloned().unwrap_or_default();
            if let Some(pos) = seq.iter().position(|s| s.is_terminal()) {
                assert_eq!(
                    pos,
                    seq.len() - 1,
                    "任务 {id} 进入终态后状态又变了：{seq:?}"
                );
            }
            assert_eq!(
                status_of(&e.store, &id),
                final_status,
                "任务 {id} 最终状态又被改了"
            );
            if cancelled {
                assert_eq!(
                    final_status,
                    TaskStatus::Cancelled,
                    "取消返回 true，但最终状态是 {final_status:?}（{seq:?}）"
                );
                assert_eq!(
                    deliveries(&e.store, &conv, &id),
                    0,
                    "任务 {id} 已取消，不应有交付消息"
                );
            }
            if final_status == TaskStatus::Completed {
                assert_eq!(
                    deliveries(&e.store, &conv, &id),
                    1,
                    "任务 {id} 已完成，应有且只有一条交付消息（{seq:?}）"
                );
            }
        }
    }

    /// 已完成、交付进行中或已交付的任务，取消返回 false，状态保持 COMPLETED
    #[tokio::test]
    async fn cancel_after_completion_is_a_no_op() {
        let e = env(1, Duration::from_millis(0), Duration::from_secs(10));
        let conv = e.store.create_conversation("t").unwrap().id;
        let task = e
            .orch
            .submit(&conv, "g", "", route(), policy(true))
            .unwrap();
        wait_for(&e.store, &task.id, &[TaskStatus::Completed], 5).await;
        assert!(!e.orch.cancel(&task.id).unwrap());
        assert_eq!(status_of(&e.store, &task.id), TaskStatus::Completed);
    }

    /// 运行中取消：状态 CANCELLED，没有交付消息
    #[tokio::test]
    async fn cancel_while_running_wins() {
        let e = env(1, Duration::from_secs(5), Duration::from_secs(10));
        let conv = e.store.create_conversation("t").unwrap().id;
        let task = e
            .orch
            .submit(&conv, "g", "", route(), policy(true))
            .unwrap();
        wait_for(&e.store, &task.id, &[TaskStatus::Running], 5).await;
        assert!(e.orch.cancel(&task.id).unwrap());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(status_of(&e.store, &task.id), TaskStatus::Cancelled);
        assert_eq!(deliveries(&e.store, &conv, &task.id), 0);
    }

    /// 第 2 条：同时发来多个批准请求，只有一个成功，agent 只执行一次
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_approve_runs_once() {
        for _ in 0..20 {
            let e = env(64, Duration::from_millis(50), Duration::from_secs(10));
            let conv = e.store.create_conversation("t").unwrap().id;
            let task = e
                .orch
                .submit(&conv, "g", "", route(), policy(false))
                .unwrap();
            assert_eq!(task.status, TaskStatus::NeedsHuman);
            let barrier = Arc::new(tokio::sync::Barrier::new(10));
            let handles: Vec<_> = (0..10)
                .map(|_| {
                    let (orch, id, b) = (e.orch.clone(), task.id.clone(), barrier.clone());
                    tokio::spawn(async move {
                        b.wait().await;
                        orch.approve(&id).is_ok()
                    })
                })
                .collect();
            let mut ok = 0;
            for h in handles {
                ok += h.await.unwrap() as usize;
            }
            wait_for(&e.store, &task.id, &[TaskStatus::Completed], 5).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(ok, 1, "只能有一个批准成功");
            assert_eq!(e.agent.runs.load(Ordering::SeqCst), 1, "agent 只能执行一次");
        }
    }

    /// 第 4 条：等空闲 agent 的任务保持 QUEUED，拿到 agent 后才是 RUNNING
    #[tokio::test]
    async fn waiting_task_stays_queued() {
        let e = env(1, Duration::from_millis(600), Duration::from_secs(10));
        let conv = e.store.create_conversation("t").unwrap().id;
        let t1 = e
            .orch
            .submit(&conv, "g1", "", route(), policy(true))
            .unwrap();
        wait_for(&e.store, &t1.id, &[TaskStatus::Running], 5).await;
        let t2 = e
            .orch
            .submit(&conv, "g2", "", route(), policy(true))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(status_of(&e.store, &t1.id), TaskStatus::Running);
        assert_eq!(
            status_of(&e.store, &t2.id),
            TaskStatus::Queued,
            "第二个任务还在等 agent，应该是 QUEUED"
        );
        wait_for(&e.store, &t2.id, &[TaskStatus::Running], 5).await;
        wait_for(&e.store, &t2.id, &[TaskStatus::Completed], 5).await;
    }

    /// 第 4 条：超时从拿到 agent 开始算，排队时间不计入
    #[tokio::test]
    async fn queue_wait_does_not_count_toward_timeout() {
        let e = env(1, Duration::from_millis(700), Duration::from_secs(1));
        let conv = e.store.create_conversation("t").unwrap().id;
        let t1 = e
            .orch
            .submit(&conv, "g1", "", route(), policy(true))
            .unwrap();
        wait_for(&e.store, &t1.id, &[TaskStatus::Running], 5).await;
        let t2 = e
            .orch
            .submit(&conv, "g2", "", route(), policy(true))
            .unwrap();
        assert_eq!(
            wait_for(
                &e.store,
                &t1.id,
                &[TaskStatus::Completed, TaskStatus::Failed],
                5
            )
            .await,
            TaskStatus::Completed
        );
        // t2 排队约 0.7 秒、执行 0.7 秒：总耗时超过 1 秒，但执行本身没超时
        assert_eq!(
            wait_for(
                &e.store,
                &t2.id,
                &[TaskStatus::Completed, TaskStatus::Failed],
                5
            )
            .await,
            TaskStatus::Completed
        );
    }

    /// 执行本身超过期限时仍然超时失败
    #[tokio::test]
    async fn execution_over_deadline_still_times_out() {
        let e = env(1, Duration::from_secs(3), Duration::from_millis(300));
        let conv = e.store.create_conversation("t").unwrap().id;
        let t = e
            .orch
            .submit(&conv, "g", "", route(), policy(true))
            .unwrap();
        assert_eq!(
            wait_for(
                &e.store,
                &t.id,
                &[TaskStatus::Completed, TaskStatus::Failed],
                5
            )
            .await,
            TaskStatus::Failed
        );
        let rec = e.store.get_task(&t.id).unwrap().unwrap();
        assert!(rec
            .result
            .unwrap()
            .errors
            .join("")
            .contains(codes::TASK_TIMEOUT));
    }

    /// 执行耗时超过心跳间隔时，应推送编排器进度（走马灯可见）
    #[tokio::test]
    async fn long_run_emits_orchestrator_heartbeat() {
        let hold = Duration::from_secs(crate::run_progress::HEARTBEAT_INTERVAL_SECS + 5);
        let e = env(1, hold, Duration::from_secs(120));
        let conv = e.store.create_conversation("t").unwrap().id;
        let mut rx = e.bus.subscribe();
        let task = e
            .orch
            .submit(&conv, "g", "", route(), policy(true))
            .unwrap();
        wait_for(&e.store, &task.id, &[TaskStatus::Running], 5).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut saw = false;
        while std::time::Instant::now() < deadline {
            if let Ok(ev) = rx.try_recv() {
                if ev.task_id.as_deref() == Some(task.id.as_str())
                    && ev.kind == EventKind::Progress
                    && ev.message.contains("仍在执行中")
                {
                    saw = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        wait_for(&e.store, &task.id, &[TaskStatus::Completed], 120).await;
        assert!(saw, "长任务执行期间应收到编排器心跳");
    }
}
