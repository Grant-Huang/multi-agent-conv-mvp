//! 模拟工厂（阶段⑤a，设计文档 `docs/mom/DES_CLOSED_LOOP.md` §11a）。
//!
//! 一个独立的车间仿真服务：像真实的 MES/EAM/QMS 一样提供观测、执行（外部写）、仿真控制与管理四类工具。
//! 进程内调用（`FactoryService::call`）与经 MCP（`an-des-mcp`）调用走同一个入口，保证两种模式结果一致。
//!
//! **当前状态：接口骨架**（阶段⑤a-1 Red）。仿真内核将从 `an-runtime::mom::des` 迁入本 crate。

pub mod service;

pub use service::{tool_catalog, FactoryService, ServiceConfig, ToolCategory, ToolSpec};
