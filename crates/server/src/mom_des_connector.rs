//! 阶段⑤a-2：AgentNexus 侧的模拟工厂连接器（设计文档 §11a，"⑤a-2 设计"）。
//!
//! 参照 RelOS 接入模式（`mom_relos`），在服务层异步调用模拟工厂（`an-des-mcp`，stdio），引擎保持确定、可重放：
//! - `tick`：推进模拟工厂 → 拉取事件与可观测快照 → 交给引擎 → 推进引擎，逐分钟进行；
//! - `dispatch_writes`：把引擎中审批后产生的写操作带幂等键与审批引用发给模拟工厂，回执写回动作；
//! - `agent_tools` / `call_agent_tool`：只向 agent 与桌面开放观测类工具；
//! - 模拟工厂不在线时返回 `E_DES_UNAVAILABLE`，不回退到进程内仿真，也不写入假观测；
//! - 管理口令只由连接器持有，用于读取标准答案交给审核层，不进入任何 agent 上下文。
//!
//! **当前状态：接口骨架**（阶段⑤a-2 Red）。

use an_runtime::mom::des::remote::RemoteWrite;
use an_runtime::mom::des::DesSimulationConfig;
use an_runtime::mom::Engine;
use anyhow::{bail, Result};
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

/// 模拟工厂不可用（命令不存在、进程退出、协议错误）时的错误码。
pub const E_DES_UNAVAILABLE: &str = "E_DES_UNAVAILABLE";

/// 连接到一个模拟工厂进程。
pub struct DesConnector {
    _private: (),
}

impl DesConnector {
    /// 启动模拟工厂（stdio）并完成握手；`admin_token` 通过环境变量 `AN_DES_ADMIN_TOKEN` 传给它。
    ///
    /// 骨架：尚未实现。
    pub async fn spawn(_command: &Path, _args: &[String], _admin_token: &str) -> Result<Self> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 模拟工厂进程编号（测试用：模拟进程意外退出）。
    pub fn child_pid(&self) -> Option<u32> {
        None
    }

    /// 在模拟工厂中创建仿真，并在引擎中登记（远程模式），同时取回标准答案交给审核层。
    ///
    /// 骨架：尚未实现。
    pub async fn create_simulation(
        &self,
        _engine: &Mutex<Engine>,
        _config: DesSimulationConfig,
    ) -> Result<String> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 逐分钟推进到 `to_minute`：模拟工厂推进 → 拉取事件与快照 → 交给引擎 → 引擎推进。
    ///
    /// 骨架：尚未实现。
    pub async fn tick(&self, _engine: &Mutex<Engine>, _to_minute: u64) -> Result<()> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 发出引擎中待发的写操作，回执写回动作，并刷新标准答案。返回本次收到的回执。
    ///
    /// 骨架：尚未实现。
    pub async fn dispatch_writes(&self, _engine: &Mutex<Engine>) -> Result<Vec<Value>> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 只发送、不记录回执（测试用：模拟"已发出但回执丢失"后的重发）。
    ///
    /// 骨架：尚未实现。
    pub async fn send_writes(&self, _writes: &[RemoteWrite]) -> Result<Vec<Value>> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 开放给 agent 与桌面的工具名（只有观测类）。
    pub fn agent_tools(&self) -> Vec<String> {
        vec![]
    }

    /// 以 agent 身份调用工具：不在 `agent_tools` 中的一律拒绝，不发给模拟工厂。
    ///
    /// 骨架：尚未实现。
    pub async fn call_agent_tool(&self, _tool: &str, _args: Value) -> Result<Value> {
        bail!("{E_DES_UNAVAILABLE}: connector is not implemented yet")
    }

    /// 已发出的写操作记录（工具名、参数、回执）。
    pub fn sent_writes(&self) -> Vec<Value> {
        vec![]
    }
}
