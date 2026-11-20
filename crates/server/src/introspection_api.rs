//! 业务内省 HTTP（S6）：`/api/introspection/*`。
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET  | /api/introspection/findings | 结论列表（默认不含已否决） |
//! | GET  | /api/introspection/findings/{id} | 详情与标注 |
//! | POST | /api/introspection/findings/{id}/verdict | `{verdict, note}` |
//! | POST | /api/introspection/findings/{id}/label | `{kind, note}` |
//! | POST | /api/introspection/run | 手动触发（可选 `fixture` 注入演示数据） |
//! | GET  | /api/introspection/unread | `{unread, conversation_id}` |
//! | POST | /api/introspection/read | 全标已读 |
//!
//! 鉴权：`is_operator`；拒绝时 403，body 仅 `{"error"}`（不含条数与标题）。

use std::sync::Arc;

use an_runtime::introspect::{
    ensure_operator, run_once, AccessDenied, ActionSummary, ApprovalSummary, CaseSummary,
    MemorySource, RunOnceOptions, RunOnceOutcome, RunOnceRequest, MAIN_CONVERSATION_TITLE,
};
use an_runtime::Runtime;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppError;

type AppState = Arc<Runtime>;
type ApiResult = Result<Json<Value>, AppError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/introspection/findings", get(list_findings))
        .route("/api/introspection/findings/{id}", get(get_finding))
        .route(
            "/api/introspection/findings/{id}/verdict",
            post(set_verdict),
        )
        .route("/api/introspection/findings/{id}/label", post(add_label))
        .route("/api/introspection/run", post(run_manual))
        .route("/api/introspection/unread", get(unread))
        .route("/api/introspection/read", post(mark_read))
}

fn require_operator(rt: &Runtime) -> Result<(), AppError> {
    ensure_operator("me", &rt.config.introspection)
        .map_err(|AccessDenied| AppError(StatusCode::FORBIDDEN, "需要运营负责人权限".into()))
}

fn finding_json(f: &an_runtime::introspect::model::IntrospectionFinding) -> Value {
    let payload: Value = serde_json::from_str(&f.json).unwrap_or(Value::Null);
    json!({
        "id": f.id,
        "job_id": f.job_id,
        "topic": f.topic,
        "json": payload,
        "verification": f.verification,
        "gate_score": f.gate_score,
        "gate_reason": f.gate_reason,
        "pushed": f.pushed,
        "verdict": f.verdict,
        "verdict_note": f.verdict_note,
        "created_at": f.created_at,
        "updated_at": f.updated_at,
    })
}

#[derive(Deserialize)]
struct ListQuery {
    /// 主题过滤
    topic: Option<String>,
    /// 为 true 时含已否决
    #[serde(default)]
    include_rejected: bool,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

async fn list_findings(State(rt): State<AppState>, Query(q): Query<ListQuery>) -> ApiResult {
    require_operator(&rt)?;
    let mut rows = rt
        .store
        .introspection_list_findings(q.topic.as_deref(), q.limit.max(1))?;
    if !q.include_rejected {
        rows.retain(|f| f.verdict.as_deref() != Some("rejected"));
    }
    Ok(Json(json!({
        "findings": rows.iter().map(finding_json).collect::<Vec<_>>(),
    })))
}

async fn get_finding(State(rt): State<AppState>, Path(id): Path<String>) -> ApiResult {
    require_operator(&rt)?;
    let Some(f) = rt.store.introspection_get_finding(&id)? else {
        return Err(AppError(StatusCode::NOT_FOUND, format!("结论 {id} 不存在")));
    };
    let labels = rt.store.introspection_labels(&id)?;
    Ok(Json(json!({
        "finding": finding_json(&f),
        "labels": labels,
    })))
}

#[derive(Deserialize)]
struct VerdictBody {
    verdict: String,
    #[serde(default)]
    note: String,
}

async fn set_verdict(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<VerdictBody>,
) -> ApiResult {
    require_operator(&rt)?;
    let v = b.verdict.trim();
    if v != "confirmed" && v != "rejected" {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "verdict 须为 confirmed 或 rejected".into(),
        ));
    }
    if !rt.store.introspection_set_verdict(&id, v, &b.note)? {
        return Err(AppError(StatusCode::NOT_FOUND, format!("结论 {id} 不存在")));
    }
    let f = rt.store.introspection_get_finding(&id)?.expect("just set");
    Ok(Json(json!({ "ok": true, "finding": finding_json(&f) })))
}

#[derive(Deserialize)]
struct LabelBody {
    kind: String,
    #[serde(default)]
    note: String,
}

async fn add_label(
    State(rt): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LabelBody>,
) -> ApiResult {
    require_operator(&rt)?;
    if rt.store.introspection_get_finding(&id)?.is_none() {
        return Err(AppError(StatusCode::NOT_FOUND, format!("结论 {id} 不存在")));
    }
    let kind = b.kind.trim();
    if kind != "should_push" && kind != "should_not_push" {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "kind 须为 should_push 或 should_not_push".into(),
        ));
    }
    let label = rt.store.introspection_add_label(&id, kind, &b.note)?;
    Ok(Json(json!({ "ok": true, "label": label })))
}

#[derive(Deserialize, Default)]
struct RunBody {
    /// `adoption_push`：可推送的采纳模式夹具；`adoption_hold`：闸门压下的夹具。
    fixture: Option<String>,
}

fn fixture_source(name: &str) -> Result<MemorySource, AppError> {
    match name {
        "adoption_push" => Ok(MemorySource {
            source_id: "fixture".into(),
            cases: (0..3).map(|i| case(&format!("c{i}"), "tpl-e2e")).collect(),
        }),
        // 单案例：有验真但影响面不足；配合较高 push_threshold（如 5）可入库但不推送
        "adoption_hold" => Ok(MemorySource {
            source_id: "fixture".into(),
            cases: vec![case("only", "tpl-hold")],
        }),
        other => Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("未知 fixture: {other}"),
        )),
    }
}

fn case(id: &str, template: &str) -> CaseSummary {
    CaseSummary {
        case_id: id.into(),
        template: template.into(),
        scenario: "s1".into(),
        approvals: vec![ApprovalSummary {
            id: "production".into(),
            scope: "production".into(),
            approver: "supervisor".into(),
            expires_at: 10_000,
            plan_revision: 1,
        }],
        actions: vec![ActionSummary {
            key: format!("{id}:a"),
            state: "ACCEPTED".into(),
            verification: Some("ok".into()),
            accepted_at: 100,
            complete_at: 102,
        }],
        event_times: vec![(1, 50)],
    }
}

async fn run_manual(State(rt): State<AppState>, body: Option<Json<RunBody>>) -> ApiResult {
    require_operator(&rt)?;
    let body = body.map(|j| j.0).unwrap_or_default();
    let src = match body.fixture.as_deref() {
        Some(name) => fixture_source(name)?,
        None => MemorySource {
            source_id: "manual".into(),
            cases: vec![],
        },
    };
    let cfg = &rt.config.introspection;
    let opts = RunOnceOptions::from_config(cfg);
    let wm = rt
        .store
        .introspection_latest_watermark()?
        .unwrap_or_else(|| "{}".into());
    let outcome = run_once(RunOnceRequest {
        store: &rt.store,
        source: &src,
        cfg,
        trigger: "manual",
        watermark_json: &wm,
        input_hash: body.fixture.as_deref().unwrap_or("manual"),
        analysis_now: 1000,
        opts,
        bus: Some(rt.bus.as_ref()),
    })
    .await
    .map_err(AppError::from)?;
    Ok(Json(match outcome {
        RunOnceOutcome::Skipped { reason } => json!({ "status": "skipped", "reason": reason }),
        RunOnceOutcome::Done { job_id } => json!({ "status": "done", "job_id": job_id }),
        RunOnceOutcome::Failed { job_id, error } => {
            json!({ "status": "failed", "job_id": job_id, "error": error })
        }
        RunOnceOutcome::Cancelled { job_id } => {
            json!({ "status": "cancelled", "job_id": job_id })
        }
    }))
}

async fn unread(State(rt): State<AppState>) -> ApiResult {
    require_operator(&rt)?;
    let unread = rt.store.introspection_unread_count()?;
    let conversation_id = rt.store.main_conversation_id()?;
    Ok(Json(json!({
        "unread": unread,
        "conversation_id": conversation_id,
        "title": MAIN_CONVERSATION_TITLE,
    })))
}

async fn mark_read(State(rt): State<AppState>) -> ApiResult {
    require_operator(&rt)?;
    let n = rt.store.introspection_mark_all_read()?;
    Ok(Json(json!({ "ok": true, "marked": n, "unread": 0 })))
}
