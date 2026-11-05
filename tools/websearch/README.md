# agent-websearch

给 agent 用的网页搜索与抓取工具。三种入口，同一套逻辑：

- **CLI**：`websearch`，stdout 输出一行 JSON，任何能执行命令的 agent 都能用
- **MCP server**：`websearch-mcp`（stdio），支持 MCP 的 agent 直接挂载
- **Python 函数**：`from agent_websearch import api`

搜索用 DuckDuckGo（无需 API key）；抓取先在本机下载并转成 Markdown，失败再用 [Jina Reader](https://r.jina.ai) 兜底。设计见 [DESIGN.md](DESIGN.md)。

## 安装

需要 Python 3.13+。推荐用 [uv](https://docs.astral.sh/uv/)，不污染系统 Python：

```
uv tool install ./tools/websearch                       # CLI 与 Python 函数（核心约 35 MB）
uv tool install "./tools/websearch[mcp]"                # 再加 MCP server
uv tool install "./tools/websearch[mcp,extract]"        # 再加 trafilatura（正文提取质量更好）
```

不装可选组件时 CLI 和 Python 函数完整可用；`extract` 装了会自动启用。

## CLI

```
websearch search-fetch "瀑布式开发 优缺点"            # 推荐：一次完成搜索 + 抓取前 3 条的正文（每页 3000 字）
websearch search "llama.cpp kv-unified" -n 8         # 只要标题 / URL / 摘要
websearch fetch https://example.com --max-chars 8000 # 抓取单个网页
```

退出码：成功 0，业务失败 1，参数错误 2。日志写 stderr，不会混进 stdout 的 JSON。

输出统一为：

```json
{"status": "success", "data": {...}, "message": ""}
{"status": "error", "data": {"code": "blocked_url"}, "message": "不允许访问内网、本机或保留地址"}
```

`search-fetch` 的 `data`：

| 字段 | 含义 |
|---|---|
| `results` | 全部搜索结果（title / url / snippet / rank） |
| `pages` | 抓取成功的页面（content 是 Markdown，`method` 为 `local` 或 `jina`，`truncated` 表示被截断，`chars` 是截断前字数） |
| `failures` | 抓取失败的页面和原因；部分页面失败时 `status` 仍是 `success`，`message` 会提示 |

错误码：`invalid_argument`、`blocked_url`、`timeout`、`provider_error`、`fetch_failed`、`internal`。

## 接入 agent

**支持 MCP 的 agent**（例如 opencode，`opencode.json`）：

```json
{
  "mcp": {
    "websearch": {"type": "local", "command": ["websearch-mcp"], "enabled": true}
  }
}
```

提供三个工具：`web_search_and_fetch`（首选）、`web_search`、`web_fetch`。

**pi**：pi 没有内置联网搜索，也不支持 MCP，通过 skill 教它调用 CLI：

```
pi --skill ./tools/websearch/skills/websearch -p "..."
```

**AgentNexus**：计划在 M3 通过 CLI 接入（见 `docs/PLAN.md`）。

## 配置（环境变量）

| 变量 | 默认值 | 说明 |
|---|---|---|
| `WEBSEARCH_FETCH_ORDER` | `local_first` | `local_first` / `jina_first` / `jina_only` / `local_only`（不能把 URL 发给第三方时用 `local_only`） |
| `WEBSEARCH_EXTRACTOR` | `auto` | `auto` / `lxml` / `trafilatura` |
| `WEBSEARCH_SEARCH_TIMEOUT` | `30` | 搜索硬超时（秒） |
| `WEBSEARCH_FETCH_TIMEOUT` | `10` | 本机抓取超时（秒） |
| `WEBSEARCH_JINA_TIMEOUT` | `20` | Jina 超时（秒） |
| `WEBSEARCH_MAX_BYTES` | `3145728` | 单页下载上限 |
| `WEBSEARCH_MIN_CHARS` | `200` | 正文少于这个字数视为抓取失败 |
| `WEBSEARCH_ALLOW_PRIVATE` | `0` | 设为 `1` 才允许访问内网地址（仅供测试） |
| `WEBSEARCH_LOG_LEVEL` | `WARNING` | 日志级别 |
| `JINA_API_KEY` | 无 | 可选；不设置则用免费额度 |

非法值会回退默认值并在日志里警告。

## 安全与已知限制

- 只允许 http / https；拒绝内网、本机、链路本地、保留地址（含重定向的每一跳）。
- 已知限制：DNS 重绑定（校验和连接时的解析结果不同）未处理。
- 查询词通过 stdin 传给搜索子进程，不出现在进程列表里；日志不记录完整查询词、完整 URL 和 key。
- **DuckDuckGo 不稳定**：`ddgs` 对同一个查询会随机返回"无结果"或被限流。搜索子进程对此自动重试最多 3 次；实测 12 个查询中 11 个有结果，仍可能偶发空结果。
- 内置的 lxml 正文提取偏简单；需要更好的提取效果时安装 `[extract]`，或让 Jina 兜底。
- 抓取 PDF、需要执行 JavaScript 才显示内容的页面，本机方式会失败，依靠 Jina 兜底。

## 开发

```
uv sync
uv run pytest --cov            # 单元测试（默认不联网），覆盖率门槛 80%
uv run pytest -m live -s       # 真实联网测试
uv run flake8 src tests
```
