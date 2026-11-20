//! 发送闸门：规则打分为主，模型只做 ±1。

use super::finding::Verification;

#[derive(Debug, Clone, PartialEq)]
pub struct GateInput {
    pub finding_id: String,
    pub verification: Verification,
    pub impact_cases: u32,
    pub has_next_step: bool,
    pub first_or_worsened: bool,
    pub time_sensitive: bool,
    pub already_pushed_unread: bool,
    pub pushed_today: u32,
    /// -1 / 0 / 1；无模型时为 0。
    pub model_delta: i8,
    /// `model_delta != 0` 时必填；无理由则忽略 delta。
    pub model_reason: String,
    /// 因日上限压下时，提示「另有 N 项」的 N（由调用方传入）。
    pub extra_pending: u32,
    /// 是否允许主动打扰用户。false = 仅内省页（值得调查 ≠ 值得推送）。
    pub interrupt_eligible: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateConfig {
    pub push_threshold: f64,
    pub daily_cap: u32,
    pub impact_threshold: u32,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            push_threshold: 4.0,
            daily_cap: 1,
            impact_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateDecision {
    pub score: f64,
    pub should_push: bool,
    /// 永远非空：推或不推都要写清。
    pub reason: String,
}

pub fn decide(input: &GateInput, cfg: &GateConfig) -> GateDecision {
    let mut notes: Vec<String> = Vec::new();

    let mut rules_score = 0.0;
    if input.verification == Verification::Supported {
        rules_score += 2.0;
        notes.push("验真支持 +2".into());
    }
    if input.impact_cases >= cfg.impact_threshold {
        rules_score += 1.0;
        notes.push(format!(
            "影响面 {}≥{} +1",
            input.impact_cases, cfg.impact_threshold
        ));
    }
    if input.has_next_step {
        rules_score += 1.0;
        notes.push("有明确下一步 +1".into());
    }
    if input.first_or_worsened {
        rules_score += 1.0;
        notes.push("首次或恶化 +1".into());
    }
    if input.time_sensitive {
        rules_score += 1.0;
        notes.push("有时效 +1".into());
    }

    let mut effective_delta: i8 = 0;
    if !(-1..=1).contains(&input.model_delta) {
        notes.push("非法 delta，已忽略".into());
    } else if input.model_delta != 0 {
        if input.model_reason.trim().is_empty() {
            notes.push("模型 delta 无理由，已忽略".into());
        } else {
            effective_delta = input.model_delta;
            notes.push(format!(
                "模型 {:+}（{}）",
                effective_delta, input.model_reason
            ));
        }
    }

    let score = rules_score + f64::from(effective_delta);

    if input.verification != Verification::Supported {
        return GateDecision {
            score,
            should_push: false,
            reason: format!(
                "不推送：验真不是 supported（{:?}）。{}",
                input.verification,
                notes.join("；")
            ),
        };
    }
    if !input.interrupt_eligible {
        return GateDecision {
            score,
            should_push: false,
            reason: format!(
                "不推送：仅内省页（值得调查，不主动打扰）。{}",
                notes.join("；")
            ),
        };
    }
    if input.already_pushed_unread {
        return GateDecision {
            score,
            should_push: false,
            reason: format!("不推送：已推送且未读，不重复推。{}", notes.join("；")),
        };
    }
    if input.pushed_today >= cfg.daily_cap {
        return GateDecision {
            score,
            should_push: false,
            reason: format!(
                "不推送：已达日上限 {}；另有 {} 项。{}",
                cfg.daily_cap,
                input.extra_pending,
                notes.join("；")
            ),
        };
    }

    // 模型不能单独把规则分从阈下抬到阈上
    if rules_score < cfg.push_threshold
        && rules_score + f64::from(effective_delta) >= cfg.push_threshold
    {
        return GateDecision {
            score,
            should_push: false,
            reason: format!(
                "不推送：规则分 {rules_score} 低于阈值 {}，模型不能单独越阈。{}",
                cfg.push_threshold,
                notes.join("；")
            ),
        };
    }

    if score < cfg.push_threshold {
        return GateDecision {
            score,
            should_push: false,
            reason: format!(
                "不推送：分数 {score} 低于阈值 {}。{}",
                cfg.push_threshold,
                notes.join("；")
            ),
        };
    }

    GateDecision {
        score,
        should_push: true,
        reason: format!(
            "推送：分数 {score} ≥ 阈值 {}。{}",
            cfg.push_threshold,
            notes.join("；")
        ),
    }
}
