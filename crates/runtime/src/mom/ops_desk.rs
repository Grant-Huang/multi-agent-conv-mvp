//! 运营频道发布器与事件分级（docs/mom/OPS_DESK.md，DECISIONS 第 56–58 条）。
//!
//! 发布器是 MOM 事件发件箱的一个消费者：用自己的游标读新事件，按分级决定是否发到固定会话
//! `ops-channel`。它自己的状态（游标、案例到消息的对应、小时配额、没发的原因）放在单独的 SQLite 文件里，
//! 不改主库表结构，也不碰引擎的状态。发卡片只是往会话里写一条带卡片的消息：不调用模型，不创建任务。
//!
//! 超时、取消与清理：本模块没有子进程和长时间异步操作；`tick` 是一次同步的短事务，`now_secs`
//! 由调用方传入，便于重放。调用方负责定期调用，进程退出时没有需要清理的资源。

use super::{Case, Engine};
use crate::store::Store;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::Arc};

/// 运营频道的固定会话 id
pub const OPS_CHANNEL_ID: &str = "ops-channel";
const OPS_CHANNEL_TITLE: &str = "运营频道";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpsLevel {
    /// 过程数据：只进桌面，不进频道
    P2,
    /// 案例状态有实质变化：已有卡片才更新
    P1,
    /// 需要人决定或有安全、质量、超期风险：发卡片
    P0,
}

impl OpsLevel {
    fn as_str(self) -> &'static str {
        match self {
            OpsLevel::P0 => "P0",
            OpsLevel::P1 => "P1",
            OpsLevel::P2 => "P2",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "P0" => OpsLevel::P0,
            "P1" => OpsLevel::P1,
            _ => OpsLevel::P2,
        }
    }
}

const P0_KINDS: &[&str] = &[
    "SafetyBlocked",
    "SlaBreached",
    "CompensationRequired",
    "BudgetExhausted",
];
const P2_KINDS: &[&str] = &[
    "ObservationRecorded",
    "VerificationObservationRecorded",
    "WorldChanged",
    "RuntimeStateChanged",
    "ToolStarted",
    "SimulationConfigured",
    "ChildStarted",
];
fn needs_human(stage: &str) -> bool {
    matches!(stage, "WAITING_HUMAN" | "BLOCKED")
}

/// 事件分级：只看事件种类和案例当前阶段，不调用模型，同输入恒得同输出。
pub fn classify(kind: &str, stage: &str) -> OpsLevel {
    if P0_KINDS.contains(&kind) {
        OpsLevel::P0
    } else if P2_KINDS.contains(&kind) {
        OpsLevel::P2
    } else if needs_human(stage) {
        OpsLevel::P0
    } else {
        OpsLevel::P1
    }
}

#[derive(Debug, Clone)]
pub struct OpsConfig {
    /// 同一案例卡片更新的合并窗口（秒）
    pub merge_secs: u64,
    /// 每小时最多新发几张卡
    pub hourly_cap: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub posted: u32,
    pub updated: u32,
    pub summarized: u32,
    pub skipped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipRecord {
    pub case_id: String,
    pub seq: u64,
    pub reason: String,
}

pub struct OpsPublisher {
    db: Connection,
    store: Arc<Store>,
    cfg: OpsConfig,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ops_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS ops_cases(
    case_id TEXT PRIMARY KEY,
    message_id TEXT,
    last_posted_at INTEGER NOT NULL DEFAULT 0,
    dirty INTEGER NOT NULL DEFAULT 0,
    level TEXT NOT NULL DEFAULT 'P2',
    summarized INTEGER NOT NULL DEFAULT 0,
    last_seq INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS ops_posts(ts INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ops_skips(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    reason TEXT NOT NULL,
    at INTEGER NOT NULL
);";

impl OpsPublisher {
    pub fn memory(store: Arc<Store>, cfg: OpsConfig) -> Result<Self> {
        Self::from_db(Connection::open_in_memory()?, store, cfg)
    }
    pub fn open(path: &Path, store: Arc<Store>, cfg: OpsConfig) -> Result<Self> {
        let db = Connection::open(path)
            .with_context(|| format!("无法打开运营频道状态库 {}", path.display()))?;
        Self::from_db(db, store, cfg)
    }
    fn from_db(db: Connection, store: Arc<Store>, cfg: OpsConfig) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db, store, cfg })
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM ops_meta WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO ops_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
    fn skip(&self, case_id: &str, seq: u64, reason: &str, now: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO ops_skips(case_id,seq,reason,at) VALUES(?1,?2,?3,?4)",
            params![case_id, seq, reason, now],
        )?;
        Ok(())
    }

    /// 消费引擎发件箱里的新事件，把需要的案例发到运营频道或更新原卡片。
    /// `now_secs` 由调用方给，保证可重放。
    pub fn tick(&mut self, engine: &mut Engine, now_secs: u64) -> Result<TickReport> {
        let mut report = TickReport::default();
        self.store
            .ensure_conversation(OPS_CHANNEL_ID, OPS_CHANNEL_TITLE)?;
        self.consume(engine, now_secs, &mut report)?;
        self.publish_dirty(engine, now_secs, &mut report)?;
        Ok(report)
    }

    /// 读新事件，按案例归并后标记“待发布”；纯过程事件只记原因
    fn consume(&mut self, engine: &Engine, now: u64, report: &mut TickReport) -> Result<()> {
        loop {
            let cursor: u64 = self
                .meta("cursor")?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let events = engine.pending_events(cursor, 500)?;
            if events.is_empty() {
                return Ok(());
            }
            let mut batch: Vec<(String, OpsLevel, u64)> = vec![];
            for ev in &events {
                // 运行时内部事件不属于任何案例
                let Ok(case) = engine.case(&ev.case_id) else {
                    continue;
                };
                let level = classify(&ev.kind, &case.stage);
                match batch.iter_mut().find(|(id, _, _)| id == &ev.case_id) {
                    Some(entry) => {
                        entry.1 = entry.1.max(level);
                        entry.2 = ev.sequence;
                    }
                    None => batch.push((ev.case_id.clone(), level, ev.sequence)),
                }
            }
            let last = events.last().map(|e| e.sequence).unwrap_or(cursor);
            for (case_id, level, seq) in batch {
                if level == OpsLevel::P2 {
                    self.skip(&case_id, seq, "level_p2", now)?;
                    report.skipped += 1;
                    continue;
                }
                self.db.execute(
                    "INSERT INTO ops_cases(case_id,dirty,level,last_seq) VALUES(?1,1,?2,?3)
                     ON CONFLICT(case_id) DO UPDATE SET dirty=1, last_seq=excluded.last_seq,
                       level=CASE WHEN excluded.level='P0' OR level='P0' THEN 'P0' ELSE 'P1' END",
                    params![case_id, level.as_str(), seq],
                )?;
            }
            self.set_meta("cursor", &last.to_string())?;
        }
    }

    fn posts_in_last_hour(&self, now: u64) -> Result<u32> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM ops_posts WHERE ts>?1",
            [now.saturating_sub(3600)],
            |r| r.get::<_, u32>(0),
        )?)
    }

    fn publish_dirty(&mut self, engine: &Engine, now: u64, report: &mut TickReport) -> Result<()> {
        let dirty: Vec<(String, Option<String>, u64, String, u64)> = {
            let mut st = self.db.prepare(
                "SELECT case_id,message_id,last_posted_at,level,last_seq FROM ops_cases WHERE dirty=1
                 ORDER BY CASE level WHEN 'P0' THEN 0 ELSE 1 END, last_seq, case_id",
            )?;
            let rows = st.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut newly_summarized: Vec<String> = vec![];
        for (case_id, message_id, last_posted_at, level, seq) in dirty {
            let case = engine.case(&case_id)?;
            match message_id {
                Some(mid) => {
                    // 已有卡片：窗口内合并，窗口后更新原卡，不新增消息
                    if now < last_posted_at + self.cfg.merge_secs {
                        self.skip(&case_id, seq, "merge_window", now)?;
                        report.skipped += 1;
                        continue;
                    }
                    self.store
                        .update_message_card(&mid, &card_for(case, OpsLevel::parse(&level)))?;
                    self.db.execute(
                        "UPDATE ops_cases SET dirty=0,last_posted_at=?2 WHERE case_id=?1",
                        params![case_id, now],
                    )?;
                    report.updated += 1;
                }
                None => {
                    if OpsLevel::parse(&level) != OpsLevel::P0 && !needs_human(&case.stage) {
                        // P1：没有卡片的案例不新发
                        self.skip(&case_id, seq, "p1_no_card", now)?;
                        self.db
                            .execute("UPDATE ops_cases SET dirty=0 WHERE case_id=?1", [&case_id])?;
                        report.skipped += 1;
                        continue;
                    }
                    if self.posts_in_last_hour(now)? >= self.cfg.hourly_cap {
                        self.skip(&case_id, seq, "hourly_cap", now)?;
                        self.db.execute(
                            "UPDATE ops_cases SET dirty=0,summarized=1 WHERE case_id=?1",
                            [&case_id],
                        )?;
                        newly_summarized.push(case_id);
                        report.summarized += 1;
                        continue;
                    }
                    let card = card_for(case, OpsLevel::parse(&level));
                    let content = format!("[模拟] 运营案例 {case_id}（详情见卡片）");
                    let msg = self.store.add_message_with_card(
                        OPS_CHANNEL_ID,
                        "assistant",
                        &content,
                        &card,
                    )?;
                    self.db.execute(
                        "UPDATE ops_cases SET message_id=?2,dirty=0,last_posted_at=?3,summarized=0 WHERE case_id=?1",
                        params![case_id, msg.id, now],
                    )?;
                    self.db
                        .execute("INSERT INTO ops_posts(ts) VALUES(?1)", [now])?;
                    report.posted += 1;
                }
            }
        }
        if !newly_summarized.is_empty() {
            self.update_summary(newly_summarized)?;
        }
        Ok(())
    }

    /// 汇总卡只有一张：第一次创建，之后累加案例并更新原卡
    fn update_summary(&mut self, added: Vec<String>) -> Result<()> {
        let mut ids: Vec<String> = self
            .meta("summary_cases")?
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default();
        ids.extend(added);
        let card = an_core::ChatCard::OpsSummary {
            count: ids.len() as u32,
            case_ids: ids.clone(),
        };
        match self.meta("summary_message")? {
            Some(mid) => self.store.update_message_card(&mid, &card)?,
            None => {
                let msg = self.store.add_message_with_card(
                    OPS_CHANNEL_ID,
                    "assistant",
                    "[模拟] 另有案例未单独发卡（超过每小时上限），全部见运营桌面",
                    &card,
                )?;
                self.set_meta("summary_message", &msg.id)?;
            }
        }
        self.set_meta("summary_cases", &serde_json::to_string(&ids)?)?;
        Ok(())
    }

    /// 运营频道里用户还没看过的新消息数（新卡、汇总卡各算一条；就地更新原卡不算）
    pub fn unread(&self) -> Result<u32> {
        let total = self.store.list_message_views(OPS_CHANNEL_ID, None)?.len() as u32;
        let read: u32 = self
            .meta("read_count")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok(total.saturating_sub(read))
    }
    /// 用户打开了运营频道：当前所有消息记为已读
    pub fn mark_read(&mut self) -> Result<()> {
        let total = self.store.list_message_views(OPS_CHANNEL_ID, None)?.len();
        self.set_meta("read_count", &total.to_string())
    }
    /// 每次“没发”的判断及原因
    pub fn skip_log(&self) -> Result<Vec<SkipRecord>> {
        let mut st = self
            .db
            .prepare("SELECT case_id,seq,reason FROM ops_skips ORDER BY id")?;
        let rows = st.query_map([], |r| {
            Ok(SkipRecord {
                case_id: r.get(0)?,
                seq: r.get(1)?,
                reason: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

pub(crate) fn stage_label(stage: &str) -> &'static str {
    match stage {
        "WAITING_HUMAN" => "待批准",
        "BLOCKED" => "受阻",
        "VERIFYING" => "核验中",
        "CLOSED" => "已关闭",
        "CANCELLED" => "已取消",
        "BUDGET_EXHAUSTED" => "预算耗尽",
        "INVESTIGATING" => "调查中",
        _ => "评估中",
    }
}

/// 与运营卡同一标题；桌面读模型复用，避免两处各写一份。
pub(crate) fn case_title(case: &Case) -> String {
    let what = if case.scenario == "s2" {
        "客户投诉"
    } else {
        "产线多扰动"
    };
    format!("{what}：{}", case.asset)
}

fn card_for(case: &Case, level: OpsLevel) -> an_core::ChatCard {
    let level = if needs_human(&case.stage) {
        OpsLevel::P0
    } else {
        level
    };
    let approver = case.approvals.values().next().map(|a| a.approver.clone());
    an_core::ChatCard::OpsCase {
        case_id: case.id.clone(),
        title: case_title(case),
        level: level.as_str().into(),
        stage: case.stage.clone(),
        summary: format!(
            "{}，约束：{}",
            stage_label(&case.stage),
            case.constraints.join("、")
        ),
        owner: "调度".into(),
        approver,
        provenance: "simulation".into(),
        updated_at: case.updated_at,
    }
}

impl OpsPublisher {
    /// 把案例卡发到运营频道（用户在桌面点“在频道中处理”）。已有卡就返回已有消息 id，不重复发。
    /// 绕过小时上限与合并窗口（是用户主动动作），但仍写入 ops_cases 记录，之后的 tick 照常就地更新这张卡。
    /// 案例不存在返回 Err，错误信息以 "E_CASE_NOT_FOUND" 开头。
    pub fn open_case(&self, engine: &Engine, case_id: &str, now_secs: u64) -> Result<String> {
        let case = engine
            .case(case_id)
            .map_err(|_| anyhow::anyhow!("E_CASE_NOT_FOUND: case {case_id} does not exist"))?;
        self.store
            .ensure_conversation(OPS_CHANNEL_ID, OPS_CHANNEL_TITLE)?;
        let existing: Option<String> = self
            .db
            .query_row(
                "SELECT message_id FROM ops_cases WHERE case_id=?1",
                [case_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        if let Some(mid) = existing {
            return Ok(mid);
        }
        let card = card_for(case, OpsLevel::P0);
        let content = format!("[模拟] 运营案例 {case_id}（详情见卡片）");
        let msg = self
            .store
            .add_message_with_card(OPS_CHANNEL_ID, "assistant", &content, &card)?;
        self.db.execute(
            "INSERT INTO ops_cases(case_id,message_id,last_posted_at,dirty,level,summarized,last_seq)
             VALUES(?1,?2,?3,0,'P0',0,0)
             ON CONFLICT(case_id) DO UPDATE SET
               message_id=excluded.message_id,
               last_posted_at=excluded.last_posted_at,
               dirty=0,
               level='P0',
               summarized=0",
            params![case_id, msg.id, now_secs],
        )?;
        Ok(msg.id)
    }
}
