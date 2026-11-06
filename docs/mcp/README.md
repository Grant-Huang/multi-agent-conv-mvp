# 接入配置好的 MCP

General 本身就是轻量 LLM 工具循环。新增 MCP 不需要安装 pi-dev，也不接管外部 coding agent 的工具系统。

1. 安装并检查你信任的 MCP 服务，确认工具名、网络和认证要求。
2. 参考 `config.example.toml` 添加 `[[mcp.servers]]` 与 `[[mcp.servers.tools]]`。每个工具显式指定逻辑能力 ID、描述和效果；工具名必须与服务发现一致。
3. 从仓库根目录启动 `cargo run --locked --bin agentnexus -- config.toml`。服务启动前完成握手与分页发现，配置缺失、Schema 无效或连接失败会报错，不能静默少注册能力。
4. 本地请求 `/api/capabilities` 查看 Schema 与 provider。工具别名为 `research.web_search` 等，General 从清单选择，调用前校验参数，结果写入现有任务证据。

stdio 使用直接命令与参数数组，工作目录默认继承服务目录，可显式设置 cwd。stdio 进程具备当前用户的系统权限，不是隔离沙箱；只应运行可信实现。调用期间保持同一服务串行、复用 MCP 会话，不同服务可并行。

HTTP 支持 Streamable HTTP，保留系统代理与 TLS 验证，不跟随重定向。除开发回环地址外要求 HTTPS；URL 不允许内嵌凭证、查询参数或 fragment。认证使用 bearer_env 引用环境变量；模型、卡片 API 和配置文件都不接收密钥值。平台环境需先配置相应域名的访问权限。本次没有自动添加公共 MCP 域名。

`effect` 是本地审查后的声明，不能根据服务 annotations 自动认定。`external_write` 和 `destructive` 可在卡片中表达但调用必定拒绝，不能用模型生成的低风险判断绕过。`workspace_write` 也不等于对第三方进程实施了 OS 文件隔离。

单次 timeout_ms 包含排队与调用。超时或任务取消后丢弃连接并终止直接 stdio 子进程，避免继续执行或复用脏会话；需要重启重新发现。远端可能已经受理操作，不自动重试，也不声称回滚。自定义 stderr 暂不转发，避免第三方服务泄露凭证。

当前接收文本、结构化 JSON 和 MCP 内容块；文本资源可进入现有内联 Artifact。图片/二进制块或链接仍是数据，尚无通用文件下载与预览，不会自动读取 file:// 或访问资源 URL。不把无法确认的远程路径宣称为可下载文件。Mermaid/PPTX 的真实提供方集成需先完成下一阶段产物存储，详见 PLAN.md。

验证：`cargo test --locked -p an-agents --test mcp`；`cargo test --locked -p an-server --test mcp`。这些测试使用真实 JSON-RPC 协议的本地 mock 服务，不依赖真实 LLM 或公共服务的免费额度。
