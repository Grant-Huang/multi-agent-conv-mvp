//! 托管配置、热注册与撤销（AG.2）。骨架：行为尚未实现。
use std::time::Duration;

/// 托管配置管理器
#[derive(Default)]
pub struct AgentConfigManager {}

impl AgentConfigManager {
    /// 测试用：调整提议的有效期
    pub fn set_proposal_ttl(&self, _ttl: Duration) {}

    /// 托管文件读取失败的原因
    pub fn load_error(&self) -> Option<String> {
        None
    }
}
