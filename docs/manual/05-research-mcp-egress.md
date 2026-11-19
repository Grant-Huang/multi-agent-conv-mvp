# 05 联网研究、MCP 工具与外发脱敏

## 场景 A：让系统写一份带公开资料的报告

在对话里说：“写一份报告，引用公开资料/最新信息，保存成 Markdown。”

条件：General 能联网，需要 `websearch` 命令在 PATH 上（`[agents.general].websearch_command`，找不到就不会注册联网工具）。

系统的处理方式取决于你同时要求了什么：

| 你的要求 | 执行方式 |
|---|---|
| 联网搜索 **+** 写文件（典型研究报告） | 走固定流程：规划 → 检索（最多 6 次、去重）→ 各章节并行写作 → 汇合 |
| 只要求联网搜索 | 逐步工具循环：先搜后写 |

报告流程的要点：

- 过半章节成功才算完成；
- 无效引用会被剔除；
- 取消后不再发起新请求；
- 可调参数：`report_max_searches`（默认 6）、`report_parallel`（同时写几章，默认 3，会给其他任务留 1 个后台槽位）、`websearch_timeout_secs`。

> 报告质量与搜索时效取决于真实模型和搜索服务，系统的自动化测试使用受控替身，不能代表真实质量。

## 场景 B：接入 MCP 工具

General 可以通过 MCP（stdio 或 Streamable HTTP）调用你**信任且已安装**的工具服务。

1. 确认服务的工具名、网络与认证要求。
2. 在 `config.toml` 增加（以 stdio 为例）：

   ```toml
   [[mcp.servers]]
   id = "research"
   transport = "stdio"
   command = "tools/websearch/.venv/bin/websearch-mcp"
   args = []
   timeout_ms = 30000
   [[mcp.servers.tools]]
   name = "web_search"
   capability_id = "research.search"
   description = "搜索网页，返回标题、链接和摘要"
   effect = "read_only"     # read_only / workspace_write
   ```

   HTTP 服务改用 `transport = "http"`、`url = "https://…"`，密钥用 `bearer_env` 引用环境变量名，**不要把密钥写进配置**。
3. 启动 `cargo run --locked --bin agentnexus -- config.toml`。启动时会握手并发现真实工具 Schema；配置缺失、Schema 无效或连接失败会**直接报错**，不会静默少注册。
4. 访问 `GET /api/capabilities` 查看已连接的能力卡片、provider 与输入输出 Schema。

安全边界：

- 只开放你在配置里显式列出的工具；调用前会校验参数。
- `external_write`（对外写）和 `destructive`（删除）目前**一律拒绝**。
- `effect` 是你审查后的声明，不是自动判定；`workspace_write` 也不是操作系统级隔离。
- stdio 进程拥有当前用户权限，只运行可信实现。
- 单次调用超时或任务取消后，连接被丢弃并终止直接子进程；**不自动重试**，也不声称回滚。
- 目前支持文本与结构化 JSON 结果，文本资源可进入任务产物；图片/二进制尚无通用下载。

详见 [MCP 接入说明](../mcp/README.md)、[设计](../mcp/DESIGN.md)。

## 场景 C：开启外发脱敏（DLP）

目的：向外部（云端 LLM、联网工具）发送内容前，先经过 `dlp-egress` 检查，防止泄露凭证、个人信息等。

```toml
[egress]
mode = "audit"          # off（默认）/ audit（检查并记录，从不拦截）/ enforce（拦截，只发脱敏内容）
timeout_ms = 2000
dlp_command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
dlp_args = ["--config", "tools/dlp-egress/config/dlp.example.yaml"]

[[egress.tools]]        # 登记哪些工具调用要检查
tool = "web_search"
destination_id = "public-search"
channel = "external_api"
purpose = "web_research"

[[egress.llm]]          # 非本机地址的 LLM 后端必须登记
backend = "cloud"
destination_id = "cloud-llm"
purpose = "llm_inference"
```

建议先用 `audit` 观察一段时间，再切 `enforce`。`enforce` 下的规则：

- 只发送**脱敏后的内容**；
- 判定为需复核、拦截，或 DLP 自身异常时，**一律不发送**；
- DLP 工具**不会**出现在模型可见的工具清单里（不要把它配成 `[[mcp.servers]]`）；
- 云端 LLM 的输出里的占位符在本机还原；
- 非本机 LLM 后端未登记时，`enforce` 下拒绝启动。

部署与评测见 [DLP 外发部署](../mcp/DLP_EGRESS_DEPLOYMENT.md) 与 `tools/dlp-egress/docs/`。真实 dlp-egress 的联调验收依赖工具侧改造，见功能地图相应条目。
