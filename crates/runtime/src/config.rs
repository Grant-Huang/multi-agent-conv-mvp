//! 配置文件（config.toml）结构。所有字段都有默认值，最小配置只需要 `[llm]`。

use std::path::{Path, PathBuf};

use an_agents::CodingConfig;
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
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("无法读取配置文件 {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("配置文件格式错误 {}", path.display()))
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
    pub conversation: Option<String>,
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
        Self { enabled: true, max_steps: d_steps(), workspace_dir: d_ws() }
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
