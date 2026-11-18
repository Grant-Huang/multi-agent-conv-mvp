//! 模拟工厂的工具目录与调用入口。
//!
//! 工具名只用小写字母、数字和下划线，长度不超过 64：MCP 规范允许点号，但 Anthropic 与 OpenAI 的
//! 工具名规则是 `^[a-zA-Z0-9_-]{1,64}$`，用点号会让挂载到这些模型的系统出错（设计文档 §11a 修订）。
//!
//! **当前状态：接口骨架**，行为尚未实现（RT-DESF-01～08、RT-DESM-01～03、RT-DES-30 应为 Red）。

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具类别：决定谁可以调用（设计文档 §11a 工具表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// 只读观测：agent、桌面、连接器。
    Observe,
    /// 外部写（幂等键、审批引用、回执）：只由 AgentNexus 审批后的执行流程调用。
    Execute,
    /// 仿真控制：测试调度器、实验脚本。
    Control,
    /// 管理（敏感）：只供审核层，需要管理口令，不进任何 agent 的允许清单。
    Admin,
}

/// 一个工具的说明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub category: ToolCategory,
    pub description: String,
    /// JSON Schema（object，拒绝未知字段）。
    pub input_schema: Value,
}

/// 服务配置。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// 管理口令；为 None 时管理工具一律拒绝。
    pub admin_token: Option<String>,
}

/// 全部工具。
///
/// 骨架：尚未实现。
pub fn tool_catalog() -> Vec<ToolSpec> {
    vec![]
}

/// 模拟工厂服务：持有多个仿真的状态，按工具名分派调用。同步、确定，不读墙钟。
#[derive(Debug, Default)]
pub struct FactoryService {
    config: ServiceConfig,
}

impl FactoryService {
    pub fn new(config: ServiceConfig) -> Self {
        Self { config }
    }

    /// 按工具名调用；参数与返回值都是 JSON，与 MCP 的 `tools/call` 一一对应。
    ///
    /// 骨架：尚未实现。
    pub fn call(&mut self, tool: &str, _args: Value) -> Result<Value> {
        let _ = &self.config;
        bail!("tool {tool} is not implemented yet")
    }
}
