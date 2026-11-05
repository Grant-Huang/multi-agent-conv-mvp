//! 假的 ACP agent：用于测试，也用于新手零依赖体验 ACP 接入。
//!
//! 它通过 stdio 说 ACP 协议，收到任务后模拟一次完整的 agent 行为：
//! 1. 报告一个"写文件"工具调用；
//! 2. 向 client 请求权限；
//! 3. 获准后在工作目录写入 `hello.txt`，并以 diff 形式报告改动；
//! 4. 回复一段文字，正常结束。
//!
//! 特殊指令（便于测试）：任务描述含"慢"时先睡 30 秒；含"拒绝"时以 refusal 结束。
//!
//! 体验方式（config.toml）：
//! ```toml
//! [[acp.agents]]
//! id = "mock"
//! command = "./target/release/an-mock-acp"
//! role = "coding"
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol as acp;
use acp::v1::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, Diff, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
    PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, SessionNotification, SessionUpdate, StopReason, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use acp::{Client, Stdio};

/// 会话 id → 工作目录
type Sessions = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 取出提示词中的纯文本
fn prompt_text(req: &PromptRequest) -> String {
    req.prompt
        .iter()
        .filter_map(|b| if let ContentBlock::Text(t) = b { Some(t.text.clone()) } else { None })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 模拟一次任务执行。在独立任务中运行：这里需要等待 client 对权限请求的应答，
/// 不能在请求回调里直接等（会阻塞消息循环导致死锁）。
async fn run_prompt(cx: acp::v1::ConnectionTo<Client>, session_id: SessionId, cwd: PathBuf, text: String) -> acp::Result<StopReason> {
    if text.contains("慢") {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    if text.contains("拒绝") {
        return Ok(StopReason::Refusal);
    }
    let notify = |update: SessionUpdate| cx.send_notification(SessionNotification::new(session_id.clone(), update));

    // 1. 报告工具调用
    let path = cwd.join("hello.txt");
    notify(SessionUpdate::ToolCall(
        ToolCall::new("call-1", "写入 hello.txt").kind(ToolKind::Edit).status(ToolCallStatus::Pending),
    ))?;

    // 2. 请求权限
    let request = RequestPermissionRequest::new(
        session_id.clone(),
        ToolCallUpdate::new("call-1", ToolCallUpdateFields::new().title("写入 hello.txt").kind(ToolKind::Edit)),
        vec![
            PermissionOption::new("allow", "允许", PermissionOptionKind::AllowOnce),
            PermissionOption::new("reject", "拒绝", PermissionOptionKind::RejectOnce),
        ],
    );
    let answer = cx.send_request(request).block_task().await?;
    let allowed = matches!(&answer.outcome, RequestPermissionOutcome::Selected(s) if s.option_id.to_string() == "allow");

    // 3. 执行并报告结果
    let content = format!("hello from mock ACP agent\n任务：{}\n", text.lines().next().unwrap_or(""));
    if allowed {
        std::fs::write(&path, &content).map_err(|e| acp::Error::internal_error().data(e.to_string()))?;
        notify(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "call-1",
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .content(vec![ToolCallContent::Diff(Diff::new(path.clone(), content))]),
        )))?;
    } else {
        notify(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("call-1", ToolCallUpdateFields::new().status(ToolCallStatus::Failed))))?;
    }

    // 4. 回复文字
    let reply = if allowed { "已创建 hello.txt（来自模拟 ACP agent）" } else { "没有获得写文件权限，未做任何修改" };
    notify(SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(reply)))))?;
    Ok(StopReason::EndTurn)
}

#[tokio::main]
async fn main() -> acp::Result<()> {
    let sessions: Sessions = Arc::default();
    let (s_new, s_prompt) = (sessions.clone(), sessions.clone());

    acp::Agent
        .v1()
        .name("an-mock-acp")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(InitializeResponse::new(req.protocol_version).agent_capabilities(AgentCapabilities::new()))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, _cx| {
                let id = format!("mock-{}", std::process::id());
                s_new.lock().unwrap().insert(id.clone(), req.cwd.clone());
                responder.respond(NewSessionResponse::new(SessionId::new(id)))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let cwd = s_prompt.lock().unwrap().get(&req.session_id.to_string()).cloned().unwrap_or_else(|| PathBuf::from("."));
                let text = prompt_text(&req);
                let session_id = req.session_id.clone();
                let task_cx = cx.clone();
                cx.spawn(async move {
                    let stop = run_prompt(task_cx, session_id, cwd, text).await?;
                    responder.respond(PromptResponse::new(stop))
                })
            },
            acp::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}
