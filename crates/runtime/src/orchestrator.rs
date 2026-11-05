//! Orchestrator：创建任务、派发给 Agent、回收结果、组织审查与返工、处理取消和审批。
//!
//! 单个任务的执行流程：
//! ```text
//! submit ─┬─ Policy 不允许自动执行 → NEEDS_HUMAN（等待 approve）
//!         └─ QUEUED → RUNNING → Worker 返回
//!                        ├─ Worker 失败                 → FAILED
//!                        ├─ 不需要审查                   → COMPLETED
//!                        └─ VERIFYING → Reviewer 意见
//!                              ├─ pass                    → COMPLETED
//!                              ├─ needs_human             → NEEDS_HUMAN
//!                              └─ fail → 还有返工次数？ → 带着问题清单重新 RUNNING
//!                                                   否   → FAILED
//! 最后由 Composer 生成交付消息，写入会话并推送给前端。
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_agents::ReviewerAgent;
use an_core::{
    now, AgentUnavailable, EventSink, Evidence, PolicyVerdict, ReviewVerdict, RouteDecision, TaskRecord, TaskResult, TaskSpec, TaskStatus, Verdict,
    WorkerStatus,
};
use anyhow::{bail, Context, Result};
use tokio::task::AbortHandle;

use crate::bus::Bus;
use crate::composer::Composer;
use crate::registry::AgentRegistry;
use crate::store::Store;

pub struct Orchestrator {
    store: Arc<Store>,
    bus: Arc<Bus>,
    registry: Arc<AgentRegistry>,
    /// 验收用的 Reviewer（未启用时为 None）
    reviewer: Option<Arc<ReviewerAgent>>,
    composer: Composer,
    max_review_rounds: u32,
    task_timeout: Duration,
    /// 正在执行的任务，用于取消
    running: Mutex<HashMap<String, AbortHandle>>,
    /// 任务的执行上下文（审批后启动时需要）
    contexts: Mutex<HashMap<String, String>>,
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
    ) -> Self {
        Self {
            store,
            bus,
            registry,
            reviewer,
            composer,
            max_review_rounds,
            task_timeout,
            running: Mutex::new(HashMap::new()),
            contexts: Mutex::new(HashMap::new()),
        }
    }

    fn set_status(&self, task: &TaskRecord, status: TaskStatus, msg: impl Into<String>) {
        if let Err(e) = self.store.set_task_status(&task.id, status) {
            tracing::error!(error = %e, task = %task.id, "更新任务状态失败");
        }
        self.bus.status(&task.conversation_id, &task.id, status, msg);
    }

    /// 创建任务并（在 Policy 允许时）开始执行
    pub fn submit(
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
            created_at: ts.clone(),
            updated_at: ts,
        };
        self.store.insert_task(&task)?;
        self.contexts.lock().unwrap().insert(task.id.clone(), context.to_string());
        self.bus.status(&task.conversation_id, &task.id, TaskStatus::Created, format!("任务已创建，执行角色：{}", task.role.as_str()));

        if task.policy.auto_execute {
            self.start(task.clone());
        } else {
            self.set_status(&task, TaskStatus::NeedsHuman, format!("等待人工审批：{}", task.policy.notes.join("；")));
        }
        Ok(TaskRecord { status: self.store.get_task(&task.id)?.map(|t| t.status).unwrap_or(task.status), ..task })
    }

    /// 人工审批通过后开始执行
    pub fn approve(self: &Arc<Self>, task_id: &str) -> Result<()> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        if task.status != TaskStatus::NeedsHuman || task.result.is_some() {
            bail!("只有等待审批（尚未执行）的任务可以批准，当前状态 {}", task.status.as_str());
        }
        EventSink::new(self.bus.clone(), &task.conversation_id, &task.id).progress("已获人工批准，开始执行");
        self.start(task);
        Ok(())
    }

    /// 取消任务。返回 true 表示确实取消了一个未结束的任务
    pub fn cancel(&self, task_id: &str) -> Result<bool> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        if task.status.is_terminal() {
            return Ok(false);
        }
        // abort 执行中的 tokio 任务；子进程会因 kill_on_drop 被一起杀掉
        if let Some(h) = self.running.lock().unwrap().remove(task_id) {
            h.abort();
        }
        self.contexts.lock().unwrap().remove(task_id);
        self.set_status(&task, TaskStatus::Cancelled, "任务已取消");
        Ok(true)
    }

    /// 启动后台执行（持有锁时 spawn，保证 abort handle 一定先登记，再被执行完的任务移除）
    fn start(self: &Arc<Self>, task: TaskRecord) {
        self.set_status(&task, TaskStatus::Queued, "排队中");
        let mut running = self.running.lock().unwrap();
        let this = self.clone();
        let id = task.id.clone();
        let handle = tokio::spawn(async move {
            let id = task.id.clone();
            this.execute(task).await;
            this.running.lock().unwrap().remove(&id);
            this.contexts.lock().unwrap().remove(&id);
        });
        running.insert(id, handle.abort_handle());
    }

    async fn execute(self: &Arc<Self>, task: TaskRecord) {
        let outcome = tokio::time::timeout(self.task_timeout, self.run_with_review(&task)).await;
        let (status, result, review) = match outcome {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => (TaskStatus::Failed, Some(TaskResult::failed(format!("{e:#}"))), None),
            Err(_) => (TaskStatus::Failed, Some(TaskResult::failed(format!("任务超时（{} 秒）", self.task_timeout.as_secs()))), None),
        };
        if let Some(r) = &result {
            let _ = self.store.set_task_result(&task.id, r);
        }
        self.set_status(&task, status, format!("任务结束：{}", status.as_str()));

        // 交付：由 Composer 生成回复，写入会话
        let reply = self.composer.compose(&task, status, result.as_ref(), review.as_ref()).await;
        match self.store.add_message(&task.conversation_id, "assistant", &reply, Some(&task.id)) {
            Ok(_) => self.bus.assistant_message(&task.conversation_id, Some(&task.id), &reply),
            Err(e) => tracing::error!(error = %e, "写入交付消息失败"),
        }
    }

    /// 按候选顺序执行：首选 agent 启动失败（AgentUnavailable）时自动换下一个。
    /// 任务执行中途失败不换，避免有副作用的操作被另一个 agent 重复执行。
    async fn run_with_fallback(&self, task: &TaskRecord, spec: TaskSpec, sink: &EventSink) -> TaskResult {
        let candidates = self.registry.candidates(task.role);
        if candidates.is_empty() {
            return TaskResult::failed(format!("没有可用的 {} agent", task.role.as_str()));
        }
        let mut skipped = vec![];
        for agent in candidates {
            let name = agent.card().name.clone();
            sink.progress(format!("由 {name} 执行"));
            match agent.run(spec.clone(), sink.clone()).await {
                Ok(mut r) => {
                    r.evidence.insert(0, Evidence { kind: "executed_by".into(), content: format!("{name}（{}）", agent.card().kind) });
                    return r;
                }
                Err(e) if e.downcast_ref::<AgentUnavailable>().is_some() => {
                    sink.progress(format!("{e:#}，尝试下一个 agent"));
                    skipped.push(format!("{e:#}"));
                }
                Err(e) => return TaskResult::failed(format!("{name}：{e:#}")),
            }
        }
        TaskResult::failed(format!("所有 {} agent 都无法启动：{}", task.role.as_str(), skipped.join("；")))
    }

    /// 执行 Worker，并按 Policy 组织审查与返工
    async fn run_with_review(&self, task: &TaskRecord) -> Result<(TaskStatus, Option<TaskResult>, Option<ReviewVerdict>)> {
        let mut context = self.contexts.lock().unwrap().get(&task.id).cloned().unwrap_or_default();
        let sink = EventSink::new(self.bus.clone(), &task.conversation_id, &task.id);

        let mut round = 0;
        loop {
            self.set_status(task, TaskStatus::Running, if round == 0 { "开始执行".to_string() } else { format!("第 {round} 轮返工") });
            let spec = TaskSpec {
                task_id: task.id.clone(),
                conversation_id: task.conversation_id.clone(),
                goal: task.goal.clone(),
                context: context.clone(),
                route: task.route.clone(),
            };
            let result = self.run_with_fallback(task, spec, &sink).await;
            self.store.set_task_result(&task.id, &result)?;

            // Worker 自己失败：不进入审查
            if result.status == WorkerStatus::Failed {
                return Ok((TaskStatus::Failed, Some(result), None));
            }
            // 不需要审查：Worker 成功即视为完成
            let (true, Some(reviewer)) = (task.policy.needs_review, &self.reviewer) else {
                return Ok((TaskStatus::Completed, Some(result), None));
            };

            self.set_status(task, TaskStatus::Verifying, "Reviewer 审查中");
            let verdict = match reviewer.review(&task.goal, &result).await {
                Ok(v) => v,
                // 审查本身失败：不能当作通过，交给人工
                Err(e) => {
                    let v = ReviewVerdict { verdict: Verdict::NeedsHuman, issues: vec![], comment: format!("审查失败：{e:#}") };
                    self.store.set_task_review(&task.id, &v)?;
                    return Ok((TaskStatus::NeedsHuman, Some(result), Some(v)));
                }
            };
            self.store.set_task_review(&task.id, &verdict)?;
            sink.progress(format!("审查结论：{:?}。{}", verdict.verdict, verdict.comment));

            match verdict.verdict {
                Verdict::Pass => return Ok((TaskStatus::Completed, Some(result), Some(verdict))),
                Verdict::NeedsHuman => return Ok((TaskStatus::NeedsHuman, Some(result), Some(verdict))),
                Verdict::Fail if round < self.max_review_rounds => {
                    // 闭环返工：把审查问题清单作为上下文，再执行一轮
                    round += 1;
                    context = format!(
                        "{context}\n\n## 上一轮结果未通过审查，请修正以下问题\n{}\n\n## 上一轮结果摘要\n{}",
                        verdict.issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"),
                        an_core::truncate_chars(&result.summary, 2000)
                    );
                }
                Verdict::Fail => return Ok((TaskStatus::Failed, Some(result), Some(verdict))),
            }
        }
    }
}
