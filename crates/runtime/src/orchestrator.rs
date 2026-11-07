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
    codes, now, AgentError, EventSink, Evidence, PolicyVerdict, ReviewVerdict, RouteDecision,
    TaskRecord, TaskResult, TaskSpec, TaskStatus, Verdict, WorkerStatus,
};
use anyhow::{bail, Context, Result};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use crate::bus::Bus;
use crate::composer::Composer;
use crate::registry::AgentRegistry;
use crate::store::{Store, ACTIVE_STATUSES};

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
            status_lock: Mutex::new(()),
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

    /// 人工审批通过后开始执行。并发批准时只有一个成功，其余返回错误。
    pub fn approve(self: &Arc<Self>, task_id: &str) -> Result<()> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
        // 判断和修改在一条 UPDATE 里：不会出现两个请求都通过检查、任务被执行两次
        let approved = {
            let _guard = self.status_lock.lock().unwrap_or_else(|e| e.into_inner());
            let approved = self.store.approve_task(task_id)?;
            if approved {
                self.bus.status(
                    &task.conversation_id,
                    &task.id,
                    TaskStatus::Queued,
                    self.queue_message(task_id),
                );
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
        self.spawn(TaskRecord {
            status: TaskStatus::Queued,
            ..task
        });
        Ok(())
    }

    /// 取消任务。返回 true 表示确实取消了一个未结束的任务。
    /// 先在数据库里改成 CANCELLED，成功了才中止后台执行：已经完成的任务不会被取消，交付照常进行。
    pub fn cancel(&self, task_id: &str) -> Result<bool> {
        let task = self.store.get_task(task_id)?.context("任务不存在")?;
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
        }
        // abort 执行中的 tokio 任务；子进程由各 agent 的清理逻辑（kill_on_drop / 进程组）一起结束
        if let Some(h) = self.running.lock().unwrap().remove(task_id) {
            h.abort();
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
        let (status, result, review) = match self.run_with_review(&task).await {
            Ok(Outcome::Finished(s, r, v)) => (s, r, v),
            Ok(Outcome::Stopped) => return,
            Err(e) => (
                TaskStatus::Failed,
                Some(TaskResult::failed(format!("{e:#}"))),
                None,
            ),
        };
        // 状态和结果一起写；已被取消时什么都不写，也不交付
        if !self.finish(
            &task,
            &FINISH_FROM,
            status,
            result.as_ref(),
            format!("任务结束：{}", status.as_str()),
        ) {
            return;
        }

        // 交付：由 Composer 生成回复，写入会话
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
            .acquire(task.role, |msg| sink.progress(msg))
            .await
        {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(event = "task.no_agent", code = e.code, task_id = %task.id, role = task.role.as_str(), error = %e.message);
                return Step::Done(TaskResult::failed_code(e.code, e.message));
            }
        };
        let msg = if round == 0 {
            "开始执行".to_string()
        } else {
            format!("第 {round} 轮返工")
        };
        if !self.transition(task, &[TaskStatus::Queued], TaskStatus::Running, msg) {
            return Step::Stopped;
        }
        let deadline = *deadline.get_or_insert_with(|| Instant::now() + self.task_timeout);

        let card = lease.agent.card().clone();
        // 节点 agent 的名称里已经带了机器名，不再重复
        let place = if card.kind == "node" {
            card.kind.clone()
        } else {
            format!("{}，{}", card.kind, card.location)
        };
        sink.progress(format!("由 {} 执行（{place}）", card.name));
        tracing::info!(event = "agent.run_start", task_id = %task.id, agent_id = %card.id, kind = %card.kind, location = %card.location);
        let started = std::time::Instant::now();
        let outcome = tokio::time::timeout_at(deadline, lease.agent.run(spec, sink.clone())).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        // 名额在审查之前归还，让排队的任务尽早开始
        drop(lease);
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
        }
        result.evidence.insert(
            0,
            Evidence {
                kind: "executed_by".into(),
                content: format!("{}（{place}）", card.name),
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
        }
    }

    fn policy(auto_execute: bool) -> PolicyVerdict {
        PolicyVerdict {
            auto_execute,
            needs_review: false,
            notes: vec!["测试".into()],
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
}
