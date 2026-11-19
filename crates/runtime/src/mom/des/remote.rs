//! 阶段⑤a-2：远程模拟工厂模式（设计文档 §11a，"⑤a-2 设计"）。
//!
//! 仿真在独立的模拟工厂（`an-des-mcp`）中运行，引擎里只登记仿真、不保存重放输入，因此**不会回退到进程内仿真**。
//! 服务层的连接器（`an_server::mom_des_connector`）负责搬运：
//! - 推进前：把模拟工厂已发生的事件与当时的可观测快照交给引擎（`des_ingest_remote`），引擎照常按虚拟时钟投递；
//! - 执行后：引擎把审批后的写操作排进待发清单（`des_pending_writes`），连接器带幂等键发给模拟工厂，回执写回动作；
//! - 审核层的标准答案由连接器从模拟工厂管理入口读取后交给引擎（`des_set_remote_truth`）。
//!
//! 引擎本身保持同步、确定、可重放；所有异步调用都在服务层。
//!
//! **当前状态：接口骨架**（阶段⑤a-2 Red）。

use super::DesSimulationConfig;
use crate::mom::{Engine, EventInput};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一个待发给模拟工厂的写操作（只在人工审批并 `execute` 之后产生）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteWrite {
    /// 幂等键：`案例:方案修订号:动作`，重发时不变。
    pub key: String,
    /// 模拟工厂的执行类工具名，例如 `maintenance_order_create`。
    pub tool: String,
    /// 工具参数（已含 `simulation_id`、`idempotency_key`、`approval_ref`）。
    pub args: Value,
}

impl Engine {
    /// 登记一个在模拟工厂中运行的仿真（编号由模拟工厂分配）。不保存重放输入。
    ///
    /// 骨架：尚未实现。
    pub fn create_remote_des_simulation(
        &mut self,
        _simulation_id: &str,
        _config: DesSimulationConfig,
    ) -> Result<()> {
        bail!("remote DES simulations are not implemented yet")
    }

    /// 交给引擎：模拟工厂中已发生的事件（带仿真编号前缀），以及当前时刻的可观测快照
    /// （`equipment_status` 的结果）。之后的 `advance` 按时刻投递这些事件。
    ///
    /// 骨架：尚未实现。
    pub fn des_ingest_remote(
        &mut self,
        _simulation_id: &str,
        _events: Vec<EventInput>,
        _observation: Value,
    ) -> Result<()> {
        bail!("remote DES ingestion is not implemented yet")
    }

    /// 尚未收到回执的写操作。
    ///
    /// 骨架：尚未实现。
    pub fn des_pending_writes(&self) -> Vec<RemoteWrite> {
        vec![]
    }

    /// 记录模拟工厂对某个写操作的回执，更新对应动作（完成时刻或失败）。
    ///
    /// 骨架：尚未实现。
    pub fn des_record_write_receipt(&mut self, _key: &str, _receipt: Value) -> Result<()> {
        bail!("remote write receipts are not implemented yet")
    }

    /// 写入从模拟工厂管理入口读到的标准答案，供审核层使用。
    ///
    /// 骨架：尚未实现。
    pub fn des_set_remote_truth(&mut self, _simulation_id: &str, _truth: Value) -> Result<()> {
        bail!("remote ground truth is not implemented yet")
    }
}
