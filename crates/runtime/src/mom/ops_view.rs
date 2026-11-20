//! 运营桌面读模型（docs/mom/OPS_DESK_TASKS.md D1-1）。
//!
//! 纯函数：从引擎当前状态投影出桌面行与状态条，不写库、不发卡片。
//! 分组与排序规则见任务卡；未知阶段走 `unknown`，零案例 `state = "no_data"`。

use super::ops_desk::{case_title, stage_label};
use super::{fixture_policy, Case, Engine};
use anyhow::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeskRow {
    pub case_id: String,
    pub title: String,
    pub stage: String,
    pub stage_label: String,
    pub group: String,
    pub owner: String,
    pub deadline_minutes_left: Option<i64>,
    pub stale: bool,
    pub provenance: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeskView {
    pub mode: &'static str,
    pub now: u64,
    pub state: &'static str,
    pub strip: DeskStrip,
    pub rows: Vec<DeskRow>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeskStrip {
    pub pending: usize,
    pub blocked: usize,
    pub overdue: usize,
    pub risk: usize,
    pub completed: usize,
}

const GROUP_ORDER: &[&str] = &[
    "pending",
    "blocked",
    "overdue",
    "risk",
    "active",
    "unknown",
    "completed",
];

fn group_rank(group: &str) -> usize {
    GROUP_ORDER
        .iter()
        .position(|g| *g == group)
        .unwrap_or(GROUP_ORDER.len())
}

fn is_known_stage(stage: &str) -> bool {
    matches!(
        stage,
        "WAITING_HUMAN"
            | "BLOCKED"
            | "BUDGET_EXHAUSTED"
            | "CLOSED"
            | "CANCELLED"
            | "ASSESS"
            | "VERIFYING"
            | "INVESTIGATING"
    )
}

fn classify_group(case: &Case) -> &'static str {
    match case.stage.as_str() {
        "WAITING_HUMAN" => "pending",
        "BLOCKED" | "BUDGET_EXHAUSTED" => "blocked",
        "CLOSED" | "CANCELLED" => "completed",
        _ if !case.breaches.is_empty() => "overdue",
        _ if !case.alerts.is_empty() => "risk",
        "ASSESS" | "VERIFYING" | "INVESTIGATING" => "active",
        _ if !is_known_stage(&case.stage) => "unknown",
        _ => "active",
    }
}

fn row_label(case: &Case, group: &str) -> String {
    match group {
        "blocked" if case.stage == "BUDGET_EXHAUSTED" => "受阻：预算耗尽".into(),
        "overdue" => "已超时".into(),
        "risk" => "有超时风险".into(),
        "unknown" => format!("未知状态（{}）", case.stage),
        _ => stage_label(&case.stage).into(),
    }
}

fn deadline_minutes_left(case: &Case, now: u64) -> Option<i64> {
    case.deadlines
        .iter()
        .filter(|(kind, _)| !case.breaches.iter().any(|b| b == *kind))
        .map(|(_, at)| *at as i64 - now as i64)
        .min()
}

fn is_terminal(stage: &str) -> bool {
    matches!(stage, "CLOSED" | "CANCELLED")
}

fn row_for(case: &Case, now: u64) -> DeskRow {
    let group = classify_group(case);
    let ttl = fixture_policy(&case.scenario).snapshot_ttl_minutes;
    let stale = !is_terminal(&case.stage) && now > case.resource_observed_at.saturating_add(ttl);
    DeskRow {
        case_id: case.id.clone(),
        title: case_title(case),
        stage: case.stage.clone(),
        stage_label: row_label(case, group),
        group: group.into(),
        owner: "调度".into(),
        deadline_minutes_left: deadline_minutes_left(case, now),
        stale,
        provenance: "simulation",
    }
}

/// 桌面读模型：按风险与时限排序；含已完成行（折叠由前端做）。
pub fn desk_view(engine: &Engine) -> Result<DeskView> {
    let now = engine.now();
    let mut rows: Vec<DeskRow> = engine
        .case_ids()
        .into_iter()
        .map(|id| {
            let case = engine.case(&id)?;
            Ok(row_for(case, now))
        })
        .collect::<Result<Vec<_>>>()?;
    rows.sort_by(|a, b| {
        group_rank(&a.group)
            .cmp(&group_rank(&b.group))
            .then_with(
                || match (a.deadline_minutes_left, b.deadline_minutes_left) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                },
            )
            .then_with(|| a.case_id.cmp(&b.case_id))
    });
    let mut strip = DeskStrip::default();
    for row in &rows {
        match row.group.as_str() {
            "pending" => strip.pending += 1,
            "blocked" => strip.blocked += 1,
            "overdue" => strip.overdue += 1,
            "risk" => strip.risk += 1,
            "completed" => strip.completed += 1,
            _ => {}
        }
    }
    Ok(DeskView {
        mode: "simulation",
        now,
        state: if rows.is_empty() { "no_data" } else { "ok" },
        strip,
        rows,
    })
}
