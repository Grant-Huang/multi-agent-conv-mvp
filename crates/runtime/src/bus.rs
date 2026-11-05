//! 事件总线：先广播给 SSE 订阅者，再把事件放进存储的写队列。
//! 落库由专用线程批量完成，广播不等待写库。界面漏收时仍可从数据库补拉。

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
            stream_id: None,
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
            stream_id: None,
            created_at: an_core::now(),
        });
    }
}

impl Bus {
    /// 推送一段流式回复：只广播给界面，不写数据库（片段很多，写库没有意义；最终回复会作为消息保存）
    pub fn assistant_delta(&self, conversation_id: &str, stream_id: &str, delta: &str) {
        let _ = self.tx.send(TaskEvent {
            id: an_core::new_id("evt"),
            conversation_id: conversation_id.into(),
            task_id: None,
            kind: EventKind::AssistantDelta,
            status: None,
            message: delta.into(),
            stream_id: Some(stream_id.into()),
            created_at: an_core::now(),
        });
    }
}

impl EventPublisher for Bus {
    fn publish(&self, event: TaskEvent) {
        // 没有订阅者时 send 会返回 Err，属于正常情况。先广播，再入队：
        // 通道满时入队会堵住，但界面已经收到，顺序也不会和别的发布方交错错乱。
        let _ = self.tx.send(event.clone());
        self.store.enqueue_event(event);
    }
}
