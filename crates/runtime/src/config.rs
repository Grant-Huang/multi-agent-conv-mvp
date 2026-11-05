//! 配置文件（config.toml）结构。所有字段都有默认值，最小配置只需要 `[llm]`。

use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use an_agents::{AcpConfig, CodingConfig};
use an_core::Risk;
use an_llm::LlmConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    pub llm: LlmConfig,
    /// 按角色覆盖模型名（例如 Router 用更小更快的模型）
    #[serde(default)]
    pub models: ModelOverrides,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub orchestrator: OrchestratorConfig,
    /// 通过 ACP 接入的外部 agent
    #[serde(default)]
    pub acp: AcpSection,
    /// 局域网节点
    #[serde(default)]
    pub nodes: NodesSection,
    #[serde(default)]
    pub logging: LoggingSection,
    #[serde(default)]
    pub conversation: ConversationSection,
}

/// 前台（Conversation）的行为
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSection {
    /// 用户发起的审查请求怎么处理：reviewer（交给独立审查）/ direct（直接回答）/ auto（短内容直接回答）
    #[serde(default)]
    pub review_requests: crate::prompts::ReviewMode,
    /// 直接回答时是否边生成边显示（推理服务不支持流式时会自动退回一次性返回）
    #[serde(default = "d_true")]
    pub stream_replies: bool,
}

impl Default for ConversationSection {
    fn default() -> Self {
        Self { review_requests: Default::default(), stream_replies: true }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("无法读取配置文件 {}", path.display()))?;
        let (cfg, unknown) = Self::parse(&text).with_context(|| format!("配置文件格式错误 {}", path.display()))?;
        for key in unknown {
            tracing::warn!(event = "config.unknown_key", code = an_core::codes::CONFIG_UNKNOWN_KEY, key = %key, "配置中有不认识的字段，已忽略（可能是拼写错误或旧版本的写法）");
        }
        Ok(cfg)
    }

    /// 解析配置，同时返回被忽略的（不认识的）字段路径，例如 `agents.prefer`
    pub fn parse(text: &str) -> Result<(Self, Vec<String>)> {
        let mut unknown = vec![];
        let de = toml::Deserializer::new(text);
        let cfg = serde_ignored::deserialize(de, |path| unknown.push(path.to_string()))?;
        Ok((cfg, unknown))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "d_bind")]
    pub bind: String,
    /// SQLite 数据库路径
    #[serde(default = "d_db")]
    pub db_path: PathBuf,
    /// 前端构建产物目录（`web/dist`），不存在时只提供 API
    #[serde(default = "d_static")]
    pub static_dir: PathBuf,
}
fn d_bind() -> String {
    "127.0.0.1:8787".into()
}
fn d_db() -> PathBuf {
    "data/agentnexus.db".into()
}
fn d_static() -> PathBuf {
    "web/dist".into()
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self { bind: d_bind(), db_path: d_db(), static_dir: d_static() }
    }
}

/// 各角色使用的模型名；不填则使用 `[llm].model`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelOverrides {
    /// 前台判断"这一轮做什么"（输出很短，适合用小而快的模型）
    pub conversation: Option<String>,
    /// 直接回答用户时生成回复（不填则与 conversation 相同）
    pub reply: Option<String>,
    pub router: Option<String>,
    pub general: Option<String>,
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub reviewer: ToggleConfig,
    #[serde(default)]
    pub coding: CodingSection,
    /// 执行组：每个角色由哪些 agent 来做（第一个是主力，后面是待命的备份，必须同类型）。
    /// 例如 `coding = "opencode"` 或 `general = ["hermes@mac-a", "hermes@mac-b"]`。
    /// 不配置的角色使用该角色第一个可用的 agent。
    #[serde(default)]
    pub assign: BTreeMap<String, OneOrMany>,
}

/// 配置里既可以写一个字符串，也可以写字符串数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

impl AgentsConfig {
    /// 执行组配置（角色 → 成员 id 列表）；角色名写错时启动报错
    pub fn assign_map(&self) -> Result<BTreeMap<String, Vec<String>>> {
        let mut m = BTreeMap::new();
        for (role, members) in &self.assign {
            if an_core::Role::parse(role).is_none() {
                anyhow::bail!("[{}] [agents.assign] 中的角色 `{role}` 不存在，只能是 general / coding / reviewer", an_core::codes::POOL_CONFIG);
            }
            m.insert(role.clone(), members.to_vec());
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "d_true")]
    pub enabled: bool,
    /// 工具循环最大步数
    #[serde(default = "d_steps")]
    pub max_steps: usize,
    /// 文件工具只能访问这个目录
    #[serde(default = "d_ws")]
    pub workspace_dir: PathBuf,
    /// 同时最多执行几个任务（受本地模型并发能力限制）
    #[serde(default = "d_general_concurrency")]
    pub max_concurrent: u32,
}
fn d_general_concurrency() -> u32 {
    4
}
fn d_true() -> bool {
    true
}
fn d_steps() -> usize {
    6
}
fn d_ws() -> PathBuf {
    "workspace/files".into()
}
impl Default for GeneralConfig {
    fn default() -> Self {
        Self { enabled: true, max_steps: d_steps(), workspace_dir: d_ws(), max_concurrent: d_general_concurrency() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleConfig {
    #[serde(default = "d_true")]
    pub enabled: bool,
}
impl Default for ToggleConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingSection {
    #[serde(default = "d_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub cfg: CodingConfig,
}
impl Default for CodingSection {
    fn default() -> Self {
        Self { enabled: true, cfg: CodingConfig::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Reviewer 判定 fail 后最多返工几轮（0 表示不返工）
    #[serde(default = "d_rounds")]
    pub max_review_rounds: u32,
    /// 构建上下文时带上最近多少条消息
    #[serde(default = "d_history")]
    pub history_messages: usize,
    /// 单个任务（含返工）的总超时
    #[serde(default = "d_task_timeout")]
    pub task_timeout_secs: u64,
    /// 风险达到该等级的任务需要人工审批后才执行
    #[serde(default = "d_approval")]
    pub approval_risk: Risk,
}
fn d_rounds() -> u32 {
    1
}
fn d_history() -> usize {
    12
}
fn d_task_timeout() -> u64 {
    1800
}
fn d_approval() -> Risk {
    Risk::High
}
impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_review_rounds: d_rounds(),
            history_messages: d_history(),
            task_timeout_secs: d_task_timeout(),
            approval_risk: d_approval(),
        }
    }
}

/// ACP 接入配置。新手什么都不用写：默认会自动发现本机已安装的常见 ACP agent。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSection {
    /// 自动发现内置预设（opencode、hermes、pi-acp）中本机已安装的 agent
    #[serde(default = "d_true")]
    pub auto_detect: bool,
    /// 自定义 agent；id 与预设相同时覆盖预设
    #[serde(default)]
    pub agents: Vec<AcpConfig>,
}
impl Default for AcpSection {
    fn default() -> Self {
        Self { auto_detect: true, agents: vec![] }
    }
}

/// 局域网节点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodesSection {
    /// 自动发现局域网中的节点（mDNS）。有的网络会屏蔽组播，此时可以在界面上按地址手动添加
    #[serde(default = "d_true")]
    pub discovery: bool,
    /// 多久检查一次已配对节点是否在线（秒）
    #[serde(default = "d_refresh")]
    pub refresh_secs: u64,
}
fn d_refresh() -> u64 {
    15
}
impl Default for NodesSection {
    fn default() -> Self {
        Self { discovery: true, refresh_secs: d_refresh() }
    }
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    /// JSON 日志文件目录（按天滚动）；设为空字符串表示不写文件
    #[serde(default = "d_log_dir")]
    pub dir: String,
    /// 日志级别：error / warn / info / debug（环境变量 RUST_LOG 优先）
    #[serde(default = "d_log_level")]
    pub level: String,
}
fn d_log_dir() -> String {
    "logs".into()
}
fn d_log_level() -> String {
    "info".into()
}
impl Default for LoggingSection {
    fn default() -> Self {
        Self { dir: d_log_dir(), level: d_log_level() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置模板必须始终能被解析（防止改模板时写错）
    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.example.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        let (cfg, unknown) = AppConfig::parse(&text).expect("config.example.toml 解析失败");
        assert!(unknown.is_empty(), "配置模板中有不认识的字段：{unknown:?}");
        assert!(cfg.acp.auto_detect);
        assert!(cfg.agents.assign_map().unwrap().is_empty());
        assert!(cfg.nodes.discovery);
        assert_eq!(cfg.logging.dir, "logs");
        assert_eq!(cfg.conversation.review_requests, crate::prompts::ReviewMode::Reviewer);
        assert!(cfg.conversation.stream_replies);
        assert_eq!(cfg.llm.base_url, "http://127.0.0.1:8098/v1");
    }
}

#[cfg(test)]
mod assign_tests {
    use super::*;

    #[test]
    fn assign_accepts_one_or_many_and_checks_role() {
        let a: AgentsConfig = toml::from_str("[assign]\ncoding = \"opencode\"\ngeneral = [\"hermes@a\", \"hermes@b\"]").unwrap();
        let m = a.assign_map().unwrap();
        assert_eq!(m["coding"], ["opencode"]);
        assert_eq!(m["general"], ["hermes@a", "hermes@b"]);
        let bad: AgentsConfig = toml::from_str("[assign]\ncodeing = \"x\"").unwrap();
        assert!(bad.assign_map().unwrap_err().to_string().contains("E_POOL_CONFIG"));
    }
}

#[cfg(test)]
mod unknown_key_tests {
    use super::*;

    #[test]
    fn reports_unknown_keys() {
        let (_, unknown) = AppConfig::parse("[llm]\nbase_url = \"x\"\nmodel = \"m\"\n[agents.prefer]\ncoding = \"a\"\n[agents.general]\nmax_stpes = 3").unwrap();
        assert_eq!(unknown, ["agents.prefer", "agents.general.max_stpes"]);
    }
}
