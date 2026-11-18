//! `an-des-mcp`：模拟工厂的 MCP 服务，经 stdio 通信，随调用方（AgentNexus 连接器、测试、实验脚本）在本机启动。
//!
//! - 工具目录与语义完全来自 `an_des::FactoryService`，本程序只做协议转换：成功时 `structuredContent`
//!   为工具返回的 JSON；业务错误（含未知工具）作为工具错误（`isError = true`、只有文字说明）返回，服务继续可用。
//! - 工具注解：观测类与管理类只读；执行类不是只读，且按幂等键幂等；控制类不是只读。
//! - 管理口令从环境变量 `AN_DES_ADMIN_TOKEN` 读取；未设置或为空时管理工具一律拒绝。
//! - 仿真计算是同步的，在阻塞线程中执行，不占用异步工作线程；同一时刻只处理一个调用（服务状态加锁）。
//! - stdin 关闭即退出；不启动子进程，不访问网络与文件。

use an_des::{tool_catalog, FactoryService, ServiceConfig, ToolCategory};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// 管理口令所在的环境变量。
const ADMIN_TOKEN_ENV: &str = "AN_DES_ADMIN_TOKEN";

/// MCP 服务：持有一个模拟工厂服务实例（每个 stdio 连接一个进程，因此每个连接一个实例）。
#[derive(Clone)]
struct DesServer {
    service: Arc<Mutex<FactoryService>>,
    tools: Arc<Vec<Tool>>,
}

impl DesServer {
    fn new(admin_token: Option<String>) -> Self {
        let tools = tool_catalog()
            .into_iter()
            .map(|spec| {
                let schema = match spec.input_schema {
                    Value::Object(map) => map,
                    _ => unreachable!("tool schemas are objects"),
                };
                let annotations = match spec.category {
                    ToolCategory::Observe | ToolCategory::Admin => {
                        ToolAnnotations::new().read_only(true)
                    }
                    ToolCategory::Execute => ToolAnnotations::new()
                        .read_only(false)
                        .destructive(false)
                        .idempotent(true),
                    ToolCategory::Control => ToolAnnotations::new().read_only(false),
                };
                let mut tool = Tool::new(spec.name, spec.description, schema);
                tool.annotations = Some(annotations);
                tool
            })
            .collect();
        Self {
            service: Arc::new(Mutex::new(FactoryService::new(ServiceConfig {
                admin_token,
            }))),
            tools: Arc::new(tools),
        }
    }
}

/// 业务错误：作为工具错误返回，只有文字说明，不带结构化数据。
fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

impl ServerHandler for DesServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("an-des-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Simulated factory (discrete-event simulation with assumed parameters; not a real plant). \
                 Observation tools are read-only. Execution tools are external writes that need an idempotency key \
                 and a human approval reference. Simulation control and administrative tools are for test schedulers \
                 and review only and must not be exposed to agents.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let service = self.service.clone();
        // 仿真计算在阻塞线程中执行；锁中毒（之前的调用崩溃）时报告为工具错误。
        let outcome = tokio::task::spawn_blocking(move || {
            let mut svc = service
                .lock()
                .map_err(|_| anyhow::anyhow!("simulated factory unavailable"))?;
            svc.call(&name, args)
        })
        .await
        .map_err(|_| McpError::internal_error("simulated factory worker failed", None))?;
        Ok(match outcome {
            Ok(value) => CallToolResult::structured(value),
            Err(e) => tool_error(format!("{e:#}")),
        }
        .into())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let admin_token = std::env::var(ADMIN_TOKEN_ENV)
        .ok()
        .filter(|t| !t.trim().is_empty());
    let running = DesServer::new(admin_token)
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    // stdin 关闭后会话结束，进程退出。
    running.waiting().await?;
    Ok(())
}
