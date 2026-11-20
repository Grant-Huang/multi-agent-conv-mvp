//! 可选模型解释层（S7）：只追加定性文字，不改 `AdoptionStats` 数字。
//! 默认关闭；开启时用后台槽位，`max_wait=0`，取不到立即跳过。

use std::sync::Arc;
use std::time::Duration;

use an_llm::{Priority, Scheduler, Who};
use async_trait::async_trait;

use super::finding::FindingDraft;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplainOutcome {
    /// 无槽位或其他原因跳过；确定性结论保留。
    Skipped { reason: String },
    /// 追加解释后的假设；`model_delta` 供闸门 ±1（须带理由）。
    Applied {
        hypothesis: String,
        model_delta: i8,
        model_reason: String,
    },
}

#[async_trait]
pub trait ModelExplainer: Send + Sync {
    async fn explain(&self, draft: &FindingDraft) -> ExplainOutcome;
}

/// 测试 / 注入用：恒跳过。
pub struct AlwaysSkipExplainer {
    pub reason: String,
}

#[async_trait]
impl ModelExplainer for AlwaysSkipExplainer {
    async fn explain(&self, _draft: &FindingDraft) -> ExplainOutcome {
        ExplainOutcome::Skipped {
            reason: self.reason.clone(),
        }
    }
}

/// 测试用：恒成功追加解释（不碰数字）。
pub struct FixedExplainer {
    pub suffix: String,
    pub model_delta: i8,
    pub model_reason: String,
}

#[async_trait]
impl ModelExplainer for FixedExplainer {
    async fn explain(&self, draft: &FindingDraft) -> ExplainOutcome {
        let hypothesis = if self.suffix.is_empty() {
            draft.hypothesis.clone()
        } else {
            format!("{}\n{}", draft.hypothesis, self.suffix)
        };
        // 断言路径：调用方须保证 stats 未被改写（本结构只读 draft）
        ExplainOutcome::Applied {
            hypothesis,
            model_delta: self.model_delta,
            model_reason: self.model_reason.clone(),
        }
    }
}

/// 生产路径：抢后台槽位（不等待）；抢到后追加固定解释句（不改统计数字）。
/// 真实 LLM 文案可后续替换；S7 先保证「不排队 / 可跳过 / 不改数」。
pub struct SlotGatedExplainer {
    pub sched: Scheduler,
}

#[async_trait]
impl ModelExplainer for SlotGatedExplainer {
    async fn explain(&self, draft: &FindingDraft) -> ExplainOutcome {
        let who = Who {
            role: "introspection",
            priority: Priority::Background,
            req_id: 0,
        };
        match self.sched.acquire(&who, 64, Duration::ZERO).await {
            Err(_) => ExplainOutcome::Skipped {
                reason: "无槽位".into(),
            },
            Ok(_lease) => {
                let hypothesis = format!(
                    "{}\n（模型解释）以上数字由代码统计确定；此处仅作定性说明，不修改表内数值。",
                    draft.hypothesis
                );
                ExplainOutcome::Applied {
                    hypothesis,
                    model_delta: 0,
                    model_reason: "解释层未调整分数".into(),
                }
            }
        }
    }
}

/// 从运行时后端里取第一个可用调度器，构造槽位门控解释器。
pub fn slot_explainer_from_backends(
    backends: &crate::backends::Backends,
) -> Option<Arc<dyn ModelExplainer>> {
    let sched = backends.backends().next()?.sched.clone();
    Some(Arc::new(SlotGatedExplainer { sched }))
}
