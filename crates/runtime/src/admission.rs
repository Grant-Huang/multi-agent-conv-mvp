//! Agent 准入与持续信任（E3）：论文 §5.2 七步协议的确定性核心。
//!
//! 七步：S1 提交 Agent Card → S2 澄清 → S3 能力测试 → S4 加权打分 → S5 契约 →
//! S6 人工审批 → S7 激活并进入试用期。
//!
//! 设计要点：
//! - **模型只做理解，程序负责决策。** S2 的澄清问题这里只生成确定性的那部分（Card 自相矛盾），
//!   S3 的分数由外部评估器给出；等级、抽样、警告、降级、撤权全部由本模块的规则决定。
//! - **凭证代际 `epoch`。** 激活时为 1；降级或撤权时加 1，旧凭证立即失效。编排器在接受结果前
//!   用 [`AdmissionLedger::credential_valid`] 核对（接入见 E3 第 2 步）。
//! - **配置文件里写的 agent 视为运维已信任**，用 [`AdmissionLedger::preadmit`] 记为 Trusted，不走协议。
//! - 每次变更先在同一事务里写入记录和审计事件，成功后才更新内存，保证两者一致。
//!
//! 本模块不调用模型、不做网络 IO；持久化通过 [`Store`] 完成。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use an_core::{codes, now, Risk};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::store::Store;

/// Agent Card 的六个能力维度（布尔声明）
pub const CAPABILITY_DIMS: [&str; 6] = [
    "read_context",
    "write_artifacts",
    "call_tools",
    "spawn_subtasks",
    "dispatch_actions",
    "human_contact",
];

/// 运行时接入的外部 agent 的 `AgentCard.kind`。其他 kind 视为配置文件里写的 agent（运维已信任）。
pub const A2A_KIND: &str = "a2a";

/// 契约中允许的角色
const CONTRACT_ROLES: [&str; 3] = ["observer", "executor", "coordinator"];

/// 信任等级（有序：Guest < Member < Trusted）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    Guest,
    Member,
    Trusted,
}

impl TrustLevel {
    fn as_str(self) -> &'static str {
        match self {
            TrustLevel::Guest => "guest",
            TrustLevel::Member => "member",
            TrustLevel::Trusted => "trusted",
        }
    }

    /// 该等级允许执行的最高风险
    fn max_risk(self) -> Risk {
        match self {
            TrustLevel::Guest => Risk::Low,
            TrustLevel::Member => Risk::Medium,
            TrustLevel::Trusted => Risk::High,
        }
    }
}

/// 协议阶段。S6 不是独立阶段：处于 Contracted 且需要审批时，等待 `approve`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// S1 已提交 Card，等待回答澄清问题
    Submitted,
    /// S2 已澄清，等待测试分数
    Clarified,
    /// S4 已打分，等待契约
    Scored,
    /// S5 已签契约，等待（可能需要的）审批与激活
    Contracted,
    /// S7 已激活（含试用期、降级后的 Guest）
    Active,
    /// 已撤权，任何凭证都无效
    Revoked,
    /// 准入被拒，可以重新提交
    Rejected,
}

/// S1：外部 agent 的自我声明
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentClaim {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    /// 六个能力维度的布尔声明，键必须在 [`CAPABILITY_DIMS`] 内
    pub capabilities: BTreeMap<String, bool>,
    pub requested_level: TrustLevel,
    /// 声明可派生子任务时必须给出
    pub max_subtask_depth: Option<u32>,
    pub working_language: Option<String>,
}

/// S5：协作契约
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// observer / executor / coordinator
    pub role: String,
    pub working_language: String,
    /// 允许访问的上下文范围
    pub context_scope: Vec<String>,
    /// 允许调用的工具
    pub allowed_tools: Vec<String>,
}

/// S3 评估器给出的三项分数，各在 0–1 之间
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub completeness: f64,
    pub conversation: f64,
    pub test: f64,
}

/// S4 的打分权重（论文默认 30/20/50）
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub completeness: f64,
    pub conversation: f64,
    pub test: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clarification {
    pub question: String,
    pub answer: String,
}

/// S7 试用期计数
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Probation {
    pub tasks_completed: u64,
    /// 已回写的抽检次数
    pub sampled: u64,
    pub failed_checks: u64,
    pub consecutive_failures: u32,
    pub warnings: u32,
    /// 已因警告降级，等待人工复核
    pub pending_review: bool,
    /// 被抽中、等待回写结论的任务
    #[serde(default)]
    pub pending_checks: Vec<String>,
}

/// 外部 agent 的接入信息（激活时提供；服务重启后据此重新注册）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct A2aBinding {
    /// 外部 agent 服务地址（http/https）
    pub endpoint: String,
    /// 执行角色：general / coding / reviewer
    pub role: String,
    /// 同类型的 agent 才能放进同一个执行组
    pub agent_type: String,
    pub max_concurrent: u32,
}

/// 一个 agent 的准入记录
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdmissionRecord {
    pub agent_id: String,
    /// "protocol"（走七步协议）或 "config"（配置文件里写的，预准入）
    pub source: String,
    pub stage: Stage,
    pub claim: Option<AgentClaim>,
    pub clarifications: Vec<Clarification>,
    pub scores: Option<Scores>,
    /// S4 总分（0–100）
    pub total: Option<f64>,
    pub recommended: Option<TrustLevel>,
    pub granted: Option<TrustLevel>,
    pub contract: Option<Contract>,
    pub approved_by: Option<String>,
    /// 凭证代际；激活前为 0
    pub epoch: u64,
    pub probation: Probation,
    pub created_at: String,
    pub updated_at: String,
    /// 外部 agent 的接入信息（配置 agent 为空）
    #[serde(default)]
    pub binding: Option<A2aBinding>,
}

/// 回写一次抽检的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// 只记录，没有触发警告
    Recorded,
    /// 触发一次警告
    Warning,
    /// 警告达到上限，降为 Guest 并作废旧凭证
    Downgraded,
}

/// 准入规则的参数（默认值取自论文 §5.2）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdmissionConfig {
    pub weights: Weights,
    /// 总分达到此值推荐 Member
    pub member_score: f64,
    /// 总分达到此值推荐 Trusted
    pub trusted_score: f64,
    /// 试用期抽检比例
    pub sample_rate: f64,
    /// 连续几次不合格记一次警告
    pub failures_per_warning: u32,
    /// 累计几次警告降级
    pub warnings_to_downgrade: u32,
    /// Member 是否也必须人工审批（Trusted 总是必须）
    pub member_requires_approval: bool,
    /// 抽样种子
    pub seed: u64,
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            weights: Weights {
                completeness: 0.3,
                conversation: 0.2,
                test: 0.5,
            },
            member_score: 60.0,
            trusted_score: 80.0,
            sample_rate: 0.2,
            failures_per_warning: 2,
            warnings_to_downgrade: 3,
            member_requires_approval: false,
            seed: 0,
        }
    }
}

impl AdmissionConfig {
    fn validate(&self) -> Result<()> {
        let w = self.weights;
        let ws = [w.completeness, w.conversation, w.test];
        if ws.iter().any(|x| !x.is_finite() || *x < 0.0) || ws.iter().sum::<f64>() <= 0.0 {
            return Err(fail(
                codes::ADMISSION_INVALID,
                "打分权重必须非负且总和大于 0",
            ));
        }
        if !(0.0..=1.0).contains(&self.sample_rate) {
            return Err(fail(codes::ADMISSION_INVALID, "抽检比例必须在 0–1 之间"));
        }
        if !(0.0..=100.0).contains(&self.member_score)
            || !(0.0..=100.0).contains(&self.trusted_score)
            || self.member_score > self.trusted_score
        {
            return Err(fail(
                codes::ADMISSION_INVALID,
                "等级阈值必须在 0–100 之间，且 Member 阈值不高于 Trusted",
            ));
        }
        if self.failures_per_warning == 0 || self.warnings_to_downgrade == 0 {
            return Err(fail(codes::ADMISSION_INVALID, "警告与降级阈值必须大于 0"));
        }
        Ok(())
    }
}

fn fail(code: &str, msg: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("[{code}] {msg}")
}

/// agent_id 只允许 ID 形态的短字符串，避免自然语言或指令文本进入日志与他人上下文
fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '@' | '-'))
}

/// S2 中能确定性发现的问题：Card 自相矛盾或缺项
fn clarification_questions(c: &AgentClaim) -> Vec<String> {
    let has = |d: &str| c.capabilities.get(d).copied().unwrap_or(false);
    let mut q = vec![];
    if c.description.trim().is_empty() {
        q.push("未描述能力范围，请说明能处理哪类任务".to_string());
    }
    if has("spawn_subtasks") && c.max_subtask_depth.is_none() {
        q.push("声明可派生子任务（spawn_subtasks），但没有给出最大子任务深度".to_string());
    }
    if c.working_language
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        q.push("未声明工作语言".to_string());
    }
    if c.requested_level >= TrustLevel::Member && !c.capabilities.values().any(|v| *v) {
        q.push(format!(
            "申请 {} 等级，但没有声明任何能力",
            c.requested_level.as_str()
        ));
    }
    if has("dispatch_actions") && c.requested_level == TrustLevel::Guest {
        q.push("声明可下发动作，但申请的是 Guest；Guest 只能执行低风险任务，请确认".to_string());
    }
    q
}

/// FNV-1a 64 位哈希：确定性、跨平台一致，不依赖标准库哈希的随机种子
fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in *p {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // 分隔符，避免 ("ab","c") 与 ("a","bc") 相同
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// splitmix64 的收尾混合：每一位输入都影响每一位输出
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// 一次变更产生的审计事件：(事件名, 详情)
type Events = Vec<(&'static str, Value)>;

/// 抽检计数规则：连续不合格达到阈值记一次警告；警告达到上限时降为 Guest、
/// 标记待人工复核，并作废旧凭证（epoch 加 1）。调用方已确认处于 Active。
fn apply_check(
    rec: &mut AdmissionRecord,
    ev: &mut Events,
    cfg: &AdmissionConfig,
    passed: bool,
    task_id: Option<&str>,
) -> CheckOutcome {
    let p = &mut rec.probation;
    p.sampled += 1;
    ev.push((
        "admission.check",
        json!({"passed": passed, "task_id": task_id}),
    ));
    if passed {
        p.consecutive_failures = 0;
        return CheckOutcome::Recorded;
    }
    p.failed_checks += 1;
    p.consecutive_failures += 1;
    if p.consecutive_failures < cfg.failures_per_warning {
        return CheckOutcome::Recorded;
    }
    p.consecutive_failures = 0;
    p.warnings += 1;
    let warnings = p.warnings;
    ev.push(("admission.warning", json!({"warnings": warnings})));
    if warnings >= cfg.warnings_to_downgrade && !rec.probation.pending_review {
        rec.granted = Some(TrustLevel::Guest);
        rec.probation.pending_review = true;
        rec.epoch += 1;
        ev.push((
            "admission.downgraded",
            json!({"granted": "guest", "epoch": rec.epoch}),
        ));
        return CheckOutcome::Downgraded;
    }
    CheckOutcome::Warning
}

/// 准入账本：内存中的记录 + SQLite 持久化
pub struct AdmissionLedger {
    store: Arc<Store>,
    cfg: AdmissionConfig,
    records: Mutex<BTreeMap<String, AdmissionRecord>>,
}

impl AdmissionLedger {
    /// 打开账本并加载已保存的记录
    pub fn open(store: Arc<Store>, cfg: AdmissionConfig) -> Result<Self> {
        cfg.validate()?;
        let mut records = BTreeMap::new();
        for raw in store.list_admissions()? {
            let rec: AdmissionRecord =
                serde_json::from_str(&raw).context("准入记录无法解析，数据库可能已损坏")?;
            records.insert(rec.agent_id.clone(), rec);
        }
        Ok(Self {
            store,
            cfg,
            records: Mutex::new(records),
        })
    }

    pub fn config(&self) -> &AdmissionConfig {
        &self.cfg
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, AdmissionRecord>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 先落库（记录 + 审计事件同一事务），成功后才写回内存
    fn persist(
        &self,
        map: &mut BTreeMap<String, AdmissionRecord>,
        mut rec: AdmissionRecord,
        events: Events,
    ) -> Result<()> {
        rec.updated_at = now();
        let evs: Vec<(String, String)> = events
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.store
            .save_admission(&rec.agent_id, &serde_json::to_string(&rec)?, &evs)?;
        for (kind, detail) in &events {
            tracing::info!(event = *kind, agent_id = %rec.agent_id, detail = %detail, "准入状态变化");
        }
        map.insert(rec.agent_id.clone(), rec);
        Ok(())
    }

    /// 对已有记录做一次变更。`f` 在副本上修改并返回事件；出错时内存与数据库都不变。
    fn update<T>(
        &self,
        agent_id: &str,
        f: impl FnOnce(&mut AdmissionRecord, &mut Events) -> Result<T>,
    ) -> Result<T> {
        let mut map = self.lock();
        let mut rec = map
            .get(agent_id)
            .cloned()
            .ok_or_else(|| fail(codes::ADMISSION_STATE, format!("未知 agent：{agent_id}")))?;
        let mut events = vec![];
        let out = f(&mut rec, &mut events)?;
        self.persist(&mut map, rec, events)?;
        Ok(out)
    }

    fn expect_stage(rec: &AdmissionRecord, want: Stage, step: &str) -> Result<()> {
        if rec.stage != want {
            return Err(fail(
                codes::ADMISSION_STATE,
                format!(
                    "{step} 需要阶段 {want:?}，当前为 {:?}（agent {}）",
                    rec.stage, rec.agent_id
                ),
            ));
        }
        Ok(())
    }

    /// 配置文件里写的 agent：直接记为 Trusted 并激活。已有记录时原样返回（幂等）。
    pub fn preadmit(&self, agent_id: &str) -> Result<AdmissionRecord> {
        if !valid_agent_id(agent_id) {
            return Err(fail(codes::ADMISSION_INVALID, "agent_id 格式不合法"));
        }
        let mut map = self.lock();
        if let Some(r) = map.get(agent_id) {
            return Ok(r.clone());
        }
        let at = now();
        let rec = AdmissionRecord {
            agent_id: agent_id.into(),
            source: "config".into(),
            stage: Stage::Active,
            claim: None,
            clarifications: vec![],
            scores: None,
            total: None,
            recommended: Some(TrustLevel::Trusted),
            granted: Some(TrustLevel::Trusted),
            contract: None,
            approved_by: None,
            epoch: 1,
            probation: Probation::default(),
            created_at: at.clone(),
            updated_at: at,
            binding: None,
        };
        self.persist(
            &mut map,
            rec.clone(),
            vec![(
                "admission.preadmitted",
                json!({"epoch": 1, "granted": "trusted"}),
            )],
        )?;
        Ok(map[agent_id].clone())
    }

    /// S1：提交 Card。返回需要回答的澄清问题（可能为空）。
    pub fn submit(&self, claim: AgentClaim) -> Result<Vec<String>> {
        if !valid_agent_id(&claim.agent_id) {
            return Err(fail(
                codes::ADMISSION_INVALID,
                "agent_id 只能包含字母、数字和 _ . : @ -，长度 1–64",
            ));
        }
        if let Some(k) = claim
            .capabilities
            .keys()
            .find(|k| !CAPABILITY_DIMS.contains(&k.as_str()))
        {
            return Err(fail(codes::ADMISSION_INVALID, format!("未知能力维度：{k}")));
        }
        let mut map = self.lock();
        if let Some(r) = map.get(&claim.agent_id) {
            if r.stage != Stage::Rejected {
                return Err(fail(
                    codes::ADMISSION_STATE,
                    format!("agent {} 已存在（阶段 {:?}）", claim.agent_id, r.stage),
                ));
            }
        }
        let questions = clarification_questions(&claim);
        let at = now();
        let rec = AdmissionRecord {
            agent_id: claim.agent_id.clone(),
            source: "protocol".into(),
            stage: Stage::Submitted,
            clarifications: questions
                .iter()
                .map(|q| Clarification {
                    question: q.clone(),
                    answer: String::new(),
                })
                .collect(),
            scores: None,
            total: None,
            recommended: None,
            granted: None,
            contract: None,
            approved_by: None,
            epoch: 0,
            probation: Probation::default(),
            created_at: at.clone(),
            updated_at: at,
            claim: Some(claim.clone()),
            binding: None,
        };
        self.persist(
            &mut map,
            rec,
            vec![(
                "admission.submitted",
                json!({"requested": claim.requested_level, "questions": questions.len()}),
            )],
        )?;
        Ok(questions)
    }

    /// S2：回答澄清问题。回答数必须与问题数一致，且不能为空。
    pub fn clarify(&self, agent_id: &str, answers: Vec<String>) -> Result<()> {
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Submitted, "S2 澄清")?;
            if answers.len() != rec.clarifications.len() {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    format!(
                        "有 {} 个澄清问题，收到 {} 个回答",
                        rec.clarifications.len(),
                        answers.len()
                    ),
                ));
            }
            if answers.iter().any(|a| a.trim().is_empty()) {
                return Err(fail(codes::ADMISSION_INVALID, "澄清回答不能为空"));
            }
            for (c, a) in rec.clarifications.iter_mut().zip(answers) {
                c.answer = a;
            }
            rec.stage = Stage::Clarified;
            ev.push((
                "admission.clarified",
                json!({"answered": rec.clarifications.len()}),
            ));
            Ok(())
        })
    }

    /// S3 + S4：记录评估器分数，按权重算出 0–100 总分和推荐等级（不超过申请等级）。
    pub fn score(&self, agent_id: &str, s: Scores) -> Result<(f64, TrustLevel)> {
        let cfg = self.cfg.clone();
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Clarified, "S4 打分")?;
            let parts = [s.completeness, s.conversation, s.test];
            if parts
                .iter()
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            {
                return Err(fail(codes::ADMISSION_INVALID, "三项分数都必须在 0–1 之间"));
            }
            let w = cfg.weights;
            let sum = w.completeness + w.conversation + w.test;
            let total = 100.0
                * (w.completeness * s.completeness
                    + w.conversation * s.conversation
                    + w.test * s.test)
                / sum;
            let by_score = if total >= cfg.trusted_score {
                TrustLevel::Trusted
            } else if total >= cfg.member_score {
                TrustLevel::Member
            } else {
                TrustLevel::Guest
            };
            let requested = rec
                .claim
                .as_ref()
                .map(|c| c.requested_level)
                .unwrap_or(TrustLevel::Guest);
            let level = by_score.min(requested);
            rec.scores = Some(s);
            rec.total = Some(total);
            rec.recommended = Some(level);
            rec.stage = Stage::Scored;
            ev.push((
                "admission.scored",
                json!({"total": total, "recommended": level, "scores": s}),
            ));
            Ok((total, level))
        })
    }

    /// S5：签协作契约，授予等级 = 推荐等级。
    pub fn contract(&self, agent_id: &str, c: Contract) -> Result<()> {
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Scored, "S5 契约")?;
            if !CONTRACT_ROLES.contains(&c.role.as_str()) {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    format!(
                        "契约角色只能是 observer / executor / coordinator，收到 {}",
                        c.role
                    ),
                ));
            }
            if c.working_language.trim().is_empty() {
                return Err(fail(codes::ADMISSION_INVALID, "契约必须写明工作语言"));
            }
            rec.granted = rec.recommended;
            ev.push((
                "admission.contracted",
                json!({"role": c.role, "granted": rec.granted}),
            ));
            rec.contract = Some(c);
            rec.stage = Stage::Contracted;
            Ok(())
        })
    }

    /// S6：人工审批（Trusted 必须；Member 视配置）
    pub fn approve(&self, agent_id: &str, approver: &str) -> Result<()> {
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Contracted, "S6 审批")?;
            if approver.trim().is_empty() {
                return Err(fail(codes::ADMISSION_INVALID, "审批人不能为空"));
            }
            rec.approved_by = Some(approver.to_string());
            ev.push(("admission.approved", json!({"approver": approver})));
            Ok(())
        })
    }

    fn needs_approval(&self, rec: &AdmissionRecord) -> bool {
        match rec.granted {
            Some(TrustLevel::Trusted) => true,
            Some(TrustLevel::Member) => self.cfg.member_requires_approval,
            _ => false,
        }
    }

    /// S7：激活并签发凭证代际，返回 epoch（首次为 1）
    pub fn activate(&self, agent_id: &str) -> Result<u64> {
        let needs = {
            let map = self.lock();
            map.get(agent_id).map(|r| self.needs_approval(r))
        };
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Contracted, "S7 激活")?;
            if needs.unwrap_or(true) && rec.approved_by.is_none() {
                return Err(fail(
                    codes::ADMISSION_STATE,
                    format!(
                        "{:?} 等级需要人工审批后才能激活",
                        rec.granted.unwrap_or(TrustLevel::Guest)
                    ),
                ));
            }
            rec.epoch += 1;
            rec.stage = Stage::Active;
            ev.push((
                "admission.activated",
                json!({"epoch": rec.epoch, "granted": rec.granted}),
            ));
            Ok(rec.epoch)
        })
    }

    /// 记录外部 agent 的接入信息（S5 签约后、激活前或激活后均可更新）
    pub fn set_binding(&self, agent_id: &str, b: A2aBinding) -> Result<()> {
        self.update(agent_id, |rec, ev| {
            if !matches!(rec.stage, Stage::Contracted | Stage::Active) {
                return Err(fail(
                    codes::ADMISSION_STATE,
                    format!("签约后才能登记接入信息，当前阶段 {:?}", rec.stage),
                ));
            }
            if !(b.endpoint.starts_with("http://") || b.endpoint.starts_with("https://")) {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    "接入地址必须以 http:// 或 https:// 开头",
                ));
            }
            if !matches!(b.role.as_str(), "general" | "coding" | "reviewer") {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    "角色只能是 general / coding / reviewer",
                ));
            }
            if !(1..=16).contains(&b.max_concurrent) {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    "max_concurrent 必须在 1–16 之间",
                ));
            }
            if !valid_agent_id(&b.agent_type) {
                return Err(fail(codes::ADMISSION_INVALID, "agent_type 格式不合法"));
            }
            ev.push((
                "admission.bound",
                json!({"endpoint": b.endpoint, "role": b.role}),
            ));
            rec.binding = Some(b);
            Ok(())
        })
    }

    /// 拒绝准入（例如澄清不通过、分数不足且不愿以 Guest 加入）。被拒后可以重新提交。
    pub fn reject(&self, agent_id: &str, reason: &str) -> Result<()> {
        self.update(agent_id, |rec, ev| {
            if matches!(rec.stage, Stage::Active | Stage::Revoked) {
                return Err(fail(
                    codes::ADMISSION_STATE,
                    "已激活的 agent 只能撤权，不能拒绝",
                ));
            }
            rec.stage = Stage::Rejected;
            ev.push(("admission.rejected", json!({"reason": reason})));
            Ok(())
        })
    }

    /// 试用期抽检：同一 (seed, agent, task) 结果固定，抽中比例约为 `sample_rate`。不改状态。
    pub fn should_sample(&self, agent_id: &str, task_id: &str) -> bool {
        let h = fnv1a(&[
            &self.cfg.seed.to_le_bytes(),
            agent_id.as_bytes(),
            task_id.as_bytes(),
        ]);
        // FNV 的高位对末尾字节变化不敏感（编号连续时会成片抽中或成片漏掉），
        // 先用 splitmix64 的收尾步骤把各位充分混合，再取高 53 位映射到 [0, 1)
        let u = (mix64(h) >> 11) as f64 / (1u64 << 53) as f64;
        u < self.cfg.sample_rate
    }

    /// 回写一次抽检结论。连续不合格达到阈值记一次警告；警告达到上限时降为 Guest、
    /// 标记待人工复核，并作废旧凭证（epoch 加 1）。
    pub fn record_check(&self, agent_id: &str, passed: bool) -> Result<CheckOutcome> {
        let cfg = self.cfg.clone();
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Active, "回写抽检")?;
            Ok(apply_check(rec, ev, &cfg, passed, None))
        })
    }

    /// 外部 agent 完成一个任务（结果已被接受）：累加完成数；被抽中时放进待检清单。
    /// 只对走协议准入的 agent 记录；配置 agent 或未激活的 agent 直接忽略。
    pub fn note_completed(&self, agent_id: &str, task_id: &str, sampled: bool) -> Result<()> {
        {
            let map = self.lock();
            match map.get(agent_id) {
                Some(r) if r.source == "protocol" && r.stage == Stage::Active => {}
                _ => return Ok(()),
            }
        }
        self.update(agent_id, |rec, ev| {
            rec.probation.tasks_completed += 1;
            if sampled && !rec.probation.pending_checks.iter().any(|t| t == task_id) {
                rec.probation.pending_checks.push(task_id.to_string());
                ev.push(("admission.sampled", json!({"task_id": task_id})));
            }
            Ok(())
        })
    }

    /// 回写某个被抽中任务的抽检结论。任务必须在待检清单里，回写后移出（不能重复回写）。
    pub fn record_check_for(
        &self,
        agent_id: &str,
        task_id: &str,
        passed: bool,
    ) -> Result<CheckOutcome> {
        let cfg = self.cfg.clone();
        self.update(agent_id, |rec, ev| {
            Self::expect_stage(rec, Stage::Active, "回写抽检")?;
            let Some(pos) = rec
                .probation
                .pending_checks
                .iter()
                .position(|t| t == task_id)
            else {
                return Err(fail(
                    codes::ADMISSION_INVALID,
                    format!("任务 {task_id} 不在 {agent_id} 的待检清单里"),
                ));
            };
            rec.probation.pending_checks.remove(pos);
            Ok(apply_check(rec, ev, &cfg, passed, Some(task_id)))
        })
    }

    /// 撤权：阶段改为 Revoked，epoch 加 1，返回新的 epoch。之后任何凭证都无效。
    pub fn revoke(&self, agent_id: &str, reason: &str) -> Result<u64> {
        self.update(agent_id, |rec, ev| {
            if rec.stage == Stage::Revoked {
                return Err(fail(codes::ADMISSION_STATE, "该 agent 已被撤权"));
            }
            rec.stage = Stage::Revoked;
            rec.epoch += 1;
            ev.push((
                "admission.revoked",
                json!({"reason": reason, "epoch": rec.epoch}),
            ));
            Ok(rec.epoch)
        })
    }

    /// 凭证是否仍然有效：agent 处于激活状态且 epoch 与当前一致
    pub fn credential_valid(&self, agent_id: &str, epoch: u64) -> bool {
        self.lock()
            .get(agent_id)
            .is_some_and(|r| r.stage == Stage::Active && r.epoch == epoch)
    }

    /// 当前等级是否允许执行该风险的任务。未知、未激活或已撤权的 agent 一律不允许。
    pub fn permits(&self, agent_id: &str, risk: Risk) -> bool {
        self.lock().get(agent_id).is_some_and(|r| {
            r.stage == Stage::Active && r.granted.is_some_and(|g| risk <= g.max_risk())
        })
    }

    pub fn get(&self, agent_id: &str) -> Option<AdmissionRecord> {
        self.lock().get(agent_id).cloned()
    }

    pub fn list(&self) -> Vec<AdmissionRecord> {
        self.lock().values().cloned().collect()
    }

    /// 某个 agent 的审计事件（按发生顺序）
    pub fn events(&self, agent_id: &str, limit: usize) -> Result<Vec<Value>> {
        self.store.admission_events(agent_id, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_separates_parts() {
        assert_ne!(fnv1a(&[b"ab", b"c"]), fnv1a(&[b"a", b"bc"]));
    }

    #[test]
    fn invalid_config_is_rejected() {
        let bad = AdmissionConfig {
            sample_rate: 1.5,
            ..AdmissionConfig::default()
        };
        assert!(bad.validate().is_err());
        let bad = AdmissionConfig {
            member_score: 90.0,
            trusted_score: 80.0,
            ..AdmissionConfig::default()
        };
        assert!(bad.validate().is_err());
        assert!(AdmissionConfig::default().validate().is_ok());
    }
}
