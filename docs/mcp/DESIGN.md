# MCP 能力目录与轻量执行 Harness

日期：2026-11-05（America/Vancouver）。基线：远端 master `929301a`。

## 设计审查结论

复用 General 的 LLM + 工具循环，模型选择工具，程序执行。pi-dev / Codex 保留代码仓库任务；不为搜索、图表和办公文件启动外部 coding agent。不改 AgentAdapter、现有 AgentCard、任务状态机、数据库或审批策略。与 MOM 设计的工具网关兼容，但本次不实现 Master、公告板、DAG 或子 agent。

原讨论中需修正的假设：MCP 不保证免密钥、服务可信、文件可下载或工具只读；tools/list 不能直接充当权限声明。能力到工具是多对多关系，不能把一个工具强制当成一个完整业务流程。自动检索、安装陌生服务、热更新目录和跨 provider 自动回退均不属于首期。没有真实模型的测试不能证明真实路由质量。

## 目录与契约

采用 MCP 官方 Rust SDK，支持 stdio 和 Streamable HTTP。服务器配置是本地信任入口，明确列出允许的工具及其能力 ID、描述、效果、输入输出媒体类型；拒绝重复 ID、重复工具绑定、空授权、无效预算。MCP 发现提供真实 inputSchema/outputSchema，编译为离线 JSON Schema 校验器，禁止远程引用解析。工具别名为 server_id + '.' + tool_name，拒绝歧义字符。描述、输出和 annotations 都是不可信数据，不能提升权限或触发安装。

能力目录统一包括内置工具与 MCP 工具，HTTP GET /api/capabilities 提供完整卡片；保持 /api/agents 返回格式兼容。单个工具卡片具有逻辑 capability_id 和具体 tool_id；同一逻辑能力可有多个显式工具绑定。首期固定启动快照，重启后重新发现，不承诺热刷新或智能检索。Router / Conversation 从同一已连接目录生成清单，General 使用同一批工具及 Schema，MCP 服务不在线则启动失败并明确指出服务器 ID，不能静默删除能力。

配置新增 [[mcp.servers]]，默认空，旧配置继续可用。异步启动完成 initialize → initialized → tools/list（分页）→ 本地允许清单过滤，再构建 Runtime。缺少已配置工具、无效 Schema 或过大的目录均拒绝启动。同步 Runtime 构造器遇到非空 MCP 配置明确报错，防止测试与生产行为不一致。

## 调用规范

保持现有 general_step 的 action/tool/args/answer。harness 通过 ToolRegistry::call 校验工具及参数，然后调用适配器；MCP 返回统一 JSON envelope：status、data、artifacts、error。内部参数、服务凭证不进入模型。服务端 isError 和协议错误不会转成成功；声明 outputSchema 时验证 structuredContent。错误只输出稳定错误码，不透出 SDK 原始错误或凭证。

单个服务串行复用会话，不同服务可并发。等待锁、执行和结果处理受单次总超时约束。会话在调用期间从池中取出；超时或外层任务取消导致会话被释放、SDK 关闭传输，harness 立即终止直接子进程并异步 wait 回收，不能把仍执行中的连接放回池。失效后明确报 unavailable，需重启重新发现；关闭 SDK 的 HTTP session-expiry 自动重放，不自动重试业务写操作，不自动换源。远端取消是尽力而为，超时结果属于完成状态未知，不能声称回滚。不承诺任意后代进程树终止。

本地允许效果为 read_only / workspace_write；external_write / destructive 首期拒绝调用，即使模型把整个任务判断为低风险。现有任务审批不能代替未来逐次、绑定参数的外部写操作审批。stdio 是受信任本机程序，cwd 不是沙箱；首期不声称隔离进程文件系统或网络。不要接入未经审查的本地命令。HTTP 使用 HTTPS、平台代理、TLS 验证、禁止重定向；本地回环 HTTP 只用于开发。Bearer 凭证只引用环境变量名。不得在配置中保存密钥或将真实值暴露到目录 API。网络出站仍由环境策略执行。

结果受大小上限约束，文本只作为观察。文本资源可作为内联 Artifact 保存；图片、二进制资源与链接保留 MCP 结构，但不自动下载或读取 file://，避免 SSRF 和任意文件读取。当前 Artifact 是文本内联，不支持通用二进制存储，不能宣称 PPTX 已可下载。二进制存储、下载授权、文件预览与格式验证是下一阶段前置条件。

## 分阶段交付与验收

1. 本期：卡片、stdio/HTTP 客户端、允许清单、Schema 校验、超时取消、统一结果、Runtime/General 接线、目录 API、配置示例。以真实 JSON-RPC mock MCP 协议验证，保留内置工具兼容。
2. 产物阶段：新增二进制 Artifact Store、下载与预览、MIME/格式验证，接入固定版本 Mermaid 和 PPTX 提供方。验证搜索→图表→PPTX，明确真实与 mock；服务选择需核实来源和依赖，不能凭包名安装。
3. 按需加载：能力检索、任务固定快照、工具列表更新、按任务可见范围及只读 Reviewer 工具。目录规模增大前，限制服务器和工具数量，避免提示词无限膨胀。
4. 外部写入：持久化调用记录与参数绑定审批、幂等/受理查询和不确定结果恢复。完成前拒绝外部写入。

测试先写后实现：配置与允许清单拒绝、分页发现、参数错误不发送、isError、输出 Schema、超时不重试、文本资源产物、会话复用、General 工具调用、目录 API、HTTP 握手。最终执行工作区测试、Clippy、格式检查、前端测试和构建。结果记录在 PLAN.md，不将未接入的公共服务记为完成。

## 错误码

| 错误码 | 含义与处理 |
|---|---|
| E_MCP_CONFIG | 配置无效；检查服务器定义、命令及允许工具 |
| E_MCP_CONNECT | 连接或子进程启动失败；检查命令、权限和服务状态 |
| E_MCP_AUTH | 认证失败；检查已配置的凭证与权限 |
| E_MCP_DENIED | 工具被策略拒绝；检查允许清单，不自动放宽权限 |
| E_MCP_DISCOVERY | 工具发现失败；检查服务能力与连接日志 |
| E_MCP_UNAVAILABLE | 工具或服务不可用；检查注册状态与启动错误 |
| E_MCP_ARGUMENTS | 工具参数不合格；按工具 schema 修正输入 |
| E_MCP_SCHEMA | 工具 schema 无效或不支持；检查服务返回定义 |
| E_MCP_PROTOCOL | 协议交互失败；检查 MCP 版本与服务日志 |
| E_MCP_TIMEOUT | 调用超时；检查服务响应和超时配置，写操作先查回执 |
| E_MCP_TOOL | 工具执行报错；查看工具返回的具体错误 |
| E_MCP_OUTPUT | 输出不合格或超过上限；检查输出类型与大小 |
