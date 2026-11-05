//! AgentNexus 核心数据结构。
//!
//! 设计原则（来自前期讨论）：
//! - 模型负责理解，程序负责决策，Harness 负责执行，治理层负责约束和验收；
//! - Worker 返回 `completed` 只代表"执行进程正常结束"，不代表"验收通过"；
//! - 任务状态由 AgentNexus 自己掌握，而不是放在某个 Harness 的内存里。
//!
//! 带 `#[derive(TS)]` 的类型会在 `cargo test` 时自动导出为 TypeScript 类型，
//! 输出到 `web/src/bindings/`（见 `.cargo/config.toml`），前后端共用一份定义。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ───────────────────────────── 枚举 ─────────────────────────────

/// 执行角色：必须是系统注册过的角色，模型不能自创新 Agent。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Role {
    /// 通用执行：问答、分析、调用工具、生成文档
    General,
    /// 工程执行：通过外部 Harness（Codex CLI）修改代码
    Coding,
    /// 独立审查：只读，给出评估意见
    Reviewer,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::General, Role::Coding, Role::Reviewer];

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::General => "general",
            Role::Coding => "coding",
            Role::Reviewer => "reviewer",
        }
    }

    /// 角色的语义描述（给 Router 模型看）。
    /// 角色是"做什么"，具体由哪个 agent 来做由注册表和配置决定。
    pub fn description(&self) -> &'static str {
        match self {
            Role::General => "通用执行：知识问答、分析、计算、读写普通文件、生成 Markdown 文档或报告",
            Role::Coding => "工程执行：在代码仓库中阅读、修改、调试代码，运行测试；生成网页、脚本等代码产物。适合明确的代码开发/修复任务",
            Role::Reviewer => "独立审查：只读，检查用户提供的方案、文档、代码片段是否正确完整，给出评估意见",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// JEV 任务分类：是否需要工具 × 是否需要生成文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TaskClass {
    /// 不需要工具，也不需要文档
    L1,
    /// 需要工具，不需要文档
    L2,
    /// 不需要工具，需要文档
    L3,
    /// 既需要工具，也需要文档
    L4,
}

/// 风险等级。模型只能给"建议值"，是否需要审批由 Policy 决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Risk {
    Low,
    Medium,
    High,
}

/// 任务状态机。
///
/// ```text
/// CREATED → QUEUED → RUNNING → VERIFYING → COMPLETED
///    │                  │           │
///    └→ NEEDS_HUMAN     └→ FAILED   └→ FAILED / NEEDS_HUMAN
/// 任意非终态 → CANCELLED
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum TaskStatus {
    Created,
    Queued,
    Running,
    Verifying,
    Completed,
    Failed,
    /// Agent 无法可靠推进，或高风险任务等待人工审批
    NeedsHuman,
    Cancelled,
}

impl TaskStatus {
    /// 终态：不会再自动变化（NeedsHuman 不算终态，人工审批后可以继续）
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Created => "CREATED",
            TaskStatus::Queued => "QUEUED",
            TaskStatus::Running => "RUNNING",
            TaskStatus::Verifying => "VERIFYING",
            TaskStatus::Completed => "COMPLETED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::NeedsHuman => "NEEDS_HUMAN",
            TaskStatus::Cancelled => "CANCELLED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

/// Worker 进程层面的结果（与业务验收无关）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WorkerStatus {
    Succeeded,
    Failed,
}

// ───────────────────────── 会话层决策 ─────────────────────────

/// Conversation 的 Turn Controller 每一轮的决策：这轮下一步做什么。
/// 注意：它只决定"要不要委派"，不决定"派给谁、用什么工具"——那是 Router 的事。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(export)]
pub enum TurnDecision {
    /// 直接回答（简单问答不走后台执行）
    Respond { reply: String },
    /// 信息不足，向用户追问
    Clarify { question: String },
    /// 委派给 Router，由后台 Agent 执行
    Delegate { task_goal: String },
    /// 针对进行中的任务：补充要求或取消
    FollowUp { task_id: String, kind: FollowUpKind, change: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FollowUpKind {
    /// 补充/修改要求：MVP 中做法是取消旧任务，用合并后的目标重新派发
    Supplement,
    /// 取消任务
    Cancel,
}

// ───────────────────────── 路由层决策 ─────────────────────────

/// Router 的结构化输出（经过 Policy 校验后才会被执行）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RouteDecision {
    pub task_class: TaskClass,
    pub execution_role: Role,
    /// 必须是工具注册表里已有的工具名
    pub required_tools: Vec<String>,
    /// 模型给出的风险建议值
    pub risk_level: Risk,
    /// 路由理由：写入日志，用于积累评测集
    pub reason: String,
}

/// Policy 对路由决策的裁决结果（确定性规则，不调用模型）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PolicyVerdict {
    /// 是否允许自动执行；false 时任务进入 NEEDS_HUMAN 等待审批
    pub auto_execute: bool,
    /// 执行完成后是否需要 Reviewer 独立审查
    pub needs_review: bool,
    /// 规则命中说明，例如 "high risk requires approval"
    pub notes: Vec<String>,
}

// ───────────────────────── Agent 能力注册 ─────────────────────────

/// 能力卡片：Router 根据它来选择角色，Policy 根据它来校验。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentCard {
    /// 唯一标识，例如 "general"、"codex"、"opencode"、"hermes@mac-mini"（配置执行组时用它）
    pub id: String,
    /// 接入方式："builtin"（内置）、"cli"（命令行子进程）、"acp"（本机 ACP）、"node"（远程节点）
    pub kind: String,
    /// agent 的类型，例如 "hermes"、"opencode"、"codex"。
    /// 只有类型相同的 agent 才能放进同一个执行组互为备份。
    pub agent_type: String,
    /// 在哪台机器上："本机" 或节点名称
    pub location: String,
    /// 同时最多执行几个任务（执行组按它分配并发）
    pub max_concurrent: u32,
    pub role: Role,
    pub name: String,
    /// 给 Router 模型看的能力描述
    pub description: String,
    /// 该 Agent 可以使用的工具
    pub tools: Vec<ToolInfo>,
    /// 是否可用（例如本机没装 codex 时，Coding 不可用）
    pub enabled: bool,
    /// 不可用的原因
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

// ───────────────────────── 任务与结果 ─────────────────────────

/// 交给 Agent 执行的任务说明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub task_id: String,
    pub conversation_id: String,
    /// 任务目标（由 Conversation 整理过，不是用户原话）
    pub goal: String,
    /// 额外上下文：近期对话摘录、审查意见等
    pub context: String,
    pub route: RouteDecision,
}

/// 所有 Worker 统一的返回格式（Agents as Tools：只返回结果，不直接面向用户）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskResult {
    pub status: WorkerStatus,
    /// 给上层理解的摘要
    pub summary: String,
    /// 产物：文件、代码 diff、报告等
    pub artifacts: Vec<Artifact>,
    /// 证据：工具调用记录、命令输出、测试结果
    pub evidence: Vec<Evidence>,
    pub errors: Vec<String>,
}

impl TaskResult {
    /// 带错误码的失败结果，错误码写在消息开头：`[E_XXX] 说明`
    pub fn failed_code(code: &str, msg: impl std::fmt::Display) -> Self {
        Self::failed(format!("[{code}] {msg}"))
    }

    pub fn failed(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        TaskResult {
            status: WorkerStatus::Failed,
            summary: msg.clone(),
            artifacts: vec![],
            evidence: vec![],
            errors: vec![msg],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Artifact {
    /// 例如 "file" / "diff" / "text"
    pub kind: String,
    pub name: String,
    /// 内容（MVP 直接内联文本；以后换成 Artifact Store 的引用）
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Evidence {
    /// 例如 "tool_call" / "exit_code" / "stdout_tail" / "git_diff_stat"
    pub kind: String,
    pub content: String,
}

/// Reviewer 的评估意见（只是"意见"，最终状态由编排层按规则决定）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewVerdict {
    pub verdict: Verdict,
    pub issues: Vec<String>,
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Verdict {
    Pass,
    Fail,
    NeedsHuman,
}

// ───────────────────────── 持久化记录（API 返回用）─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    /// "user" 或 "assistant"
    pub role: String,
    pub content: String,
    /// 这条消息关联的任务（委派确认、结果交付时会带上）
    pub task_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskRecord {
    pub id: String,
    pub conversation_id: String,
    pub goal: String,
    pub role: Role,
    pub status: TaskStatus,
    pub route: RouteDecision,
    pub policy: PolicyVerdict,
    pub result: Option<TaskResult>,
    pub review: Option<ReviewVerdict>,
    pub created_at: String,
    pub updated_at: String,
}

/// 每次路由的完整日志：用于调试和积累评测集
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RouteLog {
    pub id: String,
    pub conversation_id: String,
    pub user_text: String,
    pub turn_decision: Option<TurnDecision>,
    pub route_decision: Option<RouteDecision>,
    pub policy: Option<PolicyVerdict>,
    pub task_id: Option<String>,
    /// 模型原始输出（便于排查小模型的格式问题）
    pub raw_outputs: Vec<String>,
    /// 结构化输出总共尝试了几次（>1 说明有重试）
    pub attempts: u32,
    #[ts(type = "number")]
    pub latency_ms: u64,
    pub error: Option<String>,
    pub created_at: String,
}

// ───────────────────────── 事件 ─────────────────────────

/// 推送给前端的事件（SSE）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskEvent {
    pub id: String,
    pub conversation_id: String,
    pub task_id: Option<String>,
    pub kind: EventKind,
    /// 状态变化事件会带上新状态
    pub status: Option<TaskStatus>,
    pub message: String,
    /// 流式回复的标识（只有 assistant_delta 事件有）：同一条回复的所有片段共用，等于最终消息的 id
    #[serde(default)]
    pub stream_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EventKind {
    /// 任务状态变化
    StatusChanged,
    /// 执行过程中的进度（工具调用、命令输出等）
    Progress,
    /// 会话中出现了新的助手消息（例如任务结果交付）
    AssistantMessage,
    /// 直接回答时的流式片段（只推送给界面，不写数据库）
    AssistantDelta,
}

/// 事件发布接口：由运行时实现（写库 + 广播），Agent 只管调用。
pub trait EventPublisher: Send + Sync {
    fn publish(&self, event: TaskEvent);
}

/// 交给 Agent 的事件出口，已经绑定了会话和任务 ID。
#[derive(Clone)]
pub struct EventSink {
    publisher: Arc<dyn EventPublisher>,
    conversation_id: String,
    task_id: String,
}

impl EventSink {
    pub fn new(publisher: Arc<dyn EventPublisher>, conversation_id: &str, task_id: &str) -> Self {
        Self { publisher, conversation_id: conversation_id.into(), task_id: task_id.into() }
    }

    /// 发布一条进度事件
    pub fn progress(&self, message: impl Into<String>) {
        self.publisher.publish(TaskEvent {
            id: new_id("evt"),
            conversation_id: self.conversation_id.clone(),
            task_id: Some(self.task_id.clone()),
            kind: EventKind::Progress,
            status: None,
            message: message.into(),
            stream_id: None,
            created_at: now(),
        });
    }
}

// ───────────────────────── Agent 接入抽象 ─────────────────────────

/// 统一的 Agent 接入接口：内置 Agent 和外部 Harness 都实现它。
///
/// 取消：编排层会直接 abort 执行该方法的 tokio 任务；
/// 实现方需保证被 drop 时能释放资源（例如子进程设置 `kill_on_drop`）。
#[async_trait::async_trait]
pub trait AgentAdapter: Send + Sync {
    /// 能力卡片
    fn card(&self) -> &AgentCard;

    /// 执行任务。出错时也尽量返回 `TaskResult::failed`，而不是直接 Err。
    async fn run(&self, task: TaskSpec, events: EventSink) -> anyhow::Result<TaskResult>;
}

/// 带错误码的 agent 错误。错误码会出现在日志、任务结果和诊断接口中，方便自动排查。
/// 出错一律如实报告，编排层不会因此悄悄换用别的 agent。
#[derive(Debug, Clone)]
pub struct AgentError {
    pub code: &'static str,
    pub message: String,
}

impl AgentError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}
impl std::error::Error for AgentError {}

/// 错误码清单（完整说明见 docs/DESIGN.md「日志与排查」）。
/// 约定：E_ 开头是错误（任务失败或功能不可用），W_ 开头是警告（可自动恢复）。
pub mod codes {
    // 模型
    pub const LLM_UNREACHABLE: &str = "E_LLM_UNREACHABLE";
    pub const LLM_HTTP: &str = "E_LLM_HTTP";
    pub const LLM_FORMAT: &str = "E_LLM_FORMAT";
    pub const LLM_FORMAT_RETRY: &str = "W_LLM_FORMAT_RETRY";
    pub const LLM_TIMEOUT: &str = "E_LLM_TIMEOUT";
    pub const LLM_CONTEXT: &str = "E_LLM_CONTEXT";
    pub const LLM_RATE_LIMITED: &str = "W_LLM_RATE_LIMITED";
    pub const LLM_TRUNCATED: &str = "E_LLM_TRUNCATED";
    pub const LLM_STREAM: &str = "E_LLM_STREAM";
    pub const LLM_BREAKER: &str = "E_LLM_BREAKER";
    pub const LLM_BUSY: &str = "E_LLM_BUSY";
    pub const LLM_SLOTS_MISMATCH: &str = "W_LLM_SLOTS_MISMATCH";
    pub const LLM_CONTEXT_TRIM: &str = "W_LLM_CONTEXT_TRIM";
    pub const LLM_RETRY: &str = "W_LLM_RETRY";
    // 路由与执行组
    pub const ROUTE_NO_AGENT: &str = "E_ROUTE_NO_AGENT";
    pub const POOL_CONFIG: &str = "E_POOL_CONFIG";
    pub const POOL_MEMBER_MISSING: &str = "W_POOL_MEMBER_MISSING";
    pub const POOL_WAIT: &str = "W_POOL_WAIT";
    // agent 执行
    pub const AGENT_NOT_INSTALLED: &str = "E_AGENT_NOT_INSTALLED";
    pub const AGENT_SPAWN: &str = "E_AGENT_SPAWN";
    pub const AGENT_TIMEOUT: &str = "E_AGENT_TIMEOUT";
    pub const AGENT_RUNTIME: &str = "E_AGENT_RUNTIME";
    pub const ACP_HANDSHAKE: &str = "E_ACP_HANDSHAKE";
    pub const ACP_RUNTIME: &str = "E_ACP_RUNTIME";
    pub const ACP_STOP: &str = "E_ACP_STOP";
    pub const REVIEW_FAILED: &str = "E_REVIEW_FAILED";
    pub const TASK_TIMEOUT: &str = "E_TASK_TIMEOUT";
    // 节点
    pub const NODE_UNREACHABLE: &str = "E_NODE_UNREACHABLE";
    pub const NODE_AUTH: &str = "E_NODE_AUTH";
    pub const NODE_PAIR: &str = "E_NODE_PAIR";
    pub const NODE_BUSY: &str = "E_NODE_BUSY";
    pub const NODE_PROTOCOL: &str = "E_NODE_PROTOCOL";
    pub const NODE_AGENT_NOT_FOUND: &str = "E_NODE_AGENT_NOT_FOUND";
    pub const NODE_OFFLINE: &str = "W_NODE_OFFLINE";
    pub const DISCOVERY: &str = "W_DISCOVERY";
    // 配置
    pub const CONFIG_UNKNOWN_KEY: &str = "W_CONFIG_UNKNOWN_KEY";
    // 存储
    pub const STORE_READ_SLOW: &str = "W_STORE_READ_SLOW";
}

/// AgentNexus 与节点之间的协议数据（HTTP + SSE）。
pub mod node {
    use serde::{Deserialize, Serialize};
    use ts_rs::TS;

    /// mDNS 服务类型
    pub const SERVICE_TYPE: &str = "_agentnexus._tcp.local.";
    /// 协议版本：不兼容的修改时加一
    pub const PROTOCOL_VERSION: u32 = 1;

    /// `GET /node/info`（无需认证）
    #[derive(Debug, Clone, Serialize, Deserialize, TS)]
    #[ts(export)]
    pub struct NodeInfo {
        pub node_id: String,
        pub name: String,
        pub version: String,
        pub protocol: u32,
        /// 操作系统，例如 "macos" / "linux" / "windows"
        pub os: String,
    }

    /// `POST /node/pair`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PairRequest {
        pub code: String,
        pub client_name: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PairResponse {
        pub token: String,
        pub node_id: String,
        pub name: String,
    }

    /// `POST /node/run`：在节点上执行任务，响应是 SSE 流（progress* → result | error）
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RunRequest {
        pub agent_id: String,
        pub task: crate::TaskSpec,
    }

    /// SSE `progress` 事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Progress {
        pub message: String,
    }

    /// 错误响应 / SSE `error` 事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NodeError {
        pub code: String,
        pub message: String,
    }
}

// ───────────────────────── 小工具函数 ─────────────────────────

/// 生成带前缀的 ID，例如 `task-3f2a...`
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

/// 当前时间（RFC3339）
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 本机名称（跨平台：调用系统的 `hostname` 命令），去掉 `.local` 后缀；失败时返回 "unknown"
pub fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}

/// 按字符数截断（不会切坏中文字符）
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for s in ["CREATED", "NEEDS_HUMAN", "COMPLETED"] {
            assert_eq!(TaskStatus::parse(s).unwrap().as_str(), s);
        }
    }

    #[test]
    fn truncate_keeps_chinese_intact() {
        assert_eq!(truncate_chars("你好世界", 2), "你好…");
        assert_eq!(truncate_chars("abc", 5), "abc");
    }
}
