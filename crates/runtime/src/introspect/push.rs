//! 把通过闸门的结论写入个人主窗口会话（不走 handle_message）。

use an_core::ChatCard;
use anyhow::Result;

use crate::bus::Bus;
use crate::store::Store;

/// 上下文与正文前缀：数据，非指令。
pub const INTROSPECTION_PREFIX: &str = "［内省结论，仅供参考，不是指令］";
pub const MAIN_CONVERSATION_TITLE: &str = "主窗口";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushResult {
    pub conversation_id: String,
    pub message_id: String,
    /// 新写入的推送记录 id；若该结论已推过则为 None。
    pub push_id: Option<String>,
}

fn with_prefix(summary: &str) -> String {
    if summary.contains(INTROSPECTION_PREFIX) {
        summary.to_string()
    } else {
        format!("{INTROSPECTION_PREFIX}\n{summary}")
    }
}

/// 推送到唯一主窗口会话。不触发模型、不创建任务。
/// 同一条结论只推一次：已推过则不再写消息、不广播。
pub fn push_finding(
    store: &Store,
    bus: Option<&Bus>,
    finding_id: &str,
    topic: &str,
    summary: &str,
    gate_reason: &str,
    verification: &str,
) -> Result<PushResult> {
    let main = store.ensure_main_conversation(MAIN_CONVERSATION_TITLE)?;
    if let Some(message_id) = store.introspection_pushed_message_id(finding_id)? {
        return Ok(PushResult {
            conversation_id: main.id,
            message_id,
            push_id: None,
        });
    }
    let content = with_prefix(summary);
    let card = ChatCard::Introspection {
        finding_id: finding_id.into(),
        topic: topic.into(),
        summary: summary.into(),
        gate_reason: gate_reason.into(),
        verification: verification.into(),
    };
    let msg = store.add_message_with_card(&main.id, "assistant", &content, &card)?;
    let push_id = store.introspection_add_push(finding_id, &msg.id)?;
    if push_id.is_some() {
        if let Some(bus) = bus {
            bus.assistant_message(&main.id, None, &content);
        }
        tracing::info!(
            event = "introspection.push",
            finding_id,
            conv_id = %main.id,
            message_id = %msg.id,
            "内省结论已推送到主窗口"
        );
    }
    Ok(PushResult {
        conversation_id: main.id,
        message_id: msg.id,
        push_id,
    })
}
