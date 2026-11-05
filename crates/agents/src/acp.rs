//! 通用 ACP（Agent Client Protocol）适配器：一个适配器接入所有支持 ACP 的 agent。
//!
//! ACP 中 AgentNexus 是 **client**（类似编辑器），外部 agent（opencode、Hermes 等）是 **agent**：
//! AgentNexus 以子进程方式启动它，通过 stdio 上的 JSON-RPC 通信。
//!
//! 一次任务的流程：
//! ```text
//! 启动子进程 → initialize → session/new(cwd=工作目录) → session/prompt(任务描述)
//!     ↑ 期间 agent 推送 session/update：消息片段、工具调用、计划、diff …  → 进度事件 / 证据
//!     ↑ 期间 agent 发来 session/request_permission                    → 按权限模式自动应答
//! ← 返回 stop_reason（end_turn 视为执行成功）→ 收集 git 改动 → TaskResult
//! ```
//!
//! 取消：编排层 abort 任务时连接被 drop，SDK 会杀掉 agent 的整个进程组。
//! 新手接入：常见 agent 已内置预设（见 [`presets`]），装好即被自动发现，无需配置。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol as acp;
use acp::schema::ProtocolVersion;
use acp::v1::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    StopReason, TextContent, ToolCallContent, ToolCallStatus, ToolKind,
};
use acp::v1::ConnectionTo;
use acp::{AcpAgentConfig, Agent};
use an_core::{truncate_chars, codes, AgentAdapter, AgentCard, AgentError, Artifact, EventSink, Evidence, Role, TaskResult, TaskSpec, WorkerStatus};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::util::{collect_git_changes, expand_tilde, resolve_command};

// ───────────────────────── 配置 ─────────────────────────

/// 一个 ACP agent 的配置。最小写法只需要 `id`、`command`、`role` 三项：
///
/// ```toml
/// [[acp.agents]]
/// id = "my-agent"
/// command = "my-agent"
/// args = ["acp"]
/// role = "coding"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfig {
    /// 唯一标识（配置执行组 [agents.assign] 时使用）
    pub id: String,
    /// 显示名称，默认等于 id
    #[serde(default)]
    pub name: Option<String>,
    /// 可执行文件：命令名（在 PATH 和常见安装目录中查找）或路径（支持 `~`）
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// 额外环境变量（例如 API Key）
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 承担的角色：general / coding / reviewer
    pub role: Role,
    /// agent 类型，执行组只允许同类型互为备份。不填时取命令的文件名（例如 hermes）
    #[serde(default)]
    pub agent_type: Option<String>,
    /// 同时最多执行几个任务
    #[serde(default = "default_concurrency")]
    pub max_concurrent: u32,
    /// 工作目录；不填时 coding 用代码目录，其他角色用文件工作目录
    #[serde(default)]
    pub workdir: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// agent 请求权限时如何应答
    #[serde(default)]
    pub permission: PermissionMode,
    /// 未安装时给用户的提示（预设使用）
    #[serde(default)]
    pub install_hint: Option<String>,
}

fn default_timeout() -> u64 {
    1800
}

fn default_concurrency() -> u32 {
    1
}

impl AcpConfig {
    /// agent 类型：显式配置优先，否则取命令的文件名（`~/bin/hermes` → `hermes`）
    pub fn agent_type(&self) -> String {
        self.agent_type.clone().unwrap_or_else(|| {
            std::path::Path::new(&self.command).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| self.id.clone())
        })
    }
}

/// 权限应答策略。任务能执行到这里，说明已经通过了 Policy（含高风险人工审批），
/// 所以默认允许；需要更严格时可以改为只读或全部拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// 全部允许（每次都记录到证据中）
    #[default]
    AllowAll,
    /// 只允许读取、搜索、思考、获取类操作，拒绝编辑、删除、执行命令等
    ReadOnly,
    /// 全部拒绝
    DenyAll,
}

/// 内置预设：本机装了对应命令就会被自动发现，不需要任何配置。
///
/// 来源：
/// - opencode：`opencode acp`（https://opencode.ai/docs/acp）
/// - Hermes Agent：`hermes acp`（https://hermes-agent.nousresearch.com/docs/user-guide/features/acp）
/// - pi：本身不支持 ACP，需要第三方适配器 pi-acp（成熟度较低）
pub fn presets() -> Vec<AcpConfig> {
    let preset = |id: &str, name: &str, command: &str, args: &[&str], role: Role, hint: &str| AcpConfig {
        id: id.into(),
        name: Some(name.into()),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: BTreeMap::new(),
        role,
        agent_type: Some(id.into()),
        max_concurrent: 1,
        workdir: None,
        timeout_secs: default_timeout(),
        permission: PermissionMode::AllowAll,
        install_hint: Some(hint.into()),
    };
    vec![
        preset("opencode", "OpenCode", "opencode", &["acp"], Role::Coding, "安装方法见 https://opencode.ai"),
        preset("hermes", "Hermes Agent", "hermes", &["acp"], Role::General, "需安装 Hermes Agent 及其 ACP 扩展，见 Hermes 文档"),
        preset("pi", "pi（经 pi-acp 适配）", "pi-acp", &[], Role::Coding, "需安装 pi 和第三方适配器 pi-acp"),
    ]
}

// ───────────────────────── 适配器 ─────────────────────────

pub struct AcpAgent {
    cfg: AcpConfig,
    /// 解析到的可执行文件路径；None 表示未安装
    resolved: Option<PathBuf>,
    workdir: PathBuf,
    card: AgentCard,
}

impl AcpAgent {
    /// `default_workdir`：配置中未指定 workdir 时使用的目录（由运行时按角色传入）
    pub fn new(cfg: AcpConfig, default_workdir: PathBuf) -> Self {
        let resolved = resolve_command(&cfg.command);
        let workdir = cfg.workdir.as_ref().map(|p| PathBuf::from(expand_tilde(&p.to_string_lossy()))).unwrap_or(default_workdir);
        let name = cfg.name.clone().unwrap_or_else(|| cfg.id.clone());
        let card = AgentCard {
            id: cfg.id.clone(),
            kind: "acp".into(),
            agent_type: cfg.agent_type(),
            location: "本机".into(),
            max_concurrent: cfg.max_concurrent.max(1),
            role: cfg.role,
            name,
            description: cfg.role.description().into(),
            tools: vec![],
            enabled: resolved.is_some(),
            disabled_reason: resolved.is_none().then(|| {
                format!("找不到命令 `{}`{}", cfg.command, cfg.install_hint.as_ref().map(|h| format!("（{h}）")).unwrap_or_default())
            }),
        };
        Self { cfg, resolved, workdir, card }
    }

    /// 本机是否安装了该 agent
    pub fn is_installed(&self) -> bool {
        self.resolved.is_some()
    }
}

/// 执行过程中收集到的信息
#[derive(Default)]
struct Collected {
    /// agent 的回复文本（消息片段拼接）
    message: String,
    /// 工具调用：id → (标题, 类型, 状态)，并记录出现顺序
    tool_calls: HashMap<String, (String, String, String)>,
    tool_order: Vec<String>,
    /// agent 报告的文件改动
    diffs: Vec<Artifact>,
    /// 权限请求及应答记录
    permissions: Vec<String>,
}

/// 把 serde 枚举转成字符串（例如 ToolKind::Edit → "edit"）
fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "other".into())
}

/// 处理 agent 推送的 session/update
fn on_update(state: &Mutex<Collected>, events: &EventSink, update: SessionUpdate) {
    let mut st = state.lock().unwrap();
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(t) = chunk.content {
                st.message.push_str(&t.text);
            }
        }
        SessionUpdate::ToolCall(call) => {
            let (kind, status) = (enum_str(&call.kind), enum_str(&call.status));
            events.progress(format!("工具调用：{}（{kind}）", call.title));
            let id = call.tool_call_id.to_string();
            collect_diffs(&mut st.diffs, &call.content);
            st.tool_order.push(id.clone());
            st.tool_calls.insert(id, (call.title, kind, status));
        }
        SessionUpdate::ToolCallUpdate(upd) => {
            let id = upd.tool_call_id.to_string();
            if let Some(content) = &upd.fields.content {
                collect_diffs(&mut st.diffs, content);
            }
            let entry = st.tool_calls.entry(id.clone()).or_insert_with(|| (id.clone(), "other".into(), "pending".into()));
            if let Some(t) = upd.fields.title {
                entry.0 = t;
            }
            if let Some(k) = upd.fields.kind {
                entry.1 = enum_str(&k);
            }
            if let Some(s) = upd.fields.status {
                entry.2 = enum_str(&s);
                if matches!(s, ToolCallStatus::Completed | ToolCallStatus::Failed) {
                    let msg = format!("{}：{}", entry.0, if s == ToolCallStatus::Completed { "完成" } else { "失败" });
                    events.progress(msg);
                }
            }
            if !st.tool_order.contains(&id) {
                st.tool_order.push(id);
            }
        }
        SessionUpdate::Plan(plan) => {
            let steps = plan.entries.iter().map(|e| e.content.clone()).collect::<Vec<_>>().join(" / ");
            events.progress(format!("计划：{}", truncate_chars(&steps, 300)));
        }
        // 思考片段、模式变化、用量等对用户意义不大，忽略
        _ => {}
    }
}

/// 从工具调用内容中提取文件改动
fn collect_diffs(diffs: &mut Vec<Artifact>, content: &[ToolCallContent]) {
    for c in content {
        if let ToolCallContent::Diff(d) = c {
            diffs.push(Artifact {
                kind: "diff".into(),
                name: d.path.to_string_lossy().into_owned(),
                content: truncate_chars(&format!("（新内容）\n{}", d.new_text), 8000),
            });
        }
    }
}

/// 按权限模式选择应答。只按选项的 kind 选择，不按位置或名称（与官方示例一致）。
fn decide_permission(mode: PermissionMode, req: &RequestPermissionRequest) -> (RequestPermissionOutcome, String) {
    let kind = req.tool_call.fields.kind.unwrap_or(ToolKind::Other);
    let title = req.tool_call.fields.title.clone().unwrap_or_else(|| req.tool_call.tool_call_id.to_string());
    let allow = match mode {
        PermissionMode::AllowAll => true,
        PermissionMode::ReadOnly => matches!(kind, ToolKind::Read | ToolKind::Search | ToolKind::Think | ToolKind::Fetch),
        PermissionMode::DenyAll => false,
    };
    let preferred = if allow {
        [PermissionOptionKind::AllowOnce, PermissionOptionKind::AllowAlways]
    } else {
        [PermissionOptionKind::RejectOnce, PermissionOptionKind::RejectAlways]
    };
    let chosen = preferred.iter().find_map(|k| req.options.iter().find(|o| o.kind == *k).map(|o| o.option_id.clone()));
    let desc = format!("{title}（{}）→ {}", enum_str(&kind), if allow { "允许" } else { "拒绝" });
    match chosen {
        Some(id) => (RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)), desc),
        None => (RequestPermissionOutcome::Cancelled, format!("{desc}（没有对应选项，已取消）")),
    }
}

/// stop_reason 的中文说明
fn stop_reason_text(r: &StopReason) -> String {
    match r {
        StopReason::EndTurn => "正常结束".into(),
        StopReason::MaxTokens => "达到最大 token 数".into(),
        StopReason::MaxTurnRequests => "达到最大轮次".into(),
        StopReason::Refusal => "agent 拒绝继续".into(),
        StopReason::Cancelled => "已取消".into(),
        other => enum_str(other),
    }
}

#[async_trait::async_trait]
impl AgentAdapter for AcpAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        let Some(command) = self.resolved.clone() else {
            return Err(AgentError::new(codes::AGENT_NOT_INSTALLED, self.card.disabled_reason.clone().unwrap_or_default()).into());
        };
        let task_id = task.task_id.clone();
        tokio::fs::create_dir_all(&self.workdir).await?;
        let workdir = self.workdir.canonicalize()?;
        let prompt = if task.context.is_empty() { task.goal.clone() } else { format!("{}\n\n## 背景信息\n{}", task.goal, task.context) };
        events.progress(format!("启动 ACP agent：{}（工作目录 {}）", self.card.name, workdir.display()));

        let state = Arc::new(Mutex::new(Collected::default()));
        // 用于区分"握手失败"和"执行中出错"，两者错误码不同，便于排查
        let initialized = Arc::new(AtomicBool::new(false));
        let process = acp::AcpAgent::new(AcpAgentConfig::new(command).args(self.cfg.args.clone()).envs(self.cfg.env.clone()));
        let mode = self.cfg.permission;

        let (st_n, ev_n) = (state.clone(), events.clone());
        let (st_p, ev_p, tid_p) = (state.clone(), events.clone(), task_id.clone());
        let init_flag = initialized.clone();
        let cwd = workdir.clone();

        let session = acp::Client
            .v1()
            .name("agentnexus")
            // agent 推送的进度
            .on_receive_notification(
                async move |n: SessionNotification, _cx| {
                    on_update(&st_n, &ev_n, n.update);
                    Ok(())
                },
                acp::on_receive_notification!(),
            )
            // agent 请求权限：按配置自动应答，并记录到证据
            .on_receive_request(
                async move |req: RequestPermissionRequest, responder, _cx| {
                    let (outcome, desc) = decide_permission(mode, &req);
                    tracing::info!(event = "acp.permission", task_id = %tid_p, decision = %desc);
                    ev_p.progress(format!("权限请求：{desc}"));
                    st_p.lock().unwrap().permissions.push(desc);
                    responder.respond(RequestPermissionResponse::new(outcome))
                },
                acp::on_receive_request!(),
            )
            .connect_with(process, async move |cx: ConnectionTo<Agent>| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
                init_flag.store(true, Ordering::SeqCst);
                let s = cx.send_request(NewSessionRequest::new(cwd)).block_task().await?;
                let r = cx
                    .send_request(PromptRequest::new(s.session_id, vec![ContentBlock::Text(TextContent::new(prompt))]))
                    .block_task()
                    .await?;
                Ok(r.stop_reason)
            });

        let outcome = tokio::time::timeout(Duration::from_secs(self.cfg.timeout_secs), session).await;
        let stop = match outcome {
            Ok(Ok(stop)) => stop,
            Ok(Err(e)) if !initialized.load(Ordering::SeqCst) => {
                tracing::error!(event = "acp.handshake_failed", code = codes::ACP_HANDSHAKE, task_id = %task_id, agent_id = %self.card.id, error = %e);
                return Err(AgentError::new(codes::ACP_HANDSHAKE, format!("{} 启动或握手失败：{e}", self.card.name)).into());
            }
            Ok(Err(e)) => {
                tracing::error!(event = "acp.runtime_error", code = codes::ACP_RUNTIME, task_id = %task_id, agent_id = %self.card.id, error = %e);
                return Ok(TaskResult::failed_code(codes::ACP_RUNTIME, format!("{} 执行出错：{e}", self.card.name)));
            }
            // 超时：future 被 drop，SDK 会杀掉 agent 进程
            Err(_) => {
                tracing::error!(event = "acp.timeout", code = codes::AGENT_TIMEOUT, task_id = %task_id, agent_id = %self.card.id, timeout_secs = self.cfg.timeout_secs);
                return Ok(TaskResult::failed_code(codes::AGENT_TIMEOUT, format!("{} 超时（{} 秒）", self.card.name, self.cfg.timeout_secs)));
            }
        };

        // ───── 整理结果 ─────
        let st = std::mem::take(&mut *state.lock().unwrap());
        let mut evidence = vec![Evidence { kind: "acp_stop_reason".into(), content: stop_reason_text(&stop) }];
        if !st.tool_order.is_empty() {
            let lines = st
                .tool_order
                .iter()
                .filter_map(|id| st.tool_calls.get(id))
                .map(|(title, kind, status)| format!("- {title}（{kind}，{status}）"))
                .collect::<Vec<_>>()
                .join("\n");
            evidence.push(Evidence { kind: "tool_calls".into(), content: truncate_chars(&lines, 3000) });
        }
        if !st.permissions.is_empty() {
            evidence.push(Evidence { kind: "permissions".into(), content: st.permissions.join("\n") });
        }
        // 优先用 git diff 作为改动证据；工作目录不是 git 仓库时退回 agent 自己报告的 diff
        let (git_evidence, git_artifacts) = collect_git_changes(&workdir).await;
        evidence.extend(git_evidence);
        let artifacts = if git_artifacts.is_empty() { st.diffs } else { git_artifacts };

        let ok = stop == StopReason::EndTurn;
        tracing::info!(
            event = "acp.finished",
            task_id = %task_id,
            agent_id = %self.card.id,
            stop_reason = %enum_str(&stop),
            tool_calls = st.tool_order.len(),
            permissions = st.permissions.len()
        );
        let summary = if st.message.trim().is_empty() { "（agent 没有返回文字说明）".to_string() } else { st.message.trim().to_string() };
        Ok(TaskResult {
            status: if ok { WorkerStatus::Succeeded } else { WorkerStatus::Failed },
            summary,
            artifacts,
            evidence,
            errors: if ok { vec![] } else { vec![format!("[{}] agent 结束原因：{}", codes::ACP_STOP, stop_reason_text(&stop))] },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp::v1::schema::{PermissionOption, ToolCallUpdate, ToolCallUpdateFields};

    fn req(kind: ToolKind) -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            "s",
            ToolCallUpdate::new("c", ToolCallUpdateFields::new().title("t").kind(kind)),
            vec![
                PermissionOption::new("a1", "允许一次", PermissionOptionKind::AllowOnce),
                PermissionOption::new("aa", "总是允许", PermissionOptionKind::AllowAlways),
                PermissionOption::new("r1", "拒绝", PermissionOptionKind::RejectOnce),
            ],
        )
    }

    fn chosen(o: RequestPermissionOutcome) -> String {
        match o {
            RequestPermissionOutcome::Selected(s) => s.option_id.to_string(),
            _ => "cancelled".into(),
        }
    }

    #[test]
    fn permission_modes() {
        // 默认全部允许，且优先选"允许一次"而不是"总是允许"
        assert_eq!(chosen(decide_permission(PermissionMode::AllowAll, &req(ToolKind::Execute)).0), "a1");
        // 只读模式：读允许，执行拒绝
        assert_eq!(chosen(decide_permission(PermissionMode::ReadOnly, &req(ToolKind::Read)).0), "a1");
        assert_eq!(chosen(decide_permission(PermissionMode::ReadOnly, &req(ToolKind::Execute)).0), "r1");
        assert_eq!(chosen(decide_permission(PermissionMode::DenyAll, &req(ToolKind::Read)).0), "r1");
    }

    #[test]
    fn presets_are_valid() {
        let p = presets();
        assert!(p.iter().any(|c| c.id == "opencode" && c.args == ["acp"]));
        assert!(p.iter().any(|c| c.id == "hermes" && c.role == Role::General));
    }
}
