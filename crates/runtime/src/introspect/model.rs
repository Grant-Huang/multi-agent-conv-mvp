//! 业务内省的存储行类型（只描述库里的行，不含业务逻辑）。
//!
//! 见 `docs/memory/INTROSPECTION_PLAN.md` 第 4 节。

/// 一次内省运行（定时或手动）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntrospectionJob {
    pub id: String,
    /// `schedule` / `manual`
    pub trigger: String,
    /// `running` / `done` / `failed` / `cancelled` / `skipped`
    pub state: String,
    /// 各来源读到的位置（JSON 字符串，由调用方解释）
    pub watermark_json: String,
    pub input_hash: String,
    /// 租约到期时间；结束后清空
    pub lease_until: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

/// 一条内省结论。`id` 是稳定 ID（由主题、范围、输入摘要算出），重复写入不会产生第二行。
#[derive(Debug, Clone, PartialEq)]
pub struct IntrospectionFinding {
    pub id: String,
    pub job_id: String,
    pub topic: String,
    /// 数值、证据引用、假设、反证、验真状态（JSON 字符串）
    pub json: String,
    /// `supported` / `inconclusive` / `contradicted`
    pub verification: String,
    pub gate_score: Option<f64>,
    /// 推送或不推送的原因（每条都记）
    pub gate_reason: String,
    pub pushed: bool,
    /// 运营负责人的判断：`confirmed` / `rejected`
    pub verdict: Option<String>,
    pub verdict_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 运营负责人在运行时给结论打的标注（如“该推 / 不该推”）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntrospectionLabel {
    pub id: String,
    pub finding_id: String,
    pub kind: String,
    pub note: String,
    pub created_at: String,
}
