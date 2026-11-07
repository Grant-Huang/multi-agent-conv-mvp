//! AgentNexus 的三个执行 Agent。
//!
//! | Agent    | 接入方式                    | 验证的能力                   |
//! |----------|-----------------------------|------------------------------|
//! | General  | 内置：LLM + 工具循环         | 内置 agent loop、工具调用     |
//! | Coding   | 外部：Codex CLI 子进程       | 外部 Harness 适配、超时、取消 |
//! | Reviewer | 内置：只读评估               | Worker 完成 ≠ 验收通过        |
//! | ACP      | 外部：任意支持 ACP 的 agent  | 一个适配器接入所有 agent      |
//! | Remote   | 远程节点上的 agent           | 跨机器执行                    |

pub mod a2a;
pub mod acp;
pub mod capability;
pub mod coding;
pub mod general;
pub mod mcp;
pub mod proc_group;
pub mod remote;
pub mod report;
pub mod reviewer;
pub mod tools;
pub mod util;

#[cfg(all(test, unix))]
mod test_proc;

pub use acp::{AcpAgent, AcpConfig, PermissionMode};
pub use coding::{CodingAgent, CodingConfig, SLOT_ENV};
pub use general::GeneralAgent;
pub use remote::{NodeClient, NodeConn, RemoteAgent};
pub use reviewer::ReviewerAgent;
pub use tools::ToolRegistry;
