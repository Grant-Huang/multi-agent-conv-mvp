//! 事件总线：事件先写库（可回放），再广播给 SSE 订阅者。
//! 借鉴"任务事件 + 持久化状态 + 回调"的思路：Conversation 不轮询任务状态，而是订阅事件。

use std::sync::Arc;

use an_core::{EventKind, EventPublisher, TaskEvent, TaskStatus};
use tokio::sync::broadcast;

use crate::store::Store;

pub struct Bus {
    tx: broadcast::Sender<TaskEvent>,
    store: Arc<Store>,
}

impl Bus {
    pub fn new(store: Arc<Store>) -> Self {
        // 容量：订阅者处理太慢时会丢掉最旧的事件（前端可通过 API 补拉）
        let (tx, _) = broadcast::channel(1024);
        Self { tx, store }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.tx.subscribe()
    }

    /// 便捷方法：发布状态变化事件
    pub fn status(&self, conversation_id: &str, task_id: &str, status: TaskStatus, message: impl Into<String>) {
        let message = message.into();
        // 埋点：每次状态变化一条，排查时按 task_id 过滤即可还原完整时间线
        match status {
            TaskStatus::Failed => tracing::error!(event = "task.status", conv_id = %conversation_id, task_id = %task_id, status = status.as_str(), detail = %message),
            TaskStatus::NeedsHuman => tracing::warn!(event = "task.status", conv_id = %conversation_id, task_id = %task_id, status = status.as_str(), detail = %message),
            _ => tracing::info!(event = "task.status", conv_id = %conversation_id, task_id = %task_id, status = status.as_str(), detail = %message),
        }
        self.publish(TaskEvent {
            id: an_core::new_id("evt"),
            conversation_id: conversation_id.into(),
            task_id: Some(task_id.into()),
            kind: EventKind::StatusChanged,
            status: Some(status),
            message,
            created_at: an_core::now(),
        });
    }

    /// 便捷方法：通知前端会话中有新的助手消息
    pub fn assistant_message(&self, conversation_id: &str, task_id: Option<&str>, content: &str) {
        self.publish(TaskEvent {
            id: an_core::new_id("evt"),
            conversation_id: conversation_id.into(),
            task_id: task_id.map(String::from),
            kind: EventKind::AssistantMessage,
            status: None,
            message: content.into(),
            created_at: an_core::now(),
        });
    }
}

impl EventPublisher for Bus {
    fn publish(&self, event: TaskEvent) {
        if let Err(e) = self.store.insert_event(&event) {
            tracing::error!(error = %e, "事件写库失败");
        }
        // 没有订阅者时 send 会返回 Err，属于正常情况
        let _ = self.tx.send(event);
    }
}
