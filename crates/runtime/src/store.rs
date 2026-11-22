//! SQLite 持久化：会话、消息、任务、事件、路由日志、agent 准入记录。
//!
//! 任务状态由 AgentNexus 自己保存（而不是放在 Harness 内存里），
//! 服务重启后仍能看到任务历史。
//!
//! 事件写入走有界通道和专用写线程，攒到一批再提交，避免每条进度都抢锁。
//! SSE 广播不等这条落库。文件库另开一条 `query_only` 连接读，WAL 下读不堵写。
//! `:memory:` 每个连接是独立库，测试库仍用同一条连接，`insert_event` 也保持同步。

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_core::{
    now, Assignment, Conversation, Message, PolicyVerdict, ReviewVerdict, RouteLog, TaskEvent,
    TaskRecord, TaskResult, TaskStatus,
};
use anyhow::{anyhow, Context, Result};

use crate::introspect::model::{IntrospectionFinding, IntrospectionJob, IntrospectionLabel};
use rusqlite::{params, Connection, OptionalExtension, Row};

/// 写线程一次最多提交多少条
const EVENT_BATCH: usize = 64;
/// 不足一批时，最多再等这么久就提交
const EVENT_FLUSH_WAIT: Duration = Duration::from_millis(20);
/// 通道满了之后，发布方会堵住（广播已经发出，不会打乱顺序，也不会丢掉）
const EVENT_QUEUE: usize = 1024;
/// `events` 只保留最新的这么多行，多出来的在每批提交时删掉
const EVENT_KEEP: i64 = 20_000;
/// 读超过这个时间记一条 `store.read_slow`
const READ_SLOW_MS: u128 = 50;

/// 尚未结束的任务状态（取消时允许的起点）。NEEDS_HUMAN 不是终态：人工审批后还能继续
pub const ACTIVE_STATUSES: [TaskStatus; 5] = [
    TaskStatus::Created,
    TaskStatus::Queued,
    TaskStatus::Running,
    TaskStatus::Verifying,
    TaskStatus::NeedsHuman,
];

enum WriteCmd {
    Event(TaskEvent),
    /// 把已经进通道的事件写完，再通知等待方
    Flush(std::sync::mpsc::Sender<()>),
    Stop,
}

struct Shared {
    write: Mutex<Connection>,
    /// 文件库的只读连接。内存库是 None，读写共用 `write`
    read: Option<Mutex<Connection>>,
    cmd: std::sync::mpsc::SyncSender<WriteCmd>,
    /// 已入队但还没提交的事件数
    pending: AtomicUsize,
}

pub struct Store {
    shared: Arc<Shared>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// AI Desktop B1：可靠受理记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopRequestRecord {
    pub request_id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub client_operation_id: String,
    pub text: String,
    pub scope: String,
    pub task_id: Option<String>,
    pub status: String,
    pub dispatch_count: u32,
}

/// `accept_desktop_request` 结果
#[derive(Debug, Clone)]
pub enum DesktopAcceptOutcome {
    /// 首次受理并写入消息
    Created(DesktopRequestRecord),
    /// 同操作 ID + 同正文：回放原记录，不二次派发
    Replay(DesktopRequestRecord),
    /// 同操作 ID + 改正文
    Conflict {
        request_id: String,
        message_id: String,
    },
}

/// AI Desktop B2：会话事件游标回放条目（`cursor` = 稳定 event id）
#[derive(Debug, Clone)]
pub struct DesktopEventCursor {
    pub cursor: String,
    pub event: TaskEvent,
}

/// `list_conversation_events_after` 结果
#[derive(Debug, Clone)]
pub enum DesktopEventsAfter {
    Ok(Vec<DesktopEventCursor>),
    /// `after` 不存在或不属于该会话（含已淘汰）
    CursorExpired,
}

/// AI Desktop B3：会话↔任务关联
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopTaskLink {
    pub id: String,
    pub client_operation_id: String,
    pub conversation_id: String,
    pub task_id: String,
    pub source_message_ids: Vec<String>,
    pub created_by: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    /// 由 `TaskRecord.conversation_id` 投影，非表内行
    #[serde(default)]
    pub projected: bool,
}

#[derive(Debug, Clone)]
pub enum DesktopLinkOutcome {
    Created(DesktopTaskLink),
    Replay(DesktopTaskLink),
    Conflict {
        link_id: String,
        conversation_id: String,
    },
}

/// AI Desktop B3：个人配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopPreferences {
    pub user_id: String,
    pub version: u32,
    pub name: String,
    pub avatar: String,
    pub role: String,
    pub soul: String,
    pub taste: String,
    pub memory_principle: String,
    pub proactivity: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub enum DesktopPrefPatchOutcome {
    Ok(DesktopPreferences),
    Conflict(DesktopPreferences),
}

/// AI Desktop C1：证据记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopEvidence {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub object_label: String,
    pub collected_at: String,
    pub source_type: String,
    pub version: String,
    /// ok | failed | expired | revoked
    pub status: String,
    pub summary: String,
    pub source_body: String,
    pub error_message: String,
}

#[derive(Debug, Clone)]
pub enum DesktopEvidenceAccess {
    Allowed(DesktopEvidence),
    Forbidden,
    SourceUnavailable(DesktopEvidence),
    NotFound,
}

/// AI Desktop C2：任务动作审批（副作用计数在 store，不写厂）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopApproval {
    pub id: String,
    pub task_id: String,
    pub plan_revision: i64,
    pub params: String,
    /// pending | approved | rejected | expired
    pub status: String,
    pub decided_by: Option<String>,
    pub execute_count: i64,
    /// none | applied | unknown | failed
    pub last_receipt: String,
}

#[derive(Debug, Clone)]
pub enum DesktopApprovalDecideOutcome {
    Ok(DesktopApproval),
    RevisionConflict(DesktopApproval),
    NotFound,
}

#[derive(Debug, Clone)]
pub enum DesktopApprovalExecuteOutcome {
    Ok(DesktopApproval),
    RevisionConflict(DesktopApproval),
    Forbidden(DesktopApproval),
    NotFound,
}

/// AI Desktop C2：Attention 条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopAttentionItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub created_at: String,
}

/// AI Desktop D：语音会话（fixture 媒体，无真实 mic）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopVoiceSession {
    pub id: String,
    pub task_id: String,
    /// prompt | granted | denied
    pub permission: String,
    /// idle | connecting | listening | speaking | muted | offline
    pub phase: String,
    pub playing: bool,
    pub partial: String,
    pub final_text: String,
    pub linked_task_id: Option<String>,
    pub share_transcript: bool,
}

/// AI Desktop D：跨端续接
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopHandoff {
    pub task_id: String,
    pub conversation_id: String,
    pub run_id: String,
    pub read_message_id: String,
    pub phone_text: String,
    pub phone_version: i64,
    pub desktop_text: String,
    pub desktop_version: i64,
    pub conflict: bool,
    pub chosen: Option<String>,
    pub voice_shared: bool,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    task_id TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    goal TEXT NOT NULL,
    role TEXT NOT NULL,
    status TEXT NOT NULL,
    route_json TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    result_json TEXT,
    review_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '',
    assignment_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_conv ON tasks(conversation_id, created_at);
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    task_id TEXT,
    json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_task ON events(task_id, created_at);
CREATE TABLE IF NOT EXISTS routing_feedback (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    original_action TEXT NOT NULL,
    original_role TEXT,
    correct_action TEXT NOT NULL,
    correct_role TEXT,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_feedback_message ON routing_feedback(message_id, created_at);
CREATE TABLE IF NOT EXISTS nodes (
    node_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    token TEXT NOT NULL,
    cards_json TEXT NOT NULL,
    paired_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS route_logs (
    id TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
-- agent 准入（E3）：只新建表，不改已有表
CREATE TABLE IF NOT EXISTS agent_admission (
    agent_id TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS admission_events (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_admission_events_agent ON admission_events(agent_id, created_at);
-- 业务内省（INT）：只新建表，不改已有表；见 docs/memory/INTROSPECTION_PLAN.md 第 4 节
CREATE TABLE IF NOT EXISTS introspection_jobs (
    id TEXT PRIMARY KEY,
    trigger TEXT NOT NULL,
    state TEXT NOT NULL,
    watermark_json TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    lease_until TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    error TEXT
);
CREATE TABLE IF NOT EXISTS introspection_findings (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    topic TEXT NOT NULL,
    json TEXT NOT NULL,
    verification TEXT NOT NULL,
    gate_score REAL,
    gate_reason TEXT NOT NULL,
    pushed INTEGER NOT NULL DEFAULT 0,
    verdict TEXT,
    verdict_note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_findings_topic ON introspection_findings(topic, created_at);
CREATE TABLE IF NOT EXISTS introspection_pushes (
    id TEXT PRIMARY KEY,
    finding_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    pushed_at TEXT NOT NULL,
    read_at TEXT
);
CREATE TABLE IF NOT EXISTS introspection_labels (
    id TEXT PRIMARY KEY,
    finding_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- AI Desktop B1：可靠受理幂等（client_operation_id）；只新建表
CREATE TABLE IF NOT EXISTS desktop_client_operations (
    client_operation_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    text TEXT NOT NULL,
    scope TEXT NOT NULL,
    task_id TEXT,
    status TEXT NOT NULL,
    dispatch_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_desktop_ops_request ON desktop_client_operations(request_id);
-- AI Desktop B3：会话↔任务多对多关联（可撤回）
CREATE TABLE IF NOT EXISTS desktop_conversation_task_links (
    id TEXT PRIMARY KEY,
    client_operation_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    source_message_ids_json TEXT NOT NULL DEFAULT '[]',
    created_by TEXT NOT NULL DEFAULT 'local',
    created_at TEXT NOT NULL,
    revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_desktop_links_task ON desktop_conversation_task_links(task_id);
CREATE INDEX IF NOT EXISTS idx_desktop_links_conv ON desktop_conversation_task_links(conversation_id);
-- AI Desktop B3：个人配置（单用户占位 local + 乐观版本）
CREATE TABLE IF NOT EXISTS desktop_preferences (
    user_id TEXT PRIMARY KEY,
    version INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
-- AI Desktop C1：证据与主体授权（撤权写 revoked_at）
CREATE TABLE IF NOT EXISTS desktop_evidence (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    title TEXT NOT NULL,
    object_label TEXT NOT NULL,
    collected_at TEXT NOT NULL,
    source_type TEXT NOT NULL,
    version TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT NOT NULL,
    source_body TEXT NOT NULL,
    error_message TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_desktop_evidence_task ON desktop_evidence(task_id);
CREATE TABLE IF NOT EXISTS desktop_evidence_grants (
    evidence_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (evidence_id, principal_id)
);
-- AI Desktop C2：动作审批（每任务一行当前方案）
CREATE TABLE IF NOT EXISTS desktop_approvals (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE,
    plan_revision INTEGER NOT NULL,
    params TEXT NOT NULL,
    status TEXT NOT NULL,
    decided_by TEXT,
    execute_count INTEGER NOT NULL DEFAULT 0,
    last_receipt TEXT NOT NULL DEFAULT 'none',
    updated_at TEXT NOT NULL
);
-- AI Desktop C2：Attention 队列与静音偏好
CREATE TABLE IF NOT EXISTS desktop_attention_items (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    task_id TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS desktop_attention_prefs (
    user_id TEXT PRIMARY KEY,
    mute_ordinary INTEGER NOT NULL DEFAULT 0
);
-- AI Desktop D：语音会话与跨端草稿（fixture）
CREATE TABLE IF NOT EXISTS desktop_voice_sessions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE,
    permission TEXT NOT NULL,
    phase TEXT NOT NULL,
    playing INTEGER NOT NULL DEFAULT 0,
    partial TEXT NOT NULL DEFAULT '',
    final_text TEXT NOT NULL DEFAULT '',
    linked_task_id TEXT,
    share_transcript INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS desktop_handoff (
    task_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    read_message_id TEXT NOT NULL,
    phone_text TEXT NOT NULL DEFAULT '',
    phone_version INTEGER NOT NULL DEFAULT 0,
    desktop_text TEXT NOT NULL DEFAULT '',
    desktop_version INTEGER NOT NULL DEFAULT 0,
    conflict INTEGER NOT NULL DEFAULT 0,
    chosen TEXT,
    voice_shared INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
"#;

fn tune(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
    )?;
    Ok(())
}

/// 删掉最旧的事件，只留 `keep` 行。不足 `keep` 行时一条都不删。
fn trim_events(conn: &Connection, keep: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM events WHERE rowid <= COALESCE((SELECT rowid FROM events ORDER BY rowid DESC LIMIT 1 OFFSET ?1), -1)",
        [keep],
    )?;
    Ok(())
}

fn write_events(conn: &mut Connection, batch: &[TaskEvent]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut st = tx.prepare("INSERT INTO events (id, conversation_id, task_id, json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for e in batch {
            st.execute(params![
                e.id,
                e.conversation_id,
                e.task_id,
                serde_json::to_string(e)?,
                e.created_at
            ])?;
        }
    }
    trim_events(&tx, EVENT_KEEP)?;
    tx.commit()?;
    Ok(())
}

fn writer_loop(shared: Arc<Shared>, rx: std::sync::mpsc::Receiver<WriteCmd>) {
    let mut batch = Vec::new();
    let mut acks = Vec::new();
    loop {
        let cmd = if batch.is_empty() {
            match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => return,
            }
        } else {
            match rx.recv_timeout(EVENT_FLUSH_WAIT) {
                Ok(cmd) => cmd,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    commit(&shared, &mut batch, &mut acks);
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    commit(&shared, &mut batch, &mut acks);
                    return;
                }
            }
        };
        if !take_cmd(&mut batch, &mut acks, cmd) {
            commit(&shared, &mut batch, &mut acks);
            return;
        }
        while batch.len() < EVENT_BATCH {
            match rx.try_recv() {
                Ok(cmd) => {
                    if !take_cmd(&mut batch, &mut acks, cmd) {
                        commit(&shared, &mut batch, &mut acks);
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        if batch.len() >= EVENT_BATCH || !acks.is_empty() {
            commit(&shared, &mut batch, &mut acks);
        }
    }
}

/// 返回 false 表示写线程该停了（停之前调用方会先把缓冲区提交掉）
fn take_cmd(
    batch: &mut Vec<TaskEvent>,
    acks: &mut Vec<std::sync::mpsc::Sender<()>>,
    cmd: WriteCmd,
) -> bool {
    match cmd {
        WriteCmd::Event(event) => batch.push(event),
        WriteCmd::Flush(ack) => acks.push(ack),
        WriteCmd::Stop => return false,
    }
    true
}

fn commit(
    shared: &Shared,
    batch: &mut Vec<TaskEvent>,
    acks: &mut Vec<std::sync::mpsc::Sender<()>>,
) {
    if !batch.is_empty() {
        let started = Instant::now();
        let size = batch.len();
        let mut conn = lock(&shared.write);
        if let Err(e) = write_events(&mut conn, batch) {
            tracing::error!(event = "store.event_batch", error = %format!("{e:#}"), size, "事件批量写库失败");
        }
        drop(conn);
        let flush_ms = started.elapsed().as_millis() as u64;
        let left = shared
            .pending
            .fetch_sub(size, Ordering::AcqRel)
            .saturating_sub(size);
        tracing::info!(
            event = "store.event_batch",
            size,
            flush_ms,
            queue_len = left,
            "事件批量落库"
        );
        batch.clear();
    }
    for ack in acks.drain(..) {
        let _ = ack.send(());
    }
}

fn lock(mutex: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    // 锁中毒说明之前有线程 panic，直接继续使用（数据由 SQLite 保证一致）
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

struct ReadTimer {
    op: &'static str,
    started: Instant,
}

impl ReadTimer {
    fn new(op: &'static str) -> Self {
        Self {
            op,
            started: Instant::now(),
        }
    }
}

impl Drop for ReadTimer {
    fn drop(&mut self) {
        let ms = self.started.elapsed().as_millis();
        if ms > READ_SLOW_MS {
            tracing::warn!(
                event = "store.read_slow",
                op = self.op,
                ms,
                code = an_core::codes::STORE_READ_SLOW,
                "读数据库偏慢"
            );
        }
    }
}

impl Store {
    /// 打开（或创建）数据库文件；传入 `:memory:` 用于测试
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("无法打开数据库 {}", path.display()))?;
        tune(&conn)?;
        conn.execute_batch(SCHEMA)?;
        // 兼容旧库：tasks 表补 context 列（新库建表时已包含）
        let has_context: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'context'",
            [],
            |r| r.get(0),
        )?;
        if has_context == 0 {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN context TEXT NOT NULL DEFAULT ''")?;
        }
        // 兼容旧库：tasks 表补 assignment_json 列（执行者；旧任务为 NULL）
        let has_assignment: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'assignment_json'",
            [],
            |r| r.get(0),
        )?;
        if has_assignment == 0 {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN assignment_json TEXT")?;
        }
        // 兼容旧库：messages 表补 card_json 列（AG.4 对话里的卡片；旧消息为 NULL）
        let has_card: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'card_json'",
            [],
            |r| r.get(0),
        )?;
        if has_card == 0 {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN card_json TEXT")?;
        }
        // 内存库不能跨连接共享，读写仍走同一条。文件库另开只读连接，写的时候读不用等。
        let read = if path == Path::new(":memory:") {
            None
        } else {
            let read = Connection::open(path)
                .with_context(|| format!("无法打开数据库只读连接 {}", path.display()))?;
            tune(&read)?;
            read.execute_batch("PRAGMA query_only=ON;")?;
            Some(Mutex::new(read))
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(EVENT_QUEUE);
        let shared = Arc::new(Shared {
            write: Mutex::new(conn),
            read,
            cmd: tx,
            pending: AtomicUsize::new(0),
        });
        let background = Arc::clone(&shared);
        let writer = std::thread::Builder::new()
            .name("store-writer".into())
            .spawn(move || writer_loop(background, rx))
            .context("无法启动事件写线程")?;
        Ok(Self {
            shared,
            writer: Mutex::new(Some(writer)),
        })
    }

    fn write(&self) -> std::sync::MutexGuard<'_, Connection> {
        lock(&self.shared.write)
    }

    fn read(&self) -> std::sync::MutexGuard<'_, Connection> {
        match &self.shared.read {
            Some(read) => lock(read),
            None => self.write(),
        }
    }

    /// 交给写线程。通道满时会等到有空位，不丢事件、不插队。
    pub fn enqueue_event(&self, event: TaskEvent) {
        self.shared.pending.fetch_add(1, Ordering::AcqRel);
        if self.shared.cmd.send(WriteCmd::Event(event)).is_err() {
            self.shared.pending.fetch_sub(1, Ordering::AcqRel);
            tracing::error!(
                event = "store.event_batch",
                "事件写线程已退出，这条事件没有落库"
            );
        }
    }

    /// 等到此前入队的事件都写进库。任务详情读取前调用，避免时间线缺刚刚发生的状态变化。
    pub fn flush_events(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.shared.cmd.send(WriteCmd::Flush(tx)).is_err() {
            return;
        }
        if rx.recv_timeout(Duration::from_secs(5)).is_err() {
            tracing::warn!(event = "store.event_batch", "等待事件落库超过 5 秒");
        }
    }

    // ───────────── 会话 ─────────────

    pub fn create_conversation(&self, title: &str) -> Result<Conversation> {
        let c = Conversation {
            id: an_core::new_id("conv"),
            title: title.into(),
            created_at: now(),
        };
        self.write().execute(
            "INSERT INTO conversations (id, title, created_at) VALUES (?1, ?2, ?3)",
            params![c.id, c.title, c.created_at],
        )?;
        Ok(c)
    }

    /// 确保指定 id 的会话存在（运营频道用固定 id）：已存在就原样返回，不改标题。不改表结构。
    pub fn ensure_conversation(&self, id: &str, title: &str) -> Result<Conversation> {
        self.write().execute(
            "INSERT OR IGNORE INTO conversations (id, title, created_at) VALUES (?1, ?2, ?3)",
            params![id, title, now()],
        )?;
        self.get_conversation(id)?
            .with_context(|| format!("会话 {id} 创建后读不到"))
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let _timer = ReadTimer::new("get_conversation");
        Ok(self
            .read()
            .query_row(
                "SELECT id, title, created_at FROM conversations WHERE id = ?1",
                [id],
                |r| {
                    Ok(Conversation {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>> {
        let _timer = ReadTimer::new("list_conversations");
        let conn = self.read();
        let mut st = conn
            .prepare("SELECT id, title, created_at FROM conversations ORDER BY created_at DESC")?;
        let rows = st.query_map([], |r| {
            Ok(Conversation {
                id: r.get(0)?,
                title: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<()> {
        self.write().execute(
            "UPDATE conversations SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    // ───────────── 消息 ─────────────

    pub fn add_message(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        task_id: Option<&str>,
    ) -> Result<Message> {
        self.add_message_with_id(
            &an_core::new_id("msg"),
            conversation_id,
            role,
            content,
            task_id,
        )
    }

    /// 同上，但使用指定的 id（流式回复：界面先用这个 id 显示片段，最终消息用同一个 id）
    pub fn add_message_with_id(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content: &str,
        task_id: Option<&str>,
    ) -> Result<Message> {
        let m = Message {
            id: id.into(),
            conversation_id: conversation_id.into(),
            role: role.into(),
            content: content.into(),
            task_id: task_id.map(String::from),
            created_at: now(),
        };
        self.write().execute(
            "INSERT INTO messages (id, conversation_id, role, content, task_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![m.id, m.conversation_id, m.role, m.content, m.task_id, m.created_at],
        )?;
        Ok(m)
    }

    /// 写一条带卡片的消息（AG.4）。`content` 是给人和模型看的大白话；卡片数据只存在 card_json，不进模型上下文
    pub fn add_message_with_card(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        card: &an_core::ChatCard,
    ) -> Result<Message> {
        let m = Message {
            id: an_core::new_id("msg"),
            conversation_id: conversation_id.into(),
            role: role.into(),
            content: content.into(),
            task_id: None,
            created_at: now(),
        };
        self.write().execute(
            "INSERT INTO messages (id, conversation_id, role, content, task_id, created_at, card_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![m.id, m.conversation_id, m.role, m.content, m.task_id, m.created_at, serde_json::to_string(card)?],
        )?;
        Ok(m)
    }

    /// 更新某条消息的卡片（AG.4：确认卡变为已确认 / 已过期）
    pub fn update_message_card(&self, message_id: &str, card: &an_core::ChatCard) -> Result<()> {
        self.write().execute(
            "UPDATE messages SET card_json = ?2 WHERE id = ?1",
            params![message_id, serde_json::to_string(card)?],
        )?;
        Ok(())
    }

    /// 读一条消息的卡片
    pub fn message_card(&self, message_id: &str) -> Result<Option<an_core::ChatCard>> {
        let conn = self.read();
        let json: Option<Option<String>> = conn
            .query_row(
                "SELECT card_json FROM messages WHERE id = ?1",
                params![message_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.flatten().and_then(|j| serde_json::from_str(&j).ok()))
    }

    /// 消息 + 卡片（消息列表接口用）。卡片读不出来时当作没有卡片，不影响消息本身
    pub fn list_message_views(
        &self,
        conversation_id: &str,
        last: Option<usize>,
    ) -> Result<Vec<an_core::MessageView>> {
        let _timer = ReadTimer::new("list_message_views");
        let conn = self.read();
        let limit = last.map(|n| n as i64).unwrap_or(-1);
        let mut st = conn.prepare(
            "SELECT id, conversation_id, role, content, task_id, created_at, card_json FROM messages
             WHERE conversation_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![conversation_id, limit], |r| {
            let card: Option<String> = r.get(6)?;
            Ok(an_core::MessageView {
                message: Message {
                    id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    task_id: r.get(4)?,
                    created_at: r.get(5)?,
                },
                card: card.and_then(|j| serde_json::from_str(&j).ok()),
            })
        })?;
        let mut v: Vec<an_core::MessageView> = rows.collect::<rusqlite::Result<_>>()?;
        v.reverse();
        Ok(v)
    }

    /// AI Desktop B1：可靠受理结果（同 client_operation_id 幂等 / 冲突）
    pub fn accept_desktop_request(
        &self,
        conversation_id: &str,
        client_operation_id: &str,
        text: &str,
        scope: &str,
        task_id: Option<&str>,
    ) -> Result<DesktopAcceptOutcome> {
        let text = text.trim();
        if text.is_empty() {
            return Err(anyhow!("消息不能为空"));
        }
        if client_operation_id.trim().is_empty() {
            return Err(anyhow!("client_operation_id 不能为空"));
        }
        let conn = self.write();
        let tx = conn.unchecked_transaction()?;
        let existing: Option<(String, String, String, i64, String)> = tx
            .query_row(
                "SELECT request_id, message_id, text, dispatch_count, status
                 FROM desktop_client_operations WHERE client_operation_id = ?1",
                params![client_operation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        if let Some((request_id, message_id, committed, dispatch_count, status)) = existing {
            if committed == text {
                tx.commit()?;
                return Ok(DesktopAcceptOutcome::Replay(DesktopRequestRecord {
                    request_id,
                    message_id,
                    conversation_id: conversation_id.into(),
                    client_operation_id: client_operation_id.into(),
                    text: committed,
                    scope: scope.into(),
                    task_id: task_id.map(String::from),
                    status,
                    dispatch_count: dispatch_count as u32,
                }));
            }
            tx.commit()?;
            return Ok(DesktopAcceptOutcome::Conflict {
                request_id,
                message_id,
            });
        }
        let request_id = an_core::new_id("dreq");
        let message_id = an_core::new_id("msg");
        let created_at = now();
        tx.execute(
            "INSERT INTO messages (id, conversation_id, role, content, task_id, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4, ?5)",
            params![message_id, conversation_id, text, task_id, created_at],
        )?;
        tx.execute(
            "INSERT INTO desktop_client_operations
             (client_operation_id, request_id, conversation_id, message_id, text, scope, task_id, status, dispatch_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'accepted', 1, ?8)",
            params![
                client_operation_id,
                request_id,
                conversation_id,
                message_id,
                text,
                scope,
                task_id,
                created_at
            ],
        )?;
        tx.commit()?;
        Ok(DesktopAcceptOutcome::Created(DesktopRequestRecord {
            request_id,
            message_id,
            conversation_id: conversation_id.into(),
            client_operation_id: client_operation_id.into(),
            text: text.into(),
            scope: scope.into(),
            task_id: task_id.map(String::from),
            status: "accepted".into(),
            dispatch_count: 1,
        }))
    }

    /// 按 request_id 读取可靠受理记录
    pub fn get_desktop_request(&self, request_id: &str) -> Result<Option<DesktopRequestRecord>> {
        let _timer = ReadTimer::new("get_desktop_request");
        let conn = self.read();
        conn.query_row(
            "SELECT request_id, message_id, conversation_id, client_operation_id, text, scope, task_id, status, dispatch_count
             FROM desktop_client_operations WHERE request_id = ?1",
            params![request_id],
            |r| {
                Ok(DesktopRequestRecord {
                    request_id: r.get(0)?,
                    message_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    client_operation_id: r.get(3)?,
                    text: r.get(4)?,
                    scope: r.get(5)?,
                    task_id: r.get(6)?,
                    status: r.get(7)?,
                    dispatch_count: r.get::<_, i64>(8)? as u32,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// AI Desktop B3：建立会话↔任务关联（client_operation_id 幂等）
    pub fn create_desktop_task_link(
        &self,
        task_id: &str,
        conversation_id: &str,
        client_operation_id: &str,
        source_message_ids: &[String],
        created_by: &str,
    ) -> Result<DesktopLinkOutcome> {
        if client_operation_id.trim().is_empty() {
            return Err(anyhow!("client_operation_id 不能为空"));
        }
        if conversation_id.trim().is_empty() {
            return Err(anyhow!("conversation_id 不能为空"));
        }
        if self.get_task(task_id)?.is_none() {
            return Err(anyhow!("任务不存在"));
        }
        if self.get_conversation(conversation_id)?.is_none() {
            return Err(anyhow!("会话不存在"));
        }
        let ids_json = serde_json::to_string(source_message_ids)?;
        let conn = self.write();
        let tx = conn.unchecked_transaction()?;
        let existing: Option<(String, String, String, Option<String>)> = tx
            .query_row(
                "SELECT id, conversation_id, task_id, revoked_at
                 FROM desktop_conversation_task_links WHERE client_operation_id = ?1",
                params![client_operation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((id, committed_conv, committed_task, revoked_at)) = existing {
            if revoked_at.is_none()
                && committed_conv == conversation_id
                && committed_task == task_id
            {
                let link = Self::load_link_row(&tx, &id)?;
                tx.commit()?;
                return Ok(DesktopLinkOutcome::Replay(link));
            }
            tx.commit()?;
            return Ok(DesktopLinkOutcome::Conflict {
                link_id: id,
                conversation_id: committed_conv,
            });
        }
        let id = an_core::new_id("dlink");
        let created_at = now();
        tx.execute(
            "INSERT INTO desktop_conversation_task_links
             (id, client_operation_id, conversation_id, task_id, source_message_ids_json, created_by, created_at, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![
                id,
                client_operation_id,
                conversation_id,
                task_id,
                ids_json,
                created_by,
                created_at
            ],
        )?;
        let link = Self::load_link_row(&tx, &id)?;
        tx.commit()?;
        Ok(DesktopLinkOutcome::Created(link))
    }

    /// 撤回关联（幂等）
    pub fn revoke_desktop_task_link(
        &self,
        task_id: &str,
        link_id: &str,
    ) -> Result<Option<DesktopTaskLink>> {
        let conn = self.write();
        let tx = conn.unchecked_transaction()?;
        let row: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT task_id, revoked_at FROM desktop_conversation_task_links WHERE id = ?1",
                params![link_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((owned_task, revoked_at)) = row else {
            tx.commit()?;
            return Ok(None);
        };
        if owned_task != task_id {
            tx.commit()?;
            return Ok(None);
        }
        if revoked_at.is_none() {
            tx.execute(
                "UPDATE desktop_conversation_task_links SET revoked_at = ?2 WHERE id = ?1",
                params![link_id, now()],
            )?;
        }
        let link = Self::load_link_row(&tx, link_id)?;
        tx.commit()?;
        Ok(Some(link))
    }

    /// 列举任务关联；默认不含已撤回。合并 `TaskRecord.conversation_id` 投影。
    pub fn list_desktop_task_links(
        &self,
        task_id: &str,
        include_revoked: bool,
    ) -> Result<Vec<DesktopTaskLink>> {
        let _timer = ReadTimer::new("list_desktop_task_links");
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(vec![]),
        };
        let conn = self.read();
        let mut st = if include_revoked {
            conn.prepare(
                "SELECT id FROM desktop_conversation_task_links WHERE task_id = ?1
                 ORDER BY created_at, rowid",
            )?
        } else {
            conn.prepare(
                "SELECT id FROM desktop_conversation_task_links WHERE task_id = ?1 AND revoked_at IS NULL
                 ORDER BY created_at, rowid",
            )?
        };
        let ids: Vec<String> = st
            .query_map(params![task_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        let mut out = Vec::new();
        for id in ids {
            out.push(Self::load_link_row(&conn, &id)?);
        }
        let has_owner = out
            .iter()
            .any(|l| l.revoked_at.is_none() && l.conversation_id == task.conversation_id);
        if !has_owner && !task.conversation_id.is_empty() {
            out.insert(
                0,
                DesktopTaskLink {
                    id: format!("legacy-{task_id}"),
                    client_operation_id: format!("legacy-op-{task_id}"),
                    conversation_id: task.conversation_id,
                    task_id: task_id.into(),
                    source_message_ids: vec![],
                    created_by: "legacy".into(),
                    created_at: task.created_at,
                    revoked_at: None,
                    projected: true,
                },
            );
        }
        Ok(out)
    }

    fn load_link_row(conn: &Connection, id: &str) -> Result<DesktopTaskLink> {
        conn.query_row(
            "SELECT id, client_operation_id, conversation_id, task_id, source_message_ids_json,
                    created_by, created_at, revoked_at
             FROM desktop_conversation_task_links WHERE id = ?1",
            params![id],
            |r| {
                let raw: String = r.get(4)?;
                let source_message_ids: Vec<String> =
                    serde_json::from_str(&raw).unwrap_or_default();
                Ok(DesktopTaskLink {
                    id: r.get(0)?,
                    client_operation_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    task_id: r.get(3)?,
                    source_message_ids,
                    created_by: r.get(5)?,
                    created_at: r.get(6)?,
                    revoked_at: r.get(7)?,
                    projected: false,
                })
            },
        )
        .map_err(Into::into)
    }

    fn default_desktop_preferences(user_id: &str) -> DesktopPreferences {
        DesktopPreferences {
            user_id: user_id.into(),
            version: 1,
            name: "我的助理".into(),
            avatar: "◇".into(),
            role: "日常工作伙伴".into(),
            soul: "简洁、可核对、不替你做未授权决定".into(),
            taste: "优先事实与可执行下一步".into(),
            memory_principle: "仅保留你确认过的长期偏好".into(),
            proactivity: "克制：重要事项可提醒，不抢焦点".into(),
            updated_at: now(),
        }
    }

    /// 读取个人配置；无行时返回默认（不落库，待首次成功 PATCH）
    pub fn get_desktop_preferences(&self, user_id: &str) -> Result<DesktopPreferences> {
        let _timer = ReadTimer::new("get_desktop_preferences");
        let conn = self.read();
        let row: Option<(i64, String, String)> = conn
            .query_row(
                "SELECT version, payload_json, updated_at FROM desktop_preferences WHERE user_id = ?1",
                params![user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            None => Ok(Self::default_desktop_preferences(user_id)),
            Some((version, payload, updated_at)) => {
                let mut p: DesktopPreferences = serde_json::from_str(&payload)?;
                p.user_id = user_id.into();
                p.version = version as u32;
                p.updated_at = updated_at;
                Ok(p)
            }
        }
    }

    /// 乐观并发更新配置
    pub fn patch_desktop_preferences(
        &self,
        user_id: &str,
        expected_version: u32,
        patch: &DesktopPreferences,
    ) -> Result<DesktopPrefPatchOutcome> {
        let conn = self.write();
        let tx = conn.unchecked_transaction()?;
        let existing: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT version, payload_json, updated_at FROM desktop_preferences WHERE user_id = ?1",
                params![user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let current_version = existing.as_ref().map(|(v, _, _)| *v as u32).unwrap_or(1);
        // 无行时视为 version=1 的默认；首次 PATCH 须 expected_version=1
        if current_version != expected_version {
            let current = match existing {
                Some((version, payload, updated_at)) => {
                    let mut p: DesktopPreferences = serde_json::from_str(&payload)?;
                    p.user_id = user_id.into();
                    p.version = version as u32;
                    p.updated_at = updated_at;
                    p
                }
                None => Self::default_desktop_preferences(user_id),
            };
            tx.commit()?;
            return Ok(DesktopPrefPatchOutcome::Conflict(current));
        }
        let next_version = expected_version + 1;
        let updated_at = now();
        let mut saved = patch.clone();
        saved.user_id = user_id.into();
        saved.version = next_version;
        saved.updated_at = updated_at.clone();
        let payload = serde_json::to_string(&saved)?;
        if existing.is_some() {
            tx.execute(
                "UPDATE desktop_preferences SET version = ?2, payload_json = ?3, updated_at = ?4 WHERE user_id = ?1",
                params![user_id, next_version as i64, payload, updated_at],
            )?;
        } else {
            tx.execute(
                "INSERT INTO desktop_preferences (user_id, version, payload_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![user_id, next_version as i64, payload, updated_at],
            )?;
        }
        tx.commit()?;
        Ok(DesktopPrefPatchOutcome::Ok(saved))
    }

    /// 确保 SMT-03 证据种子与 alice/carol 授权（幂等）
    pub fn ensure_desktop_evidence_fixtures(&self) -> Result<()> {
        const TASK: &str = "TASK-2026-018";
        let seeds = [
            DesktopEvidence {
                id: "ev-smt-qc-1".into(),
                task_id: TASK.into(),
                title: "首件检验记录".into(),
                object_label: "质量批次 SMT-03".into(),
                collected_at: "2026-11-20T09:12:00-08:00".into(),
                source_type: "质检系统".into(),
                version: "v3".into(),
                status: "ok".into(),
                summary:
                    "事实：尺寸抽检 12/12 合格。推断：工艺窗口稳定。待核实：供应商批次一致性。"
                        .into(),
                source_body: "【原文】LOT=SMT-03-Q · Cpk=1.42 · inspector=fixture-qa".into(),
                error_message: String::new(),
            },
            DesktopEvidence {
                id: "ev-smt-fail".into(),
                task_id: TASK.into(),
                title: "影像比对报告".into(),
                object_label: "质量批次 SMT-03".into(),
                collected_at: "2026-11-20T08:01:00-08:00".into(),
                source_type: "视觉检测".into(),
                version: "v1".into(),
                status: "failed".into(),
                summary: "加载失败夹具".into(),
                source_body: String::new(),
                error_message: "证据服务暂时不可用".into(),
            },
            DesktopEvidence {
                id: "ev-smt-exp".into(),
                task_id: TASK.into(),
                title: "过期工艺参数快照".into(),
                object_label: "质量批次 SMT-03".into(),
                collected_at: "2026-10-01T10:00:00-08:00".into(),
                source_type: "MES 快照".into(),
                version: "v0".into(),
                status: "expired".into(),
                summary: "该快照已过期，不可作为核验依据".into(),
                source_body: "【过期】请重新采集".into(),
                error_message: "证据已过期，请刷新采集".into(),
            },
            DesktopEvidence {
                id: "ev-smt-revoked-src".into(),
                task_id: TASK.into(),
                title: "已撤权来源样例".into(),
                object_label: "质量批次 SMT-03".into(),
                collected_at: "2026-11-19T12:00:00-08:00".into(),
                source_type: "RelOS".into(),
                version: "v1".into(),
                status: "revoked".into(),
                summary: "来源已失效".into(),
                source_body: "SECRET-SHOULD-NOT-LEAK".into(),
                error_message: "来源权限已撤回".into(),
            },
        ];
        let conn = self.write();
        for e in &seeds {
            conn.execute(
                "INSERT OR IGNORE INTO desktop_evidence
                 (id, task_id, title, object_label, collected_at, source_type, version, status, summary, source_body, error_message)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    e.id,
                    e.task_id,
                    e.title,
                    e.object_label,
                    e.collected_at,
                    e.source_type,
                    e.version,
                    e.status,
                    e.summary,
                    e.source_body,
                    e.error_message
                ],
            )?;
            for principal in ["alice", "carol", "local"] {
                conn.execute(
                    "INSERT OR IGNORE INTO desktop_evidence_grants (evidence_id, principal_id, revoked_at)
                     VALUES (?1, ?2, NULL)",
                    params![e.id, principal],
                )?;
            }
        }
        Ok(())
    }

    fn load_evidence(conn: &Connection, id: &str) -> Result<Option<DesktopEvidence>> {
        conn.query_row(
            "SELECT id, task_id, title, object_label, collected_at, source_type, version, status, summary, source_body, error_message
             FROM desktop_evidence WHERE id = ?1",
            params![id],
            |r| {
                Ok(DesktopEvidence {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    title: r.get(2)?,
                    object_label: r.get(3)?,
                    collected_at: r.get(4)?,
                    source_type: r.get(5)?,
                    version: r.get(6)?,
                    status: r.get(7)?,
                    summary: r.get(8)?,
                    source_body: r.get(9)?,
                    error_message: r.get(10)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    fn grant_active(conn: &Connection, evidence_id: &str, principal: &str) -> Result<bool> {
        let row: Option<Option<String>> = conn
            .query_row(
                "SELECT revoked_at FROM desktop_evidence_grants WHERE evidence_id = ?1 AND principal_id = ?2",
                params![evidence_id, principal],
                |r| r.get(0),
            )
            .optional()?;
        Ok(matches!(row, Some(None)))
    }

    /// 有权主体可见的任务证据列表（不含正文；摘要仅 ok 且有权时在详情返回）
    pub fn list_desktop_evidence_for(
        &self,
        task_id: &str,
        principal: &str,
    ) -> Result<Vec<DesktopEvidence>> {
        self.ensure_desktop_evidence_fixtures()?;
        let _timer = ReadTimer::new("list_desktop_evidence");
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT e.id FROM desktop_evidence e
             INNER JOIN desktop_evidence_grants g ON g.evidence_id = e.id
             WHERE e.task_id = ?1 AND g.principal_id = ?2 AND g.revoked_at IS NULL
             ORDER BY e.collected_at DESC, e.id",
        )?;
        let ids: Vec<String> = st
            .query_map(params![task_id, principal], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        let mut out = Vec::new();
        for id in ids {
            if let Some(mut e) = Self::load_evidence(&conn, &id)? {
                e.source_body.clear();
                e.summary.clear();
                out.push(e);
            }
        }
        Ok(out)
    }

    /// 按视图读取；无授权 Forbidden；expired/revoked 对原文类视图 SourceUnavailable
    pub fn get_desktop_evidence_for(
        &self,
        evidence_id: &str,
        principal: &str,
        view: &str,
    ) -> Result<DesktopEvidenceAccess> {
        self.ensure_desktop_evidence_fixtures()?;
        let conn = self.read();
        let Some(ev) = Self::load_evidence(&conn, evidence_id)? else {
            return Ok(DesktopEvidenceAccess::NotFound);
        };
        if !Self::grant_active(&conn, evidence_id, principal)? {
            return Ok(DesktopEvidenceAccess::Forbidden);
        }
        if matches!(view, "source" | "download" | "summary")
            && matches!(ev.status.as_str(), "expired" | "revoked")
        {
            let mut redacted = ev;
            redacted.source_body.clear();
            redacted.summary.clear();
            return Ok(DesktopEvidenceAccess::SourceUnavailable(redacted));
        }
        let mut out = ev;
        if view == "summary" || view == "list" {
            out.source_body.clear();
        }
        if view == "download" || view == "source" {
            // 保留原文
        }
        if out.status == "failed" && matches!(view, "source" | "download") {
            out.source_body.clear();
        }
        Ok(DesktopEvidenceAccess::Allowed(out))
    }

    /// 撤权（幂等）；返回是否找到授权行
    pub fn revoke_desktop_evidence_grant(
        &self,
        evidence_id: &str,
        principal: &str,
    ) -> Result<bool> {
        let conn = self.write();
        let exists: Option<Option<String>> = conn
            .query_row(
                "SELECT revoked_at FROM desktop_evidence_grants WHERE evidence_id = ?1 AND principal_id = ?2",
                params![evidence_id, principal],
                |r| r.get(0),
            )
            .optional()?;
        let Some(revoked_at) = exists else {
            return Ok(false);
        };
        if revoked_at.is_none() {
            conn.execute(
                "UPDATE desktop_evidence_grants SET revoked_at = ?3 WHERE evidence_id = ?1 AND principal_id = ?2",
                params![evidence_id, principal, now()],
            )?;
        }
        Ok(true)
    }

    /// 授权搜索：仅返回有权且标题/对象匹配的条目（无正文）
    pub fn search_desktop_evidence_for(
        &self,
        principal: &str,
        query: &str,
    ) -> Result<Vec<DesktopEvidence>> {
        self.ensure_desktop_evidence_fixtures()?;
        let q = query.trim().to_lowercase();
        let _timer = ReadTimer::new("search_desktop_evidence");
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT e.id FROM desktop_evidence e
             INNER JOIN desktop_evidence_grants g ON g.evidence_id = e.id
             WHERE g.principal_id = ?1 AND g.revoked_at IS NULL
             ORDER BY e.collected_at DESC",
        )?;
        let ids: Vec<String> = st
            .query_map(params![principal], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        let mut out = Vec::new();
        for id in ids {
            if let Some(mut e) = Self::load_evidence(&conn, &id)? {
                let hay =
                    format!("{} {} {}", e.title, e.object_label, e.source_type).to_lowercase();
                if q.is_empty() || hay.contains(&q) {
                    e.source_body.clear();
                    e.summary.clear();
                    out.push(e);
                }
            }
        }
        Ok(out)
    }

    /// 测试辅助：写入自定义证据与授权
    pub fn upsert_desktop_evidence_with_grant(
        &self,
        evidence: &DesktopEvidence,
        principal: &str,
    ) -> Result<()> {
        let conn = self.write();
        conn.execute(
            "INSERT INTO desktop_evidence
             (id, task_id, title, object_label, collected_at, source_type, version, status, summary, source_body, error_message)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET
               task_id=excluded.task_id, title=excluded.title, object_label=excluded.object_label,
               collected_at=excluded.collected_at, source_type=excluded.source_type, version=excluded.version,
               status=excluded.status, summary=excluded.summary, source_body=excluded.source_body,
               error_message=excluded.error_message",
            params![
                evidence.id,
                evidence.task_id,
                evidence.title,
                evidence.object_label,
                evidence.collected_at,
                evidence.source_type,
                evidence.version,
                evidence.status,
                evidence.summary,
                evidence.source_body,
                evidence.error_message
            ],
        )?;
        conn.execute(
            "INSERT INTO desktop_evidence_grants (evidence_id, principal_id, revoked_at) VALUES (?1,?2,NULL)
             ON CONFLICT(evidence_id, principal_id) DO UPDATE SET revoked_at = NULL",
            params![evidence.id, principal],
        )?;
        Ok(())
    }

    fn map_desktop_approval(r: &rusqlite::Row<'_>) -> rusqlite::Result<DesktopApproval> {
        Ok(DesktopApproval {
            id: r.get(0)?,
            task_id: r.get(1)?,
            plan_revision: r.get(2)?,
            params: r.get(3)?,
            status: r.get(4)?,
            decided_by: r.get(5)?,
            execute_count: r.get(6)?,
            last_receipt: r.get(7)?,
        })
    }

    fn load_desktop_approval_by_task(
        conn: &Connection,
        task_id: &str,
    ) -> Result<Option<DesktopApproval>> {
        conn.query_row(
            "SELECT id, task_id, plan_revision, params, status, decided_by, execute_count, last_receipt
             FROM desktop_approvals WHERE task_id = ?1",
            params![task_id],
            Self::map_desktop_approval,
        )
        .optional()
        .map_err(Into::into)
    }

    fn load_desktop_approval_by_id(conn: &Connection, id: &str) -> Result<Option<DesktopApproval>> {
        conn.query_row(
            "SELECT id, task_id, plan_revision, params, status, decided_by, execute_count, last_receipt
             FROM desktop_approvals WHERE id = ?1",
            params![id],
            Self::map_desktop_approval,
        )
        .optional()
        .map_err(Into::into)
    }

    fn save_desktop_approval(conn: &Connection, a: &DesktopApproval) -> Result<()> {
        conn.execute(
            "INSERT INTO desktop_approvals
             (id, task_id, plan_revision, params, status, decided_by, execute_count, last_receipt, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(task_id) DO UPDATE SET
               id=excluded.id, plan_revision=excluded.plan_revision, params=excluded.params,
               status=excluded.status, decided_by=excluded.decided_by,
               execute_count=excluded.execute_count, last_receipt=excluded.last_receipt,
               updated_at=excluded.updated_at",
            params![
                a.id,
                a.task_id,
                a.plan_revision,
                a.params,
                a.status,
                a.decided_by,
                a.execute_count,
                a.last_receipt,
                now()
            ],
        )?;
        Ok(())
    }

    /// 读取或创建任务默认审批夹具
    pub fn get_or_create_desktop_approval(&self, task_id: &str) -> Result<DesktopApproval> {
        let conn = self.write();
        if let Some(a) = Self::load_desktop_approval_by_task(&conn, task_id)? {
            return Ok(a);
        }
        let a = DesktopApproval {
            id: format!("appr-{task_id}"),
            task_id: task_id.into(),
            plan_revision: 1,
            params: "停机检修 · 备用机切换".into(),
            status: "pending".into(),
            decided_by: None,
            execute_count: 0,
            last_receipt: "none".into(),
        };
        Self::save_desktop_approval(&conn, &a)?;
        Ok(a)
    }

    /// 重置为初始 pending（夹具）
    pub fn reset_desktop_approval(&self, task_id: &str) -> Result<DesktopApproval> {
        let a = DesktopApproval {
            id: format!("appr-{task_id}"),
            task_id: task_id.into(),
            plan_revision: 1,
            params: "停机检修 · 备用机切换".into(),
            status: "pending".into(),
            decided_by: None,
            execute_count: 0,
            last_receipt: "none".into(),
        };
        let conn = self.write();
        Self::save_desktop_approval(&conn, &a)?;
        Ok(a)
    }

    pub fn decide_desktop_approval(
        &self,
        id: &str,
        decision: &str,
        expected_revision: i64,
        decided_by: &str,
    ) -> Result<DesktopApprovalDecideOutcome> {
        let conn = self.write();
        let Some(mut a) = Self::load_desktop_approval_by_id(&conn, id)? else {
            return Ok(DesktopApprovalDecideOutcome::NotFound);
        };
        if a.plan_revision != expected_revision {
            return Ok(DesktopApprovalDecideOutcome::RevisionConflict(a));
        }
        match decision {
            "approve" => {
                a.status = "approved".into();
                a.decided_by = Some(decided_by.into());
            }
            "reject" => {
                a.status = "rejected".into();
                a.decided_by = Some(decided_by.into());
            }
            _ => return Ok(DesktopApprovalDecideOutcome::NotFound),
        }
        Self::save_desktop_approval(&conn, &a)?;
        Ok(DesktopApprovalDecideOutcome::Ok(a))
    }

    pub fn bump_desktop_approval(&self, id: &str, params: &str) -> Result<Option<DesktopApproval>> {
        let conn = self.write();
        let Some(mut a) = Self::load_desktop_approval_by_id(&conn, id)? else {
            return Ok(None);
        };
        a.params = params.into();
        a.plan_revision += 1;
        a.status = "pending".into();
        a.decided_by = None;
        a.last_receipt = "none".into();
        // 升版不重置 execute_count（历史副作用保留）；本夹具单方案行仍保留计数供审计
        Self::save_desktop_approval(&conn, &a)?;
        Ok(Some(a))
    }

    pub fn expire_desktop_approval(&self, id: &str) -> Result<Option<DesktopApproval>> {
        let conn = self.write();
        let Some(mut a) = Self::load_desktop_approval_by_id(&conn, id)? else {
            return Ok(None);
        };
        a.status = "expired".into();
        Self::save_desktop_approval(&conn, &a)?;
        Ok(Some(a))
    }

    pub fn execute_desktop_approval(
        &self,
        id: &str,
        expected_revision: i64,
        force_unknown: bool,
    ) -> Result<DesktopApprovalExecuteOutcome> {
        let conn = self.write();
        let Some(mut a) = Self::load_desktop_approval_by_id(&conn, id)? else {
            return Ok(DesktopApprovalExecuteOutcome::NotFound);
        };
        if a.plan_revision != expected_revision {
            return Ok(DesktopApprovalExecuteOutcome::RevisionConflict(a));
        }
        if a.status == "rejected" || a.status == "expired" {
            return Ok(DesktopApprovalExecuteOutcome::Forbidden(a));
        }
        if a.status != "approved" {
            return Ok(DesktopApprovalExecuteOutcome::Forbidden(a));
        }
        if force_unknown {
            a.last_receipt = "unknown".into();
            Self::save_desktop_approval(&conn, &a)?;
            return Ok(DesktopApprovalExecuteOutcome::Ok(a));
        }
        if a.execute_count > 0 && a.last_receipt == "applied" {
            return Ok(DesktopApprovalExecuteOutcome::Ok(a));
        }
        a.execute_count += 1;
        a.last_receipt = "applied".into();
        Self::save_desktop_approval(&conn, &a)?;
        Ok(DesktopApprovalExecuteOutcome::Ok(a))
    }

    pub fn ack_desktop_approval_unknown(&self, id: &str) -> Result<Option<DesktopApproval>> {
        let conn = self.write();
        let Some(mut a) = Self::load_desktop_approval_by_id(&conn, id)? else {
            return Ok(None);
        };
        if a.last_receipt != "unknown" {
            return Ok(Some(a));
        }
        a.last_receipt = "applied".into();
        if a.execute_count < 1 {
            a.execute_count = 1;
        }
        Self::save_desktop_approval(&conn, &a)?;
        Ok(Some(a))
    }

    pub fn get_desktop_attention_mute(&self, user_id: &str) -> Result<bool> {
        let conn = self.read();
        let v: Option<i64> = conn
            .query_row(
                "SELECT mute_ordinary FROM desktop_attention_prefs WHERE user_id = ?1",
                params![user_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.unwrap_or(0) != 0)
    }

    pub fn set_desktop_attention_mute(&self, user_id: &str, mute: bool) -> Result<()> {
        let conn = self.write();
        conn.execute(
            "INSERT INTO desktop_attention_prefs (user_id, mute_ordinary) VALUES (?1, ?2)
             ON CONFLICT(user_id) DO UPDATE SET mute_ordinary = excluded.mute_ordinary",
            params![user_id, if mute { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn list_desktop_attention(&self) -> Result<Vec<DesktopAttentionItem>> {
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT id, kind, title, body, task_id, created_at FROM desktop_attention_items
             ORDER BY created_at DESC, id DESC LIMIT 40",
        )?;
        let rows = st.query_map([], |r| {
            Ok(DesktopAttentionItem {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                task_id: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        let mut out: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
        out.reverse();
        Ok(out)
    }

    /// 投递 Attention；返回是否实际入队
    pub fn enqueue_desktop_attention(
        &self,
        user_id: &str,
        kind: &str,
        title: &str,
        body: &str,
        task_id: Option<&str>,
    ) -> Result<(bool, Option<DesktopAttentionItem>)> {
        let mute = self.get_desktop_attention_mute(user_id)?;
        if mute && kind != "urgent" {
            return Ok((false, None));
        }
        let item = DesktopAttentionItem {
            id: an_core::new_id("attn"),
            kind: kind.into(),
            title: title.into(),
            body: body.into(),
            task_id: task_id.map(str::to_string),
            created_at: now(),
        };
        let conn = self.write();
        if kind == "ordinary" {
            conn.execute(
                "DELETE FROM desktop_attention_items WHERE kind = 'ordinary' AND title = ?1",
                params![title],
            )?;
        }
        conn.execute(
            "INSERT INTO desktop_attention_items (id, kind, title, body, task_id, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                item.id,
                item.kind,
                item.title,
                item.body,
                item.task_id,
                item.created_at
            ],
        )?;
        Ok((true, Some(item)))
    }

    pub fn dismiss_desktop_attention(&self, id: &str) -> Result<()> {
        let conn = self.write();
        conn.execute(
            "DELETE FROM desktop_attention_items WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn clear_desktop_attention(&self) -> Result<()> {
        let conn = self.write();
        conn.execute("DELETE FROM desktop_attention_items", [])?;
        Ok(())
    }

    fn map_voice(r: &rusqlite::Row<'_>) -> rusqlite::Result<DesktopVoiceSession> {
        let playing: i64 = r.get(4)?;
        let share: i64 = r.get(8)?;
        Ok(DesktopVoiceSession {
            id: r.get(0)?,
            task_id: r.get(1)?,
            permission: r.get(2)?,
            phase: r.get(3)?,
            playing: playing != 0,
            partial: r.get(5)?,
            final_text: r.get(6)?,
            linked_task_id: r.get(7)?,
            share_transcript: share != 0,
        })
    }

    fn load_voice(conn: &Connection, task_id: &str) -> Result<Option<DesktopVoiceSession>> {
        conn.query_row(
            "SELECT id, task_id, permission, phase, playing, partial, final_text, linked_task_id, share_transcript
             FROM desktop_voice_sessions WHERE task_id = ?1",
            params![task_id],
            Self::map_voice,
        )
        .optional()
        .map_err(Into::into)
    }

    fn save_voice(conn: &Connection, s: &DesktopVoiceSession) -> Result<()> {
        conn.execute(
            "INSERT INTO desktop_voice_sessions
             (id, task_id, permission, phase, playing, partial, final_text, linked_task_id, share_transcript, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(task_id) DO UPDATE SET
               id=excluded.id, permission=excluded.permission, phase=excluded.phase,
               playing=excluded.playing, partial=excluded.partial, final_text=excluded.final_text,
               linked_task_id=excluded.linked_task_id, share_transcript=excluded.share_transcript,
               updated_at=excluded.updated_at",
            params![
                s.id,
                s.task_id,
                s.permission,
                s.phase,
                if s.playing { 1 } else { 0 },
                s.partial,
                s.final_text,
                s.linked_task_id,
                if s.share_transcript { 1 } else { 0 },
                now()
            ],
        )?;
        Ok(())
    }

    fn empty_voice(task_id: &str) -> DesktopVoiceSession {
        DesktopVoiceSession {
            id: format!("voice-{task_id}"),
            task_id: task_id.into(),
            permission: "prompt".into(),
            phase: "idle".into(),
            playing: false,
            partial: String::new(),
            final_text: String::new(),
            linked_task_id: None,
            share_transcript: false,
        }
    }

    pub fn reset_desktop_voice(&self, task_id: &str) -> Result<DesktopVoiceSession> {
        let s = Self::empty_voice(task_id);
        let conn = self.write();
        Self::save_voice(&conn, &s)?;
        Ok(s)
    }

    pub fn get_or_create_desktop_voice(&self, task_id: &str) -> Result<DesktopVoiceSession> {
        let conn = self.write();
        if let Some(s) = Self::load_voice(&conn, task_id)? {
            return Ok(s);
        }
        let s = Self::empty_voice(task_id);
        Self::save_voice(&conn, &s)?;
        Ok(s)
    }

    pub fn set_desktop_voice_permission(
        &self,
        task_id: &str,
        permission: &str,
    ) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        if permission == "denied" {
            s.permission = "denied".into();
            s.phase = "idle".into();
            s.playing = false;
            s.partial.clear();
        } else {
            s.permission = permission.into();
        }
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn start_desktop_voice(&self, task_id: &str) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        if s.permission == "denied" {
            Self::save_voice(&conn, &s)?;
            return Ok(Some(s));
        }
        if s.permission == "prompt" {
            s.permission = "granted".into();
            s.phase = "listening".into();
            s.playing = false;
        } else {
            s.phase = "listening".into();
            s.playing = false;
        }
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn set_desktop_voice_partial(
        &self,
        task_id: &str,
        text: &str,
    ) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        if s.phase == "listening" || s.phase == "speaking" {
            s.partial = text.into();
        }
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn finalize_desktop_voice(
        &self,
        task_id: &str,
        text: &str,
        link_task: bool,
    ) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        s.partial.clear();
        s.final_text = text.into();
        s.linked_task_id = if link_task {
            Some(s.task_id.clone())
        } else {
            None
        };
        s.phase = "listening".into();
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn play_desktop_voice(&self, task_id: &str) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        s.playing = true;
        s.phase = "speaking".into();
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn barge_desktop_voice(&self, task_id: &str) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        s.playing = false;
        s.phase = "listening".into();
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn offline_desktop_voice(&self, task_id: &str) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        s.phase = "offline".into();
        s.playing = false;
        s.partial.clear();
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn voice_continue_as_text(
        &self,
        task_id: &str,
        text: &str,
    ) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        if s.permission != "denied" && s.phase != "offline" {
            s.phase = "listening".into();
        }
        s.final_text = text.into();
        s.partial.clear();
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    pub fn set_desktop_voice_share(
        &self,
        task_id: &str,
        share: bool,
    ) -> Result<Option<DesktopVoiceSession>> {
        let conn = self.write();
        let Some(mut s) = Self::load_voice(&conn, task_id)? else {
            return Ok(None);
        };
        s.share_transcript = share;
        Self::save_voice(&conn, &s)?;
        Ok(Some(s))
    }

    fn map_handoff(r: &rusqlite::Row<'_>) -> rusqlite::Result<DesktopHandoff> {
        let conflict: i64 = r.get(8)?;
        let voice_shared: i64 = r.get(10)?;
        Ok(DesktopHandoff {
            task_id: r.get(0)?,
            conversation_id: r.get(1)?,
            run_id: r.get(2)?,
            read_message_id: r.get(3)?,
            phone_text: r.get(4)?,
            phone_version: r.get(5)?,
            desktop_text: r.get(6)?,
            desktop_version: r.get(7)?,
            conflict: conflict != 0,
            chosen: r.get(9)?,
            voice_shared: voice_shared != 0,
        })
    }

    fn load_handoff(conn: &Connection, task_id: &str) -> Result<Option<DesktopHandoff>> {
        conn.query_row(
            "SELECT task_id, conversation_id, run_id, read_message_id,
                    phone_text, phone_version, desktop_text, desktop_version,
                    conflict, chosen, voice_shared
             FROM desktop_handoff WHERE task_id = ?1",
            params![task_id],
            Self::map_handoff,
        )
        .optional()
        .map_err(Into::into)
    }

    fn save_handoff(conn: &Connection, h: &DesktopHandoff) -> Result<()> {
        conn.execute(
            "INSERT INTO desktop_handoff
             (task_id, conversation_id, run_id, read_message_id, phone_text, phone_version,
              desktop_text, desktop_version, conflict, chosen, voice_shared, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(task_id) DO UPDATE SET
               conversation_id=excluded.conversation_id, run_id=excluded.run_id,
               read_message_id=excluded.read_message_id, phone_text=excluded.phone_text,
               phone_version=excluded.phone_version, desktop_text=excluded.desktop_text,
               desktop_version=excluded.desktop_version, conflict=excluded.conflict,
               chosen=excluded.chosen, voice_shared=excluded.voice_shared,
               updated_at=excluded.updated_at",
            params![
                h.task_id,
                h.conversation_id,
                h.run_id,
                h.read_message_id,
                h.phone_text,
                h.phone_version,
                h.desktop_text,
                h.desktop_version,
                if h.conflict { 1 } else { 0 },
                h.chosen,
                if h.voice_shared { 1 } else { 0 },
                now()
            ],
        )?;
        Ok(())
    }

    fn empty_handoff(task_id: &str) -> DesktopHandoff {
        DesktopHandoff {
            task_id: task_id.into(),
            conversation_id: "conv-smt-03".into(),
            run_id: "run-smt-03-1".into(),
            read_message_id: "msg-anchor-1".into(),
            phone_text: String::new(),
            phone_version: 0,
            desktop_text: String::new(),
            desktop_version: 0,
            conflict: false,
            chosen: None,
            voice_shared: false,
        }
    }

    pub fn reset_desktop_handoff(&self, task_id: &str) -> Result<DesktopHandoff> {
        let h = Self::empty_handoff(task_id);
        let conn = self.write();
        Self::save_handoff(&conn, &h)?;
        Ok(h)
    }

    pub fn get_or_create_desktop_handoff(&self, task_id: &str) -> Result<DesktopHandoff> {
        let conn = self.write();
        if let Some(h) = Self::load_handoff(&conn, task_id)? {
            return Ok(h);
        }
        let h = Self::empty_handoff(task_id);
        Self::save_handoff(&conn, &h)?;
        Ok(h)
    }

    pub fn write_desktop_handoff_draft(
        &self,
        task_id: &str,
        device: &str,
        text: &str,
    ) -> Result<Option<DesktopHandoff>> {
        let conn = self.write();
        let Some(mut h) = Self::load_handoff(&conn, task_id)? else {
            return Ok(None);
        };
        match device {
            "phone" => {
                h.phone_version += 1;
                h.phone_text = text.into();
                h.conflict =
                    h.desktop_version > 0 && !h.desktop_text.is_empty() && h.desktop_text != text;
            }
            "desktop" => {
                h.desktop_version += 1;
                h.desktop_text = text.into();
                h.conflict =
                    h.phone_version > 0 && !h.phone_text.is_empty() && h.phone_text != text;
            }
            _ => return Ok(Some(h)),
        }
        if h.conflict {
            h.chosen = None;
        }
        Self::save_handoff(&conn, &h)?;
        Ok(Some(h))
    }

    pub fn resolve_desktop_handoff(
        &self,
        task_id: &str,
        chosen: &str,
    ) -> Result<Option<DesktopHandoff>> {
        let conn = self.write();
        let Some(mut h) = Self::load_handoff(&conn, task_id)? else {
            return Ok(None);
        };
        let keep = if chosen == "phone" {
            h.phone_text.clone()
        } else {
            h.desktop_text.clone()
        };
        h.phone_text = keep.clone();
        h.desktop_text = keep;
        h.conflict = false;
        h.chosen = Some(chosen.into());
        Self::save_handoff(&conn, &h)?;
        Ok(Some(h))
    }

    pub fn set_desktop_handoff_voice_shared(
        &self,
        task_id: &str,
        shared: bool,
    ) -> Result<Option<DesktopHandoff>> {
        let conn = self.write();
        let Some(mut h) = Self::load_handoff(&conn, task_id)? else {
            return Ok(None);
        };
        h.voice_shared = shared;
        Self::save_handoff(&conn, &h)?;
        Ok(Some(h))
    }

    /// 按时间顺序返回消息；`last` 为 Some(n) 时只返回最近 n 条
    pub fn list_messages(
        &self,
        conversation_id: &str,
        last: Option<usize>,
    ) -> Result<Vec<Message>> {
        let _timer = ReadTimer::new("list_messages");
        let conn = self.read();
        let limit = last.map(|n| n as i64).unwrap_or(-1); // SQLite 中 LIMIT -1 表示不限制
        let mut st = conn.prepare(
            "SELECT id, conversation_id, role, content, task_id, created_at FROM messages
             WHERE conversation_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![conversation_id, limit], |r| {
            Ok(Message {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                task_id: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        let mut v: Vec<Message> = rows.collect::<rusqlite::Result<_>>()?;
        v.reverse();
        Ok(v)
    }

    // ───────────── 任务 ─────────────

    pub fn insert_task(&self, t: &TaskRecord, context: &str) -> Result<()> {
        self.write().execute(
            "INSERT INTO tasks (id, conversation_id, goal, role, status, route_json, policy_json, result_json, review_json, created_at, updated_at, context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                t.id,
                t.conversation_id,
                t.goal,
                t.role.as_str(),
                t.status.as_str(),
                serde_json::to_string(&t.route)?,
                serde_json::to_string(&t.policy)?,
                t.result.as_ref().map(serde_json::to_string).transpose()?,
                t.review.as_ref().map(serde_json::to_string).transpose()?,
                t.created_at,
                t.updated_at,
                context,
            ],
        )?;
        Ok(())
    }

    /// 执行上下文（近期对话摘录；返工时会追加审查问题）。submit 时写入，执行时读取
    pub fn get_task_context(&self, id: &str) -> Result<Option<String>> {
        let _timer = ReadTimer::new("get_task_context");
        Ok(self
            .read()
            .query_row("SELECT context FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_task_context(&self, id: &str, context: &str) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET context = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, context, now()],
        )?;
        Ok(())
    }

    pub fn set_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now()],
        )?;
        Ok(())
    }

    /// 带条件的状态转换：只有当前状态属于 `from` 时才改成 `to`。返回 true 表示确实改了。
    /// 判断和修改在同一条 UPDATE 里完成，并发调用时只有一个能成功（取消 / 完成 / 批准互不覆盖）。
    pub fn transition_task(&self, id: &str, from: &[TaskStatus], to: TaskStatus) -> Result<bool> {
        self.finish_task(id, from, to, None)
    }

    /// 人工批准：只有 NEEDS_HUMAN 且还没有执行结果（审批前就停下的任务）才能改成 QUEUED。
    /// 并发批准时只有一个返回 true。
    pub fn approve_task(&self, id: &str) -> Result<bool> {
        let changed = self.write().execute(
            "UPDATE tasks SET status = 'QUEUED', updated_at = ?2 WHERE id = ?1 AND status = 'NEEDS_HUMAN' AND result_json IS NULL",
            params![id, now()],
        )?;
        Ok(changed == 1)
    }

    /// 同 [`Store::transition_task`]，`result` 有值时与状态在同一条 UPDATE 里写入：
    /// 条件不满足时状态和结果都不写。
    pub fn finish_task(
        &self,
        id: &str,
        from: &[TaskStatus],
        to: TaskStatus,
        result: Option<&TaskResult>,
    ) -> Result<bool> {
        if from.is_empty() {
            return Ok(false);
        }
        // from 来自代码里的常量，只有固定的几个值；仍然走参数绑定，不拼接字符串
        let placeholders = (0..from.len())
            .map(|i| format!("?{}", i + 5))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE tasks SET status = ?2, updated_at = ?3, result_json = COALESCE(?4, result_json) WHERE id = ?1 AND status IN ({placeholders})"
        );
        let result_json = result.map(serde_json::to_string).transpose()?;
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(id.to_string()),
            Box::new(to.as_str()),
            Box::new(now()),
            Box::new(result_json),
        ];
        values.extend(
            from.iter()
                .map(|s| Box::new(s.as_str()) as Box<dyn rusqlite::ToSql>),
        );
        let changed = self
            .write()
            .execute(&sql, rusqlite::params_from_iter(values.iter()))?;
        Ok(changed == 1)
    }

    pub fn set_task_result(&self, id: &str, result: &TaskResult) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET result_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(result)?, now()],
        )?;
        Ok(())
    }

    /// 记录执行者（每轮拿到 agent 时写入，返工轮覆盖之前的）
    pub fn set_task_assignment(&self, id: &str, assignment: &Assignment) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET assignment_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(assignment)?, now()],
        )?;
        Ok(())
    }

    pub fn set_task_review(&self, id: &str, review: &ReviewVerdict) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET review_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(review)?, now()],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let _timer = ReadTimer::new("get_task");
        let conn = self.read();
        let mut st = conn.prepare(&format!("{TASK_SELECT} WHERE id = ?1"))?;
        let rec = st.query_row([id], task_from_row).optional()?;
        rec.transpose()
    }

    /// 列出任务；`conversation_id` 为 None 时返回全部
    pub fn list_tasks(&self, conversation_id: Option<&str>) -> Result<Vec<TaskRecord>> {
        let _timer = ReadTimer::new("list_tasks");
        let conn = self.read();
        let rows: Vec<Result<TaskRecord>> = match conversation_id {
            Some(c) => {
                let mut st = conn.prepare(&format!(
                    "{TASK_SELECT} WHERE conversation_id = ?1 ORDER BY created_at DESC"
                ))?;
                let r = st
                    .query_map([c], task_from_row)?
                    .collect::<rusqlite::Result<_>>()?;
                r
            }
            None => {
                let mut st =
                    conn.prepare(&format!("{TASK_SELECT} ORDER BY created_at DESC LIMIT 200"))?;
                let r = st
                    .query_map([], task_from_row)?
                    .collect::<rusqlite::Result<_>>()?;
                r
            }
        };
        rows.into_iter().collect()
    }

    /// 排队名次：同角色、比这个任务早创建、还在 QUEUED 的任务数（不含自己）
    pub fn queued_ahead(&self, id: &str) -> Result<i64> {
        let _timer = ReadTimer::new("queued_ahead");
        Ok(self.read().query_row(
            "SELECT COUNT(*) FROM tasks o JOIN tasks me ON me.id = ?1
             WHERE o.role = me.role AND o.status = 'QUEUED' AND o.created_at < me.created_at AND o.id <> me.id",
            [id],
            |r| r.get(0),
        )?)
    }

    /// 会话中尚未结束的任务（给 Turn Controller 判断 follow_up 用）
    pub fn active_tasks(&self, conversation_id: &str) -> Result<Vec<TaskRecord>> {
        Ok(self
            .list_tasks(Some(conversation_id))?
            .into_iter()
            .filter(|t| !t.status.is_terminal())
            .collect())
    }

    /// 服务启动时调用：把上次运行中断的任务标记为失败（MVP 不做断点续跑）
    pub fn fail_interrupted_tasks(&self) -> Result<usize> {
        Ok(self.write().execute(
            "UPDATE tasks SET status = 'FAILED', updated_at = ?1 WHERE status IN ('CREATED', 'QUEUED', 'RUNNING', 'VERIFYING')",
            [now()],
        )?)
    }

    // ───────────── 事件 ─────────────

    pub fn insert_event(&self, e: &TaskEvent) -> Result<()> {
        self.write().execute(
            "INSERT INTO events (id, conversation_id, task_id, json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.id, e.conversation_id, e.task_id, serde_json::to_string(e)?, e.created_at],
        )?;
        Ok(())
    }

    pub fn list_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        self.list_events_after(task_id, None)
    }

    /// `after_id` 之后的事件（不含这条）。用于任务详情增量拉取。找不到该 id 时返回全部，避免漏事件
    pub fn list_events_after(
        &self,
        task_id: &str,
        after_id: Option<&str>,
    ) -> Result<Vec<TaskEvent>> {
        let _timer = ReadTimer::new("list_events");
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT json FROM events WHERE task_id = ?1 AND (?2 IS NULL OR rowid > COALESCE((SELECT rowid FROM events WHERE id = ?2), 0)) ORDER BY created_at, rowid",
        )?;
        let rows = st.query_map(params![task_id, after_id], |r| r.get::<_, String>(0))?;
        rows.map(|j| Ok(serde_json::from_str(&j?)?)).collect()
    }

    /// AI Desktop B2：会话维度事件游标回放。`after_id` 缺失/不属于该会话 → `CursorExpired`（与任务详情「找不到就全量」不同）。
    pub fn list_conversation_events_after(
        &self,
        conversation_id: &str,
        after_id: Option<&str>,
    ) -> Result<DesktopEventsAfter> {
        let _timer = ReadTimer::new("list_conversation_events");
        let conn = self.read();
        let after_rowid: Option<i64> = match after_id {
            None => None,
            Some(aid) => {
                let row: Option<i64> = conn
                    .query_row(
                        "SELECT rowid FROM events WHERE id = ?1 AND conversation_id = ?2",
                        params![aid, conversation_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                match row {
                    Some(r) => Some(r),
                    None => return Ok(DesktopEventsAfter::CursorExpired),
                }
            }
        };
        let mut st = conn.prepare(
            "SELECT id, json FROM events WHERE conversation_id = ?1 AND (?2 IS NULL OR rowid > ?2) ORDER BY created_at, rowid",
        )?;
        let rows = st.query_map(params![conversation_id, after_rowid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, json) = row?;
            let event: TaskEvent = serde_json::from_str(&json)?;
            out.push(DesktopEventCursor { cursor: id, event });
        }
        Ok(DesktopEventsAfter::Ok(out))
    }

    // ───────────── 节点 ─────────────

    /// 保存（或更新）配对的节点。`cards` 是节点最后一次上报的 agent 列表，节点离线时用它显示
    pub fn upsert_node(&self, n: &StoredNode) -> Result<()> {
        self.write().execute(
            "INSERT INTO nodes (node_id, name, address, token, cards_json, paired_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(node_id) DO UPDATE SET name = ?2, address = ?3, token = ?4, cards_json = ?5",
            params![n.node_id, n.name, n.address, n.token, serde_json::to_string(&n.cards)?, n.paired_at],
        )?;
        Ok(())
    }

    pub fn update_node_cards(&self, node_id: &str, cards: &[an_core::AgentCard]) -> Result<()> {
        self.write().execute(
            "UPDATE nodes SET cards_json = ?2 WHERE node_id = ?1",
            params![node_id, serde_json::to_string(cards)?],
        )?;
        Ok(())
    }

    pub fn list_nodes(&self) -> Result<Vec<StoredNode>> {
        let _timer = ReadTimer::new("list_nodes");
        let conn = self.read();
        let mut st = conn.prepare("SELECT node_id, name, address, token, cards_json, paired_at FROM nodes ORDER BY paired_at")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (node_id, name, address, token, cards, paired_at) = row?;
            Ok(StoredNode {
                node_id,
                name,
                address,
                token,
                cards: serde_json::from_str(&cards)?,
                paired_at,
            })
        })
        .collect()
    }

    pub fn delete_node(&self, node_id: &str) -> Result<bool> {
        Ok(self
            .write()
            .execute("DELETE FROM nodes WHERE node_id = ?1", [node_id])?
            > 0)
    }

    // ───────────── 业务内省（INT） ─────────────
    // 只新建表、不改已有表；业务规则在 `crate::introspect`，这里只负责原子落库。

    /// 抢一个运行租约：已有未过期的 running 任务则返回 None；否则新建 running 任务。
    /// 整个“检查 + 新建”在同一个事务里，手动触发和定时触发撞车时只有一个能拿到。
    pub fn introspection_acquire_job(
        &self,
        trigger: &str,
        watermark_json: &str,
        input_hash: &str,
        lease_until: &str,
    ) -> Result<Option<IntrospectionJob>> {
        let mut conn = self.write();
        let tx = conn.transaction()?;
        let at = now();
        let busy: i64 = tx.query_row(
            "SELECT COUNT(*) FROM introspection_jobs WHERE state = 'running' AND lease_until > ?1",
            [&at],
            |r| r.get(0),
        )?;
        if busy > 0 {
            return Ok(None);
        }
        let job = IntrospectionJob {
            id: an_core::new_id("intjob"),
            trigger: trigger.into(),
            state: "running".into(),
            watermark_json: watermark_json.into(),
            input_hash: input_hash.into(),
            lease_until: Some(lease_until.into()),
            started_at: at,
            finished_at: None,
            error: None,
        };
        tx.execute(
            "INSERT INTO introspection_jobs
             (id, trigger, state, watermark_json, input_hash, lease_until, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                job.id,
                job.trigger,
                job.state,
                job.watermark_json,
                job.input_hash,
                job.lease_until,
                job.started_at
            ],
        )?;
        tx.commit()?;
        Ok(Some(job))
    }

    /// 结束任务并释放租约；任务不存在返回 false。
    pub fn introspection_finish_job(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<bool> {
        Ok(self.write().execute(
            "UPDATE introspection_jobs
             SET state = ?2, error = ?3, finished_at = ?4, lease_until = NULL WHERE id = ?1",
            params![id, state, error, now()],
        )? > 0)
    }

    pub fn introspection_get_job(&self, id: &str) -> Result<Option<IntrospectionJob>> {
        let _timer = ReadTimer::new("introspection_get_job");
        let conn = self.read();
        Ok(conn
            .query_row(
                "SELECT id, trigger, state, watermark_json, input_hash, lease_until,
                        started_at, finished_at, error
                 FROM introspection_jobs WHERE id = ?1",
                [id],
                |r| {
                    Ok(IntrospectionJob {
                        id: r.get(0)?,
                        trigger: r.get(1)?,
                        state: r.get(2)?,
                        watermark_json: r.get(3)?,
                        input_hash: r.get(4)?,
                        lease_until: r.get(5)?,
                        started_at: r.get(6)?,
                        finished_at: r.get(7)?,
                        error: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    /// 最近一次成功（done）任务的水位；失败、取消的任务不推进水位。
    pub fn introspection_latest_watermark(&self) -> Result<Option<String>> {
        let _timer = ReadTimer::new("introspection_latest_watermark");
        let conn = self.read();
        Ok(conn
            .query_row(
                "SELECT watermark_json FROM introspection_jobs WHERE state = 'done'
                 ORDER BY started_at DESC, rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// 把租约已过期仍为 running 的任务（上次崩溃遗留）标为 failed，返回条数。
    pub fn introspection_fail_stale_jobs(&self) -> Result<usize> {
        let at = now();
        Ok(self.write().execute(
            "UPDATE introspection_jobs
             SET state = 'failed', error = 'interrupted: 进程中断，租约已过期',
                 finished_at = ?1, lease_until = NULL
             WHERE state = 'running' AND lease_until IS NOT NULL AND lease_until <= ?1",
            [&at],
        )?)
    }

    /// 写入结论；稳定 ID 已存在则不覆盖，返回是否新写入。
    pub fn introspection_insert_finding(&self, f: &IntrospectionFinding) -> Result<bool> {
        Ok(self.write().execute(
            "INSERT INTO introspection_findings
             (id, job_id, topic, json, verification, gate_score, gate_reason, pushed,
              verdict, verdict_note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO NOTHING",
            params![
                f.id,
                f.job_id,
                f.topic,
                f.json,
                f.verification,
                f.gate_score,
                f.gate_reason,
                f.pushed as i64,
                f.verdict,
                f.verdict_note,
                f.created_at,
                f.updated_at
            ],
        )? > 0)
    }

    pub fn introspection_get_finding(&self, id: &str) -> Result<Option<IntrospectionFinding>> {
        let _timer = ReadTimer::new("introspection_get_finding");
        let conn = self.read();
        Ok(conn
            .query_row(
                &format!("{FINDING_COLUMNS} WHERE id = ?1"),
                [id],
                finding_from_row,
            )
            .optional()?)
    }

    /// 按创建时间倒序；`topic` 为 None 时不过滤。
    pub fn introspection_list_findings(
        &self,
        topic: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IntrospectionFinding>> {
        let _timer = ReadTimer::new("introspection_list_findings");
        let conn = self.read();
        let mut st = conn.prepare(&format!(
            "{FINDING_COLUMNS} WHERE (?1 IS NULL OR topic = ?1)
             ORDER BY created_at DESC, rowid DESC LIMIT ?2"
        ))?;
        let rows = st.query_map(params![topic, limit as i64], finding_from_row)?;
        rows.map(|r| Ok(r?)).collect()
    }

    /// 记录发送闸门的打分和原因（推送或不推送都要记）。
    pub fn introspection_set_gate(
        &self,
        id: &str,
        score: f64,
        reason: &str,
        pushed: bool,
    ) -> Result<bool> {
        Ok(self.write().execute(
            "UPDATE introspection_findings
             SET gate_score = ?2, gate_reason = ?3, pushed = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, score, reason, pushed as i64, now()],
        )? > 0)
    }

    /// 运营负责人的判断（confirmed / rejected）和原因。
    pub fn introspection_set_verdict(&self, id: &str, verdict: &str, note: &str) -> Result<bool> {
        Ok(self.write().execute(
            "UPDATE introspection_findings
             SET verdict = ?2, verdict_note = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, verdict, note, now()],
        )? > 0)
    }

    /// 记录一次推送；同一条结论只推一次，已推过返回 None。
    pub fn introspection_add_push(
        &self,
        finding_id: &str,
        message_id: &str,
    ) -> Result<Option<String>> {
        let mut conn = self.write();
        let tx = conn.transaction()?;
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM introspection_pushes WHERE finding_id = ?1",
            [finding_id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Ok(None);
        }
        let id = an_core::new_id("intpush");
        tx.execute(
            "INSERT INTO introspection_pushes (id, finding_id, message_id, pushed_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, finding_id, message_id, now()],
        )?;
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn introspection_unread_count(&self) -> Result<usize> {
        let _timer = ReadTimer::new("introspection_unread_count");
        let conn = self.read();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM introspection_pushes WHERE read_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// 今日（UTC）已投递的内省推送条数，供日配额按真实投递记录执行。
    pub fn introspection_pushes_today(&self) -> Result<u32> {
        let _timer = ReadTimer::new("introspection_pushes_today");
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let start = format!("{day}T00:00:00.000Z");
        let conn = self.read();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM introspection_pushes WHERE pushed_at >= ?1",
            [start],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// 某条结论是否仍有未读推送（用于闸门「已推未读不重推」）。
    pub fn introspection_has_unread_push(&self, finding_id: &str) -> Result<bool> {
        let conn = self.read();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM introspection_pushes
             WHERE finding_id = ?1 AND read_at IS NULL",
            [finding_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// 某条结论已推送时返回对应消息 id；未推过返回 None（S5 推送幂等）。
    pub fn introspection_pushed_message_id(&self, finding_id: &str) -> Result<Option<String>> {
        let conn = self.read();
        Ok(conn
            .query_row(
                "SELECT message_id FROM introspection_pushes WHERE finding_id = ?1 LIMIT 1",
                [finding_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// 全部标为已读，返回本次标记的条数。
    pub fn introspection_mark_all_read(&self) -> Result<usize> {
        Ok(self.write().execute(
            "UPDATE introspection_pushes SET read_at = ?1 WHERE read_at IS NULL",
            [now()],
        )?)
    }

    pub fn introspection_add_label(
        &self,
        finding_id: &str,
        kind: &str,
        note: &str,
    ) -> Result<IntrospectionLabel> {
        let l = IntrospectionLabel {
            id: an_core::new_id("intlabel"),
            finding_id: finding_id.into(),
            kind: kind.into(),
            note: note.into(),
            created_at: now(),
        };
        self.write().execute(
            "INSERT INTO introspection_labels (id, finding_id, kind, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![l.id, l.finding_id, l.kind, l.note, l.created_at],
        )?;
        Ok(l)
    }

    /// 某条结论的标注，按写入顺序。
    pub fn introspection_labels(&self, finding_id: &str) -> Result<Vec<IntrospectionLabel>> {
        let _timer = ReadTimer::new("introspection_labels");
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT id, finding_id, kind, note, created_at FROM introspection_labels
             WHERE finding_id = ?1 ORDER BY rowid",
        )?;
        let rows = st.query_map([finding_id], |r| {
            Ok(IntrospectionLabel {
                id: r.get(0)?,
                finding_id: r.get(1)?,
                kind: r.get(2)?,
                note: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        rows.map(|r| Ok(r?)).collect()
    }

    pub fn app_state_get(&self, key: &str) -> Result<Option<String>> {
        let _timer = ReadTimer::new("app_state_get");
        let conn = self.read();
        Ok(conn
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn app_state_set(&self, key: &str, value: &str) -> Result<()> {
        self.write().execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 个人主窗口会话 ID（尚未创建返回 None）。
    pub fn main_conversation_id(&self) -> Result<Option<String>> {
        self.app_state_get(MAIN_CONVERSATION_KEY)
    }

    /// 取得唯一的个人主窗口会话；不存在时创建。无论调用多少次都只有一个：
    /// “查状态 + 建会话 + 写状态”在同一个写事务里完成。
    pub fn ensure_main_conversation(&self, title: &str) -> Result<Conversation> {
        let mut conn = self.write();
        let tx = conn.transaction()?;
        let known: Option<String> = tx
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                [MAIN_CONVERSATION_KEY],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = known {
            let found = tx
                .query_row(
                    "SELECT id, title, created_at FROM conversations WHERE id = ?1",
                    [&id],
                    |r| {
                        Ok(Conversation {
                            id: r.get(0)?,
                            title: r.get(1)?,
                            created_at: r.get(2)?,
                        })
                    },
                )
                .optional()?;
            if let Some(c) = found {
                return Ok(c);
            }
        }
        let c = Conversation {
            id: an_core::new_id("conv"),
            title: title.into(),
            created_at: now(),
        };
        tx.execute(
            "INSERT INTO conversations (id, title, created_at) VALUES (?1, ?2, ?3)",
            params![c.id, c.title, c.created_at],
        )?;
        tx.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![MAIN_CONVERSATION_KEY, c.id],
        )?;
        tx.commit()?;
        Ok(c)
    }

    // ───────────── agent 准入（E3） ─────────────
    // 记录与审计事件以 JSON 保存，结构由 `crate::admission` 定义；这里只负责原子落库。

    /// 保存一条准入记录，并在同一事务里追加审计事件（按插入顺序）
    pub fn save_admission(
        &self,
        agent_id: &str,
        record_json: &str,
        events: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.write();
        let tx = conn.transaction()?;
        let at = now();
        tx.execute(
            "INSERT INTO agent_admission (agent_id, json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(agent_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            params![agent_id, record_json, at],
        )?;
        for (kind, detail) in events {
            tx.execute(
                "INSERT INTO admission_events (id, agent_id, kind, json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![an_core::new_id("adm"), agent_id, kind, detail, at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 全部准入记录的 JSON（按 agent_id 排序）
    pub fn list_admissions(&self) -> Result<Vec<String>> {
        let _timer = ReadTimer::new("list_admissions");
        let conn = self.read();
        let mut st = conn.prepare("SELECT json FROM agent_admission ORDER BY agent_id")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(r?)).collect()
    }

    /// 某个 agent 最早的 `limit` 条审计事件，按发生顺序
    pub fn admission_events(&self, agent_id: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let _timer = ReadTimer::new("admission_events");
        let conn = self.read();
        let mut st = conn.prepare(
            "SELECT id, kind, json, created_at FROM admission_events WHERE agent_id = ?1 ORDER BY rowid LIMIT ?2",
        )?;
        let rows = st.query_map(params![agent_id, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, kind, detail, created_at) = row?;
            Ok(serde_json::json!({
                "id": id, "agent_id": agent_id, "kind": kind,
                "detail": serde_json::from_str::<serde_json::Value>(&detail)?,
                "created_at": created_at,
            }))
        })
        .collect()
    }

    // ───────────── 路由日志 ─────────────

    pub fn insert_route_log(&self, log: &RouteLog) -> Result<()> {
        self.write().execute(
            "INSERT INTO route_logs (id, json, created_at) VALUES (?1, ?2, ?3)",
            params![log.id, serde_json::to_string(log)?, log.created_at],
        )?;
        Ok(())
    }

    pub fn list_route_logs(&self, limit: usize) -> Result<Vec<RouteLog>> {
        let _timer = ReadTimer::new("list_route_logs");
        let conn = self.read();
        // rowid 决胜：同一毫秒内写入的日志保持"新的在前"（与 messages 的排序约定一致）
        let mut st = conn
            .prepare("SELECT json FROM route_logs ORDER BY created_at DESC, rowid DESC LIMIT ?1")?;
        let rows = st.query_map([limit as i64], |r| r.get::<_, String>(0))?;
        rows.map(|j| Ok(serde_json::from_str(&j?)?)).collect()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // 先停写线程并等它把缓冲区写完，再关掉连接
        let _ = self.shared.cmd.send(WriteCmd::Stop);
        if let Some(handle) = lock_writer(&self.writer).take() {
            let _ = handle.join();
        }
    }
}

fn lock_writer(
    mutex: &Mutex<Option<std::thread::JoinHandle<()>>>,
) -> std::sync::MutexGuard<'_, Option<std::thread::JoinHandle<()>>> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

const TASK_SELECT: &str = "SELECT id, conversation_id, goal, role, status, route_json, policy_json, result_json, review_json, created_at, updated_at, assignment_json FROM tasks";

/// 把一行 tasks 记录还原成 TaskRecord（JSON 列解析失败时返回 Err）
fn task_from_row(r: &Row<'_>) -> rusqlite::Result<Result<TaskRecord>> {
    let role: String = r.get(3)?;
    let status: String = r.get(4)?;
    let route: String = r.get(5)?;
    let policy: String = r.get(6)?;
    let result: Option<String> = r.get(7)?;
    let review: Option<String> = r.get(8)?;
    let assignment: Option<String> = r.get(11)?;
    let (id, conversation_id, goal, created_at, updated_at): (
        String,
        String,
        String,
        String,
        String,
    ) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(9)?, r.get(10)?);
    Ok((|| -> Result<TaskRecord> {
        Ok(TaskRecord {
            id,
            conversation_id,
            goal,
            role: serde_json::from_value(serde_json::Value::String(role))?,
            status: TaskStatus::parse(&status).ok_or_else(|| anyhow!("未知任务状态 {status}"))?,
            route: serde_json::from_str(&route)?,
            policy: serde_json::from_str::<PolicyVerdict>(&policy)?,
            result: result.map(|s| serde_json::from_str(&s)).transpose()?,
            review: review.map(|s| serde_json::from_str(&s)).transpose()?,
            assignment: assignment.map(|s| serde_json::from_str(&s)).transpose()?,
            created_at,
            updated_at,
        })
    })())
}

const ACTIONS: &[&str] = &["respond", "clarify", "delegate", "follow_up"];

/// 用户指出的一次派发纠错。角色只在动作是 delegate 时有意义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingFeedback {
    pub id: String,
    pub message_id: String,
    pub original_action: String,
    pub original_role: Option<String>,
    pub correct_action: String,
    pub correct_role: Option<String>,
    pub note: String,
    pub created_at: String,
}

impl Store {
    /// 记下一次派发纠错。动作和角色不合法时拒绝，不写库。
    pub fn insert_feedback(&self, feedback: &RoutingFeedback) -> Result<()> {
        check_feedback(feedback)?;
        self.write().execute(
            "INSERT INTO routing_feedback (id, message_id, original_action, original_role, correct_action, correct_role, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                feedback.id, feedback.message_id, feedback.original_action, feedback.original_role,
                feedback.correct_action, feedback.correct_role, feedback.note, feedback.created_at,
            ],
        )?;
        Ok(())
    }

    /// 按字符二元组重合度取出最相似的纠正，最多 `limit` 条。没有重合的不返回。
    pub fn similar_feedback(
        &self,
        text: &str,
        limit: usize,
    ) -> Result<Vec<crate::prompts::CorrectionExample>> {
        let mine = char_grams(text);
        let conn = self.read();
        let mut stmt = conn.prepare(
            "SELECT substr(m.content, 1, 200), f.correct_action, f.correct_role, substr(f.note, 1, 200)
             FROM routing_feedback f JOIN messages m ON m.id = f.message_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut scored = Vec::new();
        for row in rows {
            let (content, action, role, note) = row?;
            let score = dice(&mine, &char_grams(&content));
            if score > 0.0 {
                scored.push((
                    score,
                    crate::prompts::CorrectionExample {
                        user_text: content,
                        correct_action: action,
                        correct_role: role,
                        note,
                    },
                ));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit.min(5));
        Ok(scored.into_iter().map(|(_, e)| e).collect())
    }

    /// 纠错草稿，格式与 `evals/routing.jsonl` 的一行相同。不写评测集文件。
    pub fn feedback_draft_jsonl(&self) -> Result<String> {
        let conn = self.read();
        let mut stmt = conn.prepare(
            "SELECT f.id, m.content, f.original_action, f.original_role, f.correct_action, f.correct_role, f.note
             FROM routing_feedback f JOIN messages m ON m.id = f.message_id
             ORDER BY f.created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut out = String::from("// 纠错草稿。确认后把样本抄进 evals/make_dataset.py，再生成 evals/routing.jsonl。不要直接把本文件并进去。\n");
        for row in rows {
            let (id, text, original_action, original_role, correct_action, correct_role, note) =
                row?;
            let was = match original_role.as_deref() {
                Some(role) if !role.is_empty() => format!("{original_action} / {role}"),
                _ => original_action,
            };
            let mut line = serde_json::json!({
                "id": id,
                "text": text,
                "tags": ["feedback"],
                "expect_action": correct_action,
                "note": format!("原判断 {was}。{note}"),
            });
            if let Some(role) = correct_role {
                line["expect_role"] = serde_json::Value::String(role);
            }
            out.push_str(&line.to_string());
            out.push('\n');
        }
        Ok(out)
    }

    pub fn list_feedback(&self) -> Result<Vec<RoutingFeedback>> {
        let conn = self.read();
        let mut stmt = conn.prepare(
            "SELECT id, message_id, original_action, original_role, correct_action, correct_role, note, created_at
             FROM routing_feedback ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RoutingFeedback {
                id: row.get(0)?,
                message_id: row.get(1)?,
                original_action: row.get(2)?,
                original_role: row.get(3)?,
                correct_action: row.get(4)?,
                correct_role: row.get(5)?,
                note: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn char_grams(text: &str) -> std::collections::HashSet<String> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 2 {
        return chars.into_iter().map(|c| c.to_string()).collect();
    }
    chars.windows(2).map(|w| w.iter().collect()).collect()
}

fn dice(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let hit = a.intersection(b).count() as f64;
    2.0 * hit / (a.len() + b.len()) as f64
}

fn check_feedback(feedback: &RoutingFeedback) -> Result<()> {
    if feedback.message_id.trim().is_empty() {
        anyhow::bail!("message_id 不能为空");
    }
    if !ACTIONS.contains(&feedback.original_action.as_str())
        || !ACTIONS.contains(&feedback.correct_action.as_str())
    {
        anyhow::bail!("动作只能是 respond、clarify、delegate、follow_up");
    }
    for role in [
        feedback.original_role.as_deref(),
        feedback.correct_role.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if an_core::Role::parse(role).is_none() {
            anyhow::bail!("未知角色 {role}");
        }
    }
    Ok(())
}

/// 个人主窗口会话 ID 在 `app_state` 里的键
const MAIN_CONVERSATION_KEY: &str = "main_conversation_id";

const FINDING_COLUMNS: &str =
    "SELECT id, job_id, topic, json, verification, gate_score, gate_reason,
        pushed, verdict, verdict_note, created_at, updated_at FROM introspection_findings";

fn finding_from_row(r: &Row) -> rusqlite::Result<IntrospectionFinding> {
    Ok(IntrospectionFinding {
        id: r.get(0)?,
        job_id: r.get(1)?,
        topic: r.get(2)?,
        json: r.get(3)?,
        verification: r.get(4)?,
        gate_score: r.get(5)?,
        gate_reason: r.get(6)?,
        pushed: r.get::<_, i64>(7)? != 0,
        verdict: r.get(8)?,
        verdict_note: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

/// 数据库中保存的节点
#[derive(Debug, Clone)]
pub struct StoredNode {
    pub node_id: String,
    pub name: String,
    pub address: String,
    pub token: String,
    pub cards: Vec<an_core::AgentCard>,
    pub paired_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧库（tasks 表没有 context 列）在 Store::open 时应自动补列，存量数据不受影响
    #[test]
    fn migrate_old_db_adds_context_column() {
        let dir = std::env::temp_dir().join(an_core::new_id("an-store-test"));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("old.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE tasks (
                    id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, goal TEXT NOT NULL,
                    role TEXT NOT NULL, status TEXT NOT NULL, route_json TEXT NOT NULL,
                    policy_json TEXT NOT NULL, result_json TEXT, review_json TEXT,
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tasks VALUES ('t1','c1','g','general','NEEDS_HUMAN','{}','{}',NULL,NULL,'2026-01-01','2026-01-01')",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&db).unwrap();
        // 旧行补列后 context 为空；get / set 正常
        assert_eq!(store.get_task_context("t1").unwrap(), Some(String::new()));
        store.set_task_context("t1", "背景").unwrap();
        assert_eq!(
            store.get_task_context("t1").unwrap().as_deref(),
            Some("背景")
        );
        // 重复 open（幂等）
        drop(store);
        Store::open(&db).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn events_after_returns_only_newer_rows() {
        use an_core::EventKind;
        let dir = std::env::temp_dir().join(an_core::new_id("an-store-events"));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        let ev = |id: &str| TaskEvent {
            id: id.into(),
            conversation_id: "c".into(),
            task_id: Some("t".into()),
            kind: EventKind::Progress,
            status: None,
            message: id.into(),
            stream_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        for id in ["e1", "e2", "e3"] {
            store.insert_event(&ev(id)).unwrap();
        }
        let ids = |after: Option<&str>| {
            store
                .list_events_after("t", after)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(None), vec!["e1", "e2", "e3"]);
        assert_eq!(ids(Some("e1")), vec!["e2", "e3"]);
        assert_eq!(ids(Some("e3")), Vec::<String>::new());
        assert_eq!(ids(Some("missing")), vec!["e1", "e2", "e3"]);
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn conversation_events_after_expires_missing_cursor() {
        use an_core::EventKind;
        let dir = std::env::temp_dir().join(an_core::new_id("an-store-desk-ev"));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        let ev = |id: &str, conv: &str| TaskEvent {
            id: id.into(),
            conversation_id: conv.into(),
            task_id: Some("t".into()),
            kind: EventKind::Progress,
            status: None,
            message: id.into(),
            stream_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        store.insert_event(&ev("e1", "c1")).unwrap();
        store.insert_event(&ev("e2", "c1")).unwrap();
        store.insert_event(&ev("e3", "c2")).unwrap();
        match store.list_conversation_events_after("c1", None).unwrap() {
            DesktopEventsAfter::Ok(v) => {
                assert_eq!(
                    v.iter().map(|x| x.cursor.as_str()).collect::<Vec<_>>(),
                    ["e1", "e2"]
                );
            }
            DesktopEventsAfter::CursorExpired => panic!("unexpected expire"),
        }
        match store
            .list_conversation_events_after("c1", Some("e1"))
            .unwrap()
        {
            DesktopEventsAfter::Ok(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].cursor, "e2");
            }
            DesktopEventsAfter::CursorExpired => panic!("unexpected expire"),
        }
        assert!(matches!(
            store
                .list_conversation_events_after("c1", Some("missing"))
                .unwrap(),
            DesktopEventsAfter::CursorExpired
        ));
        // 其他会话的游标对本会话视为过期
        assert!(matches!(
            store
                .list_conversation_events_after("c1", Some("e3"))
                .unwrap(),
            DesktopEventsAfter::CursorExpired
        ));
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn progress(task_id: &str, id: &str) -> TaskEvent {
        TaskEvent {
            id: id.into(),
            conversation_id: "c".into(),
            task_id: Some(task_id.into()),
            kind: an_core::EventKind::Progress,
            status: None,
            message: id.into(),
            stream_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn temp_db(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(an_core::new_id(prefix));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("t.db");
        (dir, db)
    }

    #[test]
    fn file_db_uses_normal_sync_and_a_read_only_connection() {
        let (dir, db) = temp_db("an-store-pragma");
        let store = Store::open(&db).unwrap();
        let sync: i64 = store
            .write()
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        let busy: i64 = store
            .write()
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        let query_only: i64 = store
            .read()
            .query_row("PRAGMA query_only", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 1, "synchronous=NORMAL");
        assert_eq!(busy, 5000);
        assert_eq!(query_only, 1, "读连接不应能写入");
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn queued_events_keep_order_and_flush_on_drop() {
        let (dir, db) = temp_db("an-store-order");
        {
            let store = Store::open(&db).unwrap();
            for i in 0..100 {
                store.enqueue_event(progress("t", &format!("e{i:04}")));
            }
            store.flush_events();
            let ids: Vec<_> = store
                .list_events("t")
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect();
            assert_eq!(ids.len(), 100);
            assert!(
                ids.windows(2).all(|w| w[0] < w[1]),
                "flush 之后顺序应与入队顺序一致"
            );
        }
        let store = Store::open(&db).unwrap();
        let ids: Vec<_> = store
            .list_events("t")
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids.len(), 100, "进程退出（Drop）要把缓冲区写完");
        assert_eq!(ids.first().map(String::as_str), Some("e0000"));
        assert_eq!(ids.last().map(String::as_str), Some("e0099"));
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn event_retention_keeps_the_newest_rows() {
        let (dir, db) = temp_db("an-store-keep");
        let store = Store::open(&db).unwrap();
        for i in 0..5 {
            store
                .insert_event(&progress("t", &format!("e{i}")))
                .unwrap();
        }
        trim_events(&store.write(), 3).unwrap();
        let ids: Vec<_> = store
            .list_events("t")
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, vec!["e2", "e3", "e4"]);
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn p95(samples: &mut [Duration]) -> Duration {
        assert!(!samples.is_empty(), "读延迟没有采到样本");
        samples.sort();
        samples[samples.len() * 95 / 100]
    }

    /// 8 个任务同时灌事件，读者只读一条固定的小任务和任务列表。
    /// 写入量扩大 10 倍时，读延迟不应跟着放大。
    #[test]
    fn read_p95_does_not_grow_with_write_volume() {
        let (dir, db) = temp_db("an-store-bench");
        let store = Arc::new(Store::open(&db).unwrap());
        for i in 0..5 {
            store
                .insert_event(&progress("probe", &format!("p{i}")))
                .unwrap();
        }

        let bench = |per_task: usize| -> Duration {
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let samples = Arc::new(Mutex::new(Vec::<Duration>::new()));
            let mut readers = Vec::new();
            for _ in 0..2 {
                let store = Arc::clone(&store);
                let done = Arc::clone(&done);
                let samples = Arc::clone(&samples);
                readers.push(std::thread::spawn(move || {
                    while !done.load(Ordering::Relaxed) {
                        let started = Instant::now();
                        store.list_tasks(None).unwrap();
                        store.list_events("probe").unwrap();
                        samples.lock().unwrap().push(started.elapsed());
                    }
                }));
            }
            let mut writers = Vec::new();
            for task_i in 0..8 {
                let store = Arc::clone(&store);
                writers.push(std::thread::spawn(move || {
                    let task_id = format!("w{per_task}-{task_i}");
                    for n in 0..per_task {
                        store.enqueue_event(progress(&task_id, &format!("{task_id}-{n:04}")));
                    }
                }));
            }
            for w in writers {
                w.join().unwrap();
            }
            done.store(true, Ordering::Relaxed);
            for r in readers {
                r.join().unwrap();
            }
            store.flush_events();
            let task_id = format!("w{per_task}-0");
            let ids: Vec<_> = store
                .list_events(&task_id)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect();
            assert_eq!(ids.len(), per_task, "任务 {task_id} 的事件应全部落库");
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "事件顺序不应打乱");
            let mut got = samples.lock().unwrap();
            let latency = p95(&mut got);
            eprintln!(
                "store bench events_per_task={per_task} samples={} p95={latency:?}",
                got.len()
            );
            latency
        };

        let small = bench(100);
        let big = bench(1000);
        assert!(
            small < Duration::from_millis(100),
            "小写入量读 P95 应低于 100ms，实际 {small:?}"
        );
        assert!(
            big < Duration::from_millis(100),
            "大写入量读 P95 应低于 100ms，实际 {big:?}"
        );
        let limit = small.saturating_mul(4) + Duration::from_millis(30);
        assert!(
            big < limit,
            "写入量 ×10 后读 P95 从 {small:?} 涨到 {big:?}，超过了 {limit:?}"
        );
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn sample_feedback() -> RoutingFeedback {
        RoutingFeedback {
            id: "fb-1".into(),
            message_id: "msg-1".into(),
            original_action: "delegate".into(),
            original_role: Some("coding".into()),
            correct_action: "respond".into(),
            correct_role: None,
            note: "只是在问概念".into(),
            created_at: "2026-11-05T00:00:00Z".into(),
        }
    }

    #[test]
    fn feedback_roundtrip() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store.insert_feedback(&sample_feedback()).unwrap();
        assert_eq!(store.list_feedback().unwrap(), vec![sample_feedback()]);
    }

    #[test]
    fn feedback_rejects_unknown_action_and_role() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut bad = sample_feedback();
        bad.correct_action = "guess".into();
        assert!(store.insert_feedback(&bad).is_err());
        bad = sample_feedback();
        bad.correct_role = Some("artist".into());
        assert!(store.insert_feedback(&bad).is_err());
        assert!(store.list_feedback().unwrap().is_empty());
    }

    #[test]
    fn similar_feedback_ranks_by_overlap() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let conv = store.create_conversation("t").unwrap();
        let code = store
            .add_message(&conv.id, "user", "在仓库里写一个 Python 斐波那契函数", None)
            .unwrap();
        let hello = store
            .add_message(&conv.id, "user", "写一个 Python 的当前时间脚本", None)
            .unwrap();
        let mut code_fb = sample_feedback();
        code_fb.id = "fb-code".into();
        code_fb.message_id = code.id;
        code_fb.correct_action = "delegate".into();
        code_fb.correct_role = Some("coding".into());
        let mut hello_fb = sample_feedback();
        hello_fb.id = "fb-hello".into();
        hello_fb.message_id = hello.id;
        hello_fb.correct_action = "respond".into();
        store.insert_feedback(&code_fb).unwrap();
        store.insert_feedback(&hello_fb).unwrap();

        let ranked = store
            .similar_feedback("请写一个 Python 斐波那契函数并附测试", 5)
            .unwrap();
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].correct_role.as_deref(), Some("coding"));
        assert_eq!(ranked[1].correct_action, "respond");
        assert!(store.similar_feedback("zzzz", 5).unwrap().is_empty());
    }

    #[test]
    fn feedback_draft_is_jsonl_and_does_not_touch_the_dataset() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let conv = store.create_conversation("t").unwrap();
        let msg = store
            .add_message(&conv.id, "user", "斐波那契是什么", None)
            .unwrap();
        let mut fb = sample_feedback();
        fb.message_id = msg.id;
        store.insert_feedback(&fb).unwrap();
        let draft = store.feedback_draft_jsonl().unwrap();
        let line = draft.lines().find(|l| !l.starts_with("//")).unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["text"], "斐波那契是什么");
        assert_eq!(v["expect_action"], "respond");
        assert!(v.get("expect_role").is_none());
        assert!(v["note"].as_str().unwrap().contains("delegate / coding"));
        assert!(draft.starts_with("// 纠错草稿"));
    }

    fn sample_task(id: &str, status: TaskStatus) -> TaskRecord {
        use an_core::{Risk, Role, RouteDecision, TaskClass};
        TaskRecord {
            id: id.into(),
            conversation_id: "c1".into(),
            goal: "g".into(),
            role: Role::General,
            status,
            route: RouteDecision {
                task_class: TaskClass::L1,
                execution_role: Role::General,
                required_tools: vec![],
                risk_level: Risk::Low,
                reason: String::new(),
            },
            policy: PolicyVerdict {
                auto_execute: true,
                needs_review: false,
                notes: vec![],
                candidates: vec![],
            },
            result: None,
            review: None,
            assignment: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    /// 状态转换只在当前状态属于 `from` 时生效
    #[test]
    fn transition_task_respects_from() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .insert_task(&sample_task("t1", TaskStatus::Queued), "")
            .unwrap();
        // 当前不是 RUNNING：不改
        assert!(!store
            .transition_task("t1", &[TaskStatus::Running], TaskStatus::Verifying)
            .unwrap());
        assert_eq!(
            store.get_task("t1").unwrap().unwrap().status,
            TaskStatus::Queued
        );
        // 当前是 QUEUED：改
        assert!(store
            .transition_task("t1", &[TaskStatus::Queued], TaskStatus::Running)
            .unwrap());
        assert_eq!(
            store.get_task("t1").unwrap().unwrap().status,
            TaskStatus::Running
        );
        // 不存在的任务：返回 false
        assert!(!store
            .transition_task("nope", &[TaskStatus::Running], TaskStatus::Cancelled)
            .unwrap());
    }

    /// 结束任务：状态和结果一起写；已进入终态的任务不会被改回去
    #[test]
    fn finish_task_writes_result_and_never_leaves_terminal() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .insert_task(&sample_task("t1", TaskStatus::Running), "")
            .unwrap();
        // 先被取消
        assert!(store
            .transition_task("t1", &ACTIVE_STATUSES, TaskStatus::Cancelled)
            .unwrap());
        // 再尝试写完成：失败，结果也不写入
        let done = TaskResult {
            status: an_core::WorkerStatus::Succeeded,
            summary: "ok".into(),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![],
        };
        assert!(!store
            .finish_task(
                "t1",
                &[TaskStatus::Running, TaskStatus::Verifying],
                TaskStatus::Completed,
                Some(&done)
            )
            .unwrap());
        let t = store.get_task("t1").unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Cancelled);
        assert!(t.result.is_none());

        store
            .insert_task(&sample_task("t2", TaskStatus::Running), "")
            .unwrap();
        assert!(store
            .finish_task(
                "t2",
                &[TaskStatus::Running],
                TaskStatus::Completed,
                Some(&done)
            )
            .unwrap());
        let t = store.get_task("t2").unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Completed);
        assert_eq!(t.result.unwrap().summary, "ok");
        // 终态之后再取消：失败
        assert!(!store
            .transition_task("t2", &ACTIVE_STATUSES, TaskStatus::Cancelled)
            .unwrap());
    }

    /// 排队名次：只数同角色、比自己早创建、状态是 QUEUED 的任务
    #[test]
    fn queued_ahead_counts_only_earlier_tasks() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut a = sample_task("a", TaskStatus::Queued);
        a.created_at = "2026-01-01T00:00:01.000Z".into();
        let mut b = sample_task("b", TaskStatus::Queued);
        b.created_at = "2026-01-01T00:00:02.000Z".into();
        let mut c = sample_task("c", TaskStatus::Running);
        c.created_at = "2026-01-01T00:00:00.000Z".into();
        for t in [&a, &b, &c] {
            store.insert_task(t, "").unwrap();
        }
        assert_eq!(store.queued_ahead("a").unwrap(), 0);
        assert_eq!(store.queued_ahead("b").unwrap(), 1);
    }

    // ───────────── 业务内省（INT）存储层，S1 ─────────────
    // 对应 docs/memory/ACCEPTANCE.md 的 RT-INT-09（幂等）、RT-INT-26/27 的存储部分。

    const FUTURE: &str = "9999-12-31T00:00:00.000Z";
    const PAST: &str = "2000-01-01T00:00:00.000Z";

    fn finding(id: &str, json: &str) -> IntrospectionFinding {
        IntrospectionFinding {
            id: id.into(),
            job_id: "job-1".into(),
            topic: "adoption_pattern".into(),
            json: json.into(),
            verification: "supported".into(),
            gate_score: None,
            gate_reason: String::new(),
            pushed: false,
            verdict: None,
            verdict_note: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn file_store(tag: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(an_core::new_id(tag));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        (dir, store)
    }

    /// 同一时刻只能有一个运行租约；结束后可以再抢；租约过期的 running 任务不再挡路
    #[test]
    fn int_store_lease_is_exclusive_until_finished_or_expired() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let a = store
            .introspection_acquire_job("schedule", "{}", "h1", FUTURE)
            .unwrap()
            .expect("第一次应拿到租约");
        assert_eq!(a.state, "running");
        assert_eq!(a.lease_until.as_deref(), Some(FUTURE));
        assert!(
            store
                .introspection_acquire_job("manual", "{}", "h2", FUTURE)
                .unwrap()
                .is_none(),
            "租约未过期时不能再抢"
        );
        assert!(store.introspection_finish_job(&a.id, "done", None).unwrap());
        let done = store.introspection_get_job(&a.id).unwrap().unwrap();
        assert_eq!(done.state, "done");
        assert!(done.lease_until.is_none(), "结束后释放租约");
        assert!(done.finished_at.is_some());
        let b = store
            .introspection_acquire_job("manual", "{}", "h2", PAST)
            .unwrap();
        assert!(b.is_some(), "上一个任务结束后可以再抢");
        assert!(
            store
                .introspection_acquire_job("schedule", "{}", "h3", FUTURE)
                .unwrap()
                .is_some(),
            "已过期租约的 running 任务不挡新任务"
        );
    }

    /// 崩溃遗留的 running 且租约已过期的任务被标为 failed；租约有效的不动
    #[test]
    fn int_store_stale_running_jobs_become_failed() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let stale = store
            .introspection_acquire_job("schedule", "{}", "h1", PAST)
            .unwrap()
            .unwrap();
        assert_eq!(store.introspection_fail_stale_jobs().unwrap(), 1);
        let j = store.introspection_get_job(&stale.id).unwrap().unwrap();
        assert_eq!(j.state, "failed");
        assert!(j.error.unwrap_or_default().contains("interrupted"));
        assert!(j.finished_at.is_some());
        let live = store
            .introspection_acquire_job("schedule", "{}", "h2", FUTURE)
            .unwrap()
            .unwrap();
        assert_eq!(store.introspection_fail_stale_jobs().unwrap(), 0);
        assert_eq!(
            store
                .introspection_get_job(&live.id)
                .unwrap()
                .unwrap()
                .state,
            "running"
        );
    }

    /// 水位只跟着成功的任务走：失败、取消的任务不推进，也不回退
    #[test]
    fn int_store_watermark_advances_only_on_done_jobs() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        assert_eq!(store.introspection_latest_watermark().unwrap(), None);
        let j1 = store
            .introspection_acquire_job("schedule", r#"{"mom":5}"#, "h1", FUTURE)
            .unwrap()
            .unwrap();
        store
            .introspection_finish_job(&j1.id, "done", None)
            .unwrap();
        let j2 = store
            .introspection_acquire_job("schedule", r#"{"mom":9}"#, "h2", FUTURE)
            .unwrap()
            .unwrap();
        store
            .introspection_finish_job(&j2.id, "failed", Some("超时"))
            .unwrap();
        assert_eq!(
            store.introspection_latest_watermark().unwrap().as_deref(),
            Some(r#"{"mom":5}"#)
        );
        let failed = store.introspection_get_job(&j2.id).unwrap().unwrap();
        assert_eq!(failed.error.as_deref(), Some("超时"));
    }

    /// RT-INT-09 存储部分：同一稳定 ID 重复写入不产生第二行，也不覆盖已有内容
    #[test]
    fn int_store_finding_insert_is_idempotent() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        assert!(store
            .introspection_insert_finding(&finding("f1", r#"{"rate":0.6}"#))
            .unwrap());
        assert!(!store
            .introspection_insert_finding(&finding("f1", r#"{"rate":0.1}"#))
            .unwrap());
        let all = store.introspection_list_findings(None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].json, r#"{"rate":0.6}"#);
        assert_eq!(store.introspection_get_finding("nope").unwrap(), None);
    }

    /// 列表按创建时间倒序，可按主题过滤，受 limit 限制
    #[test]
    fn int_store_list_findings_orders_filters_and_limits() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for (id, topic, at) in [
            ("f1", "adoption_pattern", "2026-01-01T00:00:01.000Z"),
            ("f2", "other_topic", "2026-01-01T00:00:02.000Z"),
            ("f3", "adoption_pattern", "2026-01-01T00:00:03.000Z"),
        ] {
            let mut f = finding(id, "{}");
            f.topic = topic.into();
            f.created_at = at.into();
            store.introspection_insert_finding(&f).unwrap();
        }
        let ids = |v: Vec<IntrospectionFinding>| v.into_iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store.introspection_list_findings(None, 10).unwrap()),
            ["f3", "f2", "f1"]
        );
        assert_eq!(
            ids(store
                .introspection_list_findings(Some("adoption_pattern"), 10)
                .unwrap()),
            ["f3", "f1"]
        );
        assert_eq!(
            ids(store.introspection_list_findings(None, 1).unwrap()),
            ["f3"]
        );
    }

    /// 闸门原因（推或不推都记）和运营负责人的判断都能写入并读回；不存在的 ID 返回 false
    #[test]
    fn int_store_gate_and_verdict_are_recorded() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .introspection_insert_finding(&finding("f1", "{}"))
            .unwrap();
        assert!(store
            .introspection_set_gate("f1", 3.5, "分数 3.5 低于阈值 4，不推送", false)
            .unwrap());
        let f = store.introspection_get_finding("f1").unwrap().unwrap();
        assert_eq!(f.gate_score, Some(3.5));
        assert_eq!(f.gate_reason, "分数 3.5 低于阈值 4，不推送");
        assert!(!f.pushed);
        assert!(store
            .introspection_set_verdict("f1", "rejected", "样本太少")
            .unwrap());
        let f = store.introspection_get_finding("f1").unwrap().unwrap();
        assert_eq!(f.verdict.as_deref(), Some("rejected"));
        assert_eq!(f.verdict_note.as_deref(), Some("样本太少"));
        assert!(!store
            .introspection_set_gate("nope", 1.0, "x", false)
            .unwrap());
        assert!(!store
            .introspection_set_verdict("nope", "confirmed", "")
            .unwrap());
    }

    /// 运行时标注按写入顺序读回，互不覆盖
    #[test]
    fn int_store_labels_keep_order() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .introspection_add_label("f1", "should_push", "很有用")
            .unwrap();
        store
            .introspection_add_label("f1", "should_not_push", "")
            .unwrap();
        let kinds: Vec<_> = store
            .introspection_labels("f1")
            .unwrap()
            .into_iter()
            .map(|l| l.kind)
            .collect();
        assert_eq!(kinds, ["should_push", "should_not_push"]);
        assert!(store.introspection_labels("other").unwrap().is_empty());
    }

    /// RT-INT-27 存储部分：未读数 2 → 标记后 0；重启后仍为 0；同一结论只推一次
    #[test]
    fn int_store_unread_counts_persist_across_restart() {
        let (dir, store) = file_store("an-int-unread");
        assert!(store.introspection_add_push("f1", "m1").unwrap().is_some());
        assert!(store.introspection_add_push("f2", "m2").unwrap().is_some());
        assert!(
            store.introspection_add_push("f1", "m3").unwrap().is_none(),
            "同一条结论不重复推送"
        );
        assert_eq!(store.introspection_unread_count().unwrap(), 2);
        assert_eq!(store.introspection_mark_all_read().unwrap(), 2);
        assert_eq!(store.introspection_unread_count().unwrap(), 0);
        assert_eq!(
            store.introspection_mark_all_read().unwrap(),
            0,
            "没有未读时标记不报错、条数为 0"
        );
        drop(store);
        let store = Store::open(&dir.join("t.db")).unwrap();
        assert_eq!(store.introspection_unread_count().unwrap(), 0);
        assert!(store.introspection_add_push("f3", "m4").unwrap().is_some());
        assert_eq!(store.introspection_unread_count().unwrap(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 普通聊天消息不计入内省未读数
    #[test]
    fn int_store_chat_messages_do_not_count_as_unread() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let c = store.create_conversation("普通").unwrap();
        store.add_message(&c.id, "assistant", "你好", None).unwrap();
        assert_eq!(store.introspection_unread_count().unwrap(), 0);
    }

    #[test]
    fn int_store_app_state_roundtrip_and_overwrite() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        assert_eq!(store.app_state_get("k").unwrap(), None);
        store.app_state_set("k", "v1").unwrap();
        store.app_state_set("k", "v2").unwrap();
        assert_eq!(store.app_state_get("k").unwrap().as_deref(), Some("v2"));
    }

    /// RT-INT-26 存储部分：主窗口会话唯一；重复确保不新建；其他会话的增改不影响；重启后仍是它
    #[test]
    fn int_store_main_conversation_is_unique_and_stable() {
        let (dir, store) = file_store("an-int-main");
        store.create_conversation("普通 A").unwrap();
        assert_eq!(store.main_conversation_id().unwrap(), None);
        let before = store.list_conversations().unwrap().len();
        let m1 = store.ensure_main_conversation("主窗口").unwrap();
        let m2 = store.ensure_main_conversation("主窗口").unwrap();
        assert_eq!(m1.id, m2.id, "只能有一个主窗口会话");
        assert_eq!(store.list_conversations().unwrap().len(), before + 1);
        assert_eq!(
            store.main_conversation_id().unwrap().as_deref(),
            Some(m1.id.as_str())
        );
        let other = store.create_conversation("普通 B").unwrap();
        store.rename_conversation(&other.id, "改名").unwrap();
        assert_eq!(
            store.ensure_main_conversation("主窗口").unwrap().id,
            m1.id,
            "其他会话的增改不影响主窗口会话"
        );
        drop(store);
        let store = Store::open(&dir.join("t.db")).unwrap();
        assert_eq!(
            store.ensure_main_conversation("主窗口").unwrap().id,
            m1.id,
            "重启后仍是同一个会话"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
