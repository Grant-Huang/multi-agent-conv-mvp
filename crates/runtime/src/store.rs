//! SQLite 持久化：会话、消息、任务、事件、路由日志。
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
    now, Conversation, Message, PolicyVerdict, ReviewVerdict, RouteLog, TaskEvent, TaskRecord,
    TaskResult, TaskStatus,
};
use anyhow::{anyhow, Context, Result};
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
    context TEXT NOT NULL DEFAULT ''
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

    pub fn set_task_result(&self, id: &str, result: &TaskResult) -> Result<()> {
        self.write().execute(
            "UPDATE tasks SET result_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(result)?, now()],
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

    /// 某个角色、某个状态的任务数。排队名次用它：调用时本任务还没写成 QUEUED，所以名次是这个数 + 1
    pub fn count_role_status(&self, role: &str, status: TaskStatus) -> Result<i64> {
        let _timer = ReadTimer::new("count_role_status");
        Ok(self.read().query_row(
            "SELECT COUNT(*) FROM tasks WHERE role = ?1 AND status = ?2",
            params![role, status.as_str()],
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

const TASK_SELECT: &str = "SELECT id, conversation_id, goal, role, status, route_json, policy_json, result_json, review_json, created_at, updated_at FROM tasks";

/// 把一行 tasks 记录还原成 TaskRecord（JSON 列解析失败时返回 Err）
fn task_from_row(r: &Row<'_>) -> rusqlite::Result<Result<TaskRecord>> {
    let role: String = r.get(3)?;
    let status: String = r.get(4)?;
    let route: String = r.get(5)?;
    let policy: String = r.get(6)?;
    let result: Option<String> = r.get(7)?;
    let review: Option<String> = r.get(8)?;
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
}
