//! SQLite 持久化：会话、消息、任务、事件、路由日志。
//!
//! 任务状态由 AgentNexus 自己保存（而不是放在 Harness 内存里），
//! 服务重启后仍能看到任务历史。MVP 用一个带锁的连接，查询都很短，够用；
//! 以后需要并发或多实例时换 Postgres（sqlx）即可，上层接口不变。

use std::path::Path;
use std::sync::Mutex;

use an_core::{
    now, Conversation, Message, PolicyVerdict, ReviewVerdict, RouteLog, TaskEvent, TaskRecord, TaskResult, TaskStatus,
};
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

pub struct Store {
    conn: Mutex<Connection>,
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
    updated_at TEXT NOT NULL
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

impl Store {
    /// 打开（或创建）数据库文件；传入 `:memory:` 用于测试
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("无法打开数据库 {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // 锁中毒说明之前有线程 panic，直接继续使用（数据由 SQLite 保证一致）
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ───────────── 会话 ─────────────

    pub fn create_conversation(&self, title: &str) -> Result<Conversation> {
        let c = Conversation { id: an_core::new_id("conv"), title: title.into(), created_at: now() };
        self.conn().execute(
            "INSERT INTO conversations (id, title, created_at) VALUES (?1, ?2, ?3)",
            params![c.id, c.title, c.created_at],
        )?;
        Ok(c)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        Ok(self
            .conn()
            .query_row("SELECT id, title, created_at FROM conversations WHERE id = ?1", [id], |r| {
                Ok(Conversation { id: r.get(0)?, title: r.get(1)?, created_at: r.get(2)? })
            })
            .optional()?)
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT id, title, created_at FROM conversations ORDER BY created_at DESC")?;
        let rows = st.query_map([], |r| Ok(Conversation { id: r.get(0)?, title: r.get(1)?, created_at: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<()> {
        self.conn().execute("UPDATE conversations SET title = ?2 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    // ───────────── 消息 ─────────────

    pub fn add_message(&self, conversation_id: &str, role: &str, content: &str, task_id: Option<&str>) -> Result<Message> {
        let m = Message {
            id: an_core::new_id("msg"),
            conversation_id: conversation_id.into(),
            role: role.into(),
            content: content.into(),
            task_id: task_id.map(String::from),
            created_at: now(),
        };
        self.conn().execute(
            "INSERT INTO messages (id, conversation_id, role, content, task_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![m.id, m.conversation_id, m.role, m.content, m.task_id, m.created_at],
        )?;
        Ok(m)
    }

    /// 按时间顺序返回消息；`last` 为 Some(n) 时只返回最近 n 条
    pub fn list_messages(&self, conversation_id: &str, last: Option<usize>) -> Result<Vec<Message>> {
        let conn = self.conn();
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

    pub fn insert_task(&self, t: &TaskRecord) -> Result<()> {
        self.conn().execute(
            "INSERT INTO tasks (id, conversation_id, goal, role, status, route_json, policy_json, result_json, review_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            ],
        )?;
        Ok(())
    }

    pub fn set_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.conn().execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now()],
        )?;
        Ok(())
    }

    pub fn set_task_result(&self, id: &str, result: &TaskResult) -> Result<()> {
        self.conn().execute(
            "UPDATE tasks SET result_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(result)?, now()],
        )?;
        Ok(())
    }

    pub fn set_task_review(&self, id: &str, review: &ReviewVerdict) -> Result<()> {
        self.conn().execute(
            "UPDATE tasks SET review_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, serde_json::to_string(review)?, now()],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!("{TASK_SELECT} WHERE id = ?1"))?;
        let rec = st.query_row([id], task_from_row).optional()?;
        rec.transpose()
    }

    /// 列出任务；`conversation_id` 为 None 时返回全部
    pub fn list_tasks(&self, conversation_id: Option<&str>) -> Result<Vec<TaskRecord>> {
        let conn = self.conn();
        let rows: Vec<Result<TaskRecord>> = match conversation_id {
            Some(c) => {
                let mut st = conn.prepare(&format!("{TASK_SELECT} WHERE conversation_id = ?1 ORDER BY created_at DESC"))?;
                let r = st.query_map([c], task_from_row)?.collect::<rusqlite::Result<_>>()?;
                r
            }
            None => {
                let mut st = conn.prepare(&format!("{TASK_SELECT} ORDER BY created_at DESC LIMIT 200"))?;
                let r = st.query_map([], task_from_row)?.collect::<rusqlite::Result<_>>()?;
                r
            }
        };
        rows.into_iter().collect()
    }

    /// 会话中尚未结束的任务（给 Turn Controller 判断 follow_up 用）
    pub fn active_tasks(&self, conversation_id: &str) -> Result<Vec<TaskRecord>> {
        Ok(self.list_tasks(Some(conversation_id))?.into_iter().filter(|t| !t.status.is_terminal()).collect())
    }

    /// 服务启动时调用：把上次运行中断的任务标记为失败（MVP 不做断点续跑）
    pub fn fail_interrupted_tasks(&self) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE tasks SET status = 'FAILED', updated_at = ?1 WHERE status IN ('CREATED', 'QUEUED', 'RUNNING', 'VERIFYING')",
            [now()],
        )?)
    }

    // ───────────── 事件 ─────────────

    pub fn insert_event(&self, e: &TaskEvent) -> Result<()> {
        self.conn().execute(
            "INSERT INTO events (id, conversation_id, task_id, json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.id, e.conversation_id, e.task_id, serde_json::to_string(e)?, e.created_at],
        )?;
        Ok(())
    }

    pub fn list_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT json FROM events WHERE task_id = ?1 ORDER BY created_at, rowid")?;
        let rows = st.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.map(|j| Ok(serde_json::from_str(&j?)?)).collect()
    }

    // ───────────── 节点 ─────────────

    /// 保存（或更新）配对的节点。`cards` 是节点最后一次上报的 agent 列表，节点离线时用它显示
    pub fn upsert_node(&self, n: &StoredNode) -> Result<()> {
        self.conn().execute(
            "INSERT INTO nodes (node_id, name, address, token, cards_json, paired_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(node_id) DO UPDATE SET name = ?2, address = ?3, token = ?4, cards_json = ?5",
            params![n.node_id, n.name, n.address, n.token, serde_json::to_string(&n.cards)?, n.paired_at],
        )?;
        Ok(())
    }

    pub fn update_node_cards(&self, node_id: &str, cards: &[an_core::AgentCard]) -> Result<()> {
        self.conn().execute("UPDATE nodes SET cards_json = ?2 WHERE node_id = ?1", params![node_id, serde_json::to_string(cards)?])?;
        Ok(())
    }

    pub fn list_nodes(&self) -> Result<Vec<StoredNode>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT node_id, name, address, token, cards_json, paired_at FROM nodes ORDER BY paired_at")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))
        })?;
        rows.map(|row| {
            let (node_id, name, address, token, cards, paired_at) = row?;
            Ok(StoredNode { node_id, name, address, token, cards: serde_json::from_str(&cards)?, paired_at })
        })
        .collect()
    }

    pub fn delete_node(&self, node_id: &str) -> Result<bool> {
        Ok(self.conn().execute("DELETE FROM nodes WHERE node_id = ?1", [node_id])? > 0)
    }

    // ───────────── 路由日志 ─────────────

    pub fn insert_route_log(&self, log: &RouteLog) -> Result<()> {
        self.conn().execute(
            "INSERT INTO route_logs (id, json, created_at) VALUES (?1, ?2, ?3)",
            params![log.id, serde_json::to_string(log)?, log.created_at],
        )?;
        Ok(())
    }

    pub fn list_route_logs(&self, limit: usize) -> Result<Vec<RouteLog>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT json FROM route_logs ORDER BY created_at DESC LIMIT ?1")?;
        let rows = st.query_map([limit as i64], |r| r.get::<_, String>(0))?;
        rows.map(|j| Ok(serde_json::from_str(&j?)?)).collect()
    }
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
    let (id, conversation_id, goal, created_at, updated_at): (String, String, String, String, String) =
        (r.get(0)?, r.get(1)?, r.get(2)?, r.get(9)?, r.get(10)?);
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
