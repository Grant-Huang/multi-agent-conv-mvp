//! 内省数据来源：只读接口。生产路径不得写 MOM。

use anyhow::Result;

/// 只读快照；内省不得持有 `Engine` 的 `&mut`。
pub trait SourceAdapter: Send + Sync {
    fn source_id(&self) -> &str;

    /// 列出可用于统计的案例摘要（已从引擎拷贝出来的自有结构）。
    fn list_case_summaries(&self) -> Result<Vec<CaseSummary>>;

    /// 校验引用是否仍可解析（`case_id` 存在；可选事件序号）。
    fn resolve_ref(&self, case_id: &str, event_seq: Option<u64>) -> Result<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSummary {
    pub case_id: String,
    /// 分组键；空字符串视为 `"_default"`。
    pub template: String,
    pub scenario: String,
    pub approvals: Vec<ApprovalSummary>,
    pub actions: Vec<ActionSummary>,
    /// `(sequence, at)`，用于耗时与引用解析。
    pub event_times: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalSummary {
    pub id: String,
    pub scope: String,
    pub approver: String,
    pub expires_at: u64,
    pub plan_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSummary {
    pub key: String,
    /// 与 MOM 引擎一致：`ACCEPTED` / `FAILED` / `VERIFIED` / `UNKNOWN` 等。
    pub state: String,
    pub verification: Option<String>,
    pub accepted_at: u64,
    pub complete_at: u64,
}

/// 测试与分析共用的内存来源（不是生产写路径）。
#[derive(Debug, Clone, Default)]
pub struct MemorySource {
    pub source_id: String,
    pub cases: Vec<CaseSummary>,
}

impl SourceAdapter for MemorySource {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn list_case_summaries(&self) -> Result<Vec<CaseSummary>> {
        Ok(self.cases.clone())
    }

    fn resolve_ref(&self, case_id: &str, event_seq: Option<u64>) -> Result<bool> {
        let Some(c) = self.cases.iter().find(|c| c.case_id == case_id) else {
            return Ok(false);
        };
        Ok(match event_seq {
            None => true,
            Some(seq) => c.event_times.iter().any(|(s, _)| *s == seq),
        })
    }
}

/// 从 MOM 引擎拷贝只读摘要。只调用 `case_ids` / `case` / `events` / `now`。
pub struct MomSource {
    inner: MemorySource,
}

impl MomSource {
    pub fn from_engine(engine: &crate::mom::Engine) -> Result<Self> {
        let mut cases = Vec::new();
        for id in engine.case_ids() {
            let c = engine.case(&id)?;
            // Engine::events 的 limit 必须在 1..=1000
            let events = engine.events(&id, 0, 1000)?;
            cases.push(CaseSummary {
                case_id: c.id.clone(),
                template: if c.template.is_empty() {
                    "_default".into()
                } else {
                    c.template.clone()
                },
                scenario: c.scenario.clone(),
                approvals: c
                    .approvals
                    .iter()
                    .map(|(scope, a)| ApprovalSummary {
                        id: scope.clone(),
                        scope: a.scope.clone(),
                        approver: a.approver.clone(),
                        expires_at: a.expires_at,
                        plan_revision: a.plan_revision,
                    })
                    .collect(),
                actions: c
                    .actions
                    .values()
                    .map(|a| ActionSummary {
                        key: a.key.clone(),
                        state: a.state.clone(),
                        verification: a.verification.clone(),
                        accepted_at: a.accepted_at,
                        complete_at: a.complete_at,
                    })
                    .collect(),
                event_times: events.iter().map(|e| (e.sequence, e.at)).collect(),
            });
        }
        Ok(Self {
            inner: MemorySource {
                source_id: "mom".into(),
                cases,
            },
        })
    }

    /// 引擎当前虚拟时间，供统计「审批是否过期」使用。
    pub fn now_from_engine(engine: &crate::mom::Engine) -> u64 {
        engine.now()
    }
}

impl SourceAdapter for MomSource {
    fn source_id(&self) -> &str {
        self.inner.source_id()
    }

    fn list_case_summaries(&self) -> Result<Vec<CaseSummary>> {
        self.inner.list_case_summaries()
    }

    fn resolve_ref(&self, case_id: &str, event_seq: Option<u64>) -> Result<bool> {
        self.inner.resolve_ref(case_id, event_seq)
    }
}
