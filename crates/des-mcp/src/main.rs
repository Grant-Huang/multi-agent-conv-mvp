//! `an-des-mcp`：模拟工厂的 MCP 服务，经 stdio 通信，随调用方（AgentNexus 连接器、测试、实验脚本）在本机启动。
//!
//! - 工具目录与语义完全来自 `an_des::FactoryService`，本程序只做协议转换：成功时 `structuredContent`
//!   为工具返回的 JSON；业务错误作为工具错误（`isError = true`）返回，服务继续可用。
//! - 管理口令从环境变量 `AN_DES_ADMIN_TOKEN` 读取；未设置时管理工具一律拒绝。
//! - stdin 关闭即退出；不启动子进程，不访问网络与文件。
//!
//! **当前状态：骨架**，尚未实现（RT-DESM-01～03 应为 Red）。

fn main() {
    eprintln!("an-des-mcp: not implemented yet");
    std::process::exit(2);
}
