# agent-websearch 设计文档

> 对应 `docs/PLAN.md` 的 M2。状态：**已定稿（2026-11-05 第二版）**，第 7 节列出全部已确认事项。
> 目标：一个独立的 Python 包，给任何 agent 提供"网页搜索 + 网页抓取"，以后可以整体拆出去单独维护。

## 1. 需求汇总

### 1.1 已确认的决策

| 项 | 决策 |
|---|---|
| 形态 | 独立 Python 包，同时提供 **Python 函数**、**CLI**、**MCP server** 三种入口 |
| 搜索 | DuckDuckGo（`ddgs`，无需 API key） |
| 主要用法 | `search_and_fetch`：一次调用完成搜索 + 并发抓取前几条结果的正文（对应最初的 `agent_search_and_scrape`） |
| 抓取顺序 | 可配置：`local_first`（默认）/ `jina_first` / `jina_only` / `local_only`；M3 时 A/B 对比 |
| 依赖 | 核心只依赖 `ddgs` + `httpx`（加上 ddgs 自带的 `lxml`）；`trafilatura`、`mcp` 为可选安装 |
| 扩展 | 搜索来源可插拔，M3 再接入 Exa / Parallel 做 A/B |
| 环境 | Python 3.13.2，uv 管理 venv，不污染系统 Python |
| 质量 | TDD，pytest 覆盖率 ≥ 80%，flake8 通过 |

### 1.2 功能需求

1. `search_and_fetch`（主要用法）：
   - 搜索后并发抓取前 N 条结果，默认 3 条，每页截断到 3000 字。
   - 抓取失败的页面跳过，但在结果里列出 URL 和原因。
2. `search`：只返回搜索结果（标题、URL、摘要）。
3. `fetch`：抓取单个 URL 的正文 Markdown，去掉导航、广告等，默认截断到 8000 字。
4. CLI 输出 JSON，供 AgentNexus 的 Rust 端（M3）和脚本调用。
5. MCP server 暴露 `web_search_and_fetch`、`web_search`、`web_fetch` 三个工具，供 pi / opencode / Hermes 等 agent 直接调用。

### 1.3 非功能需求

- **不卡死**：任何一次调用都有硬超时。`ddgs` 底层的 primp 可能在持有 GIL 时挂住（Hermes 踩过），必须放到子进程里跑。
- **适配本地模型上下文**：8200 上 4 个槽位共享 64K，满载时每个槽位约 16K token。默认截断长度要保证一次工具返回不超过约 1 万字。
- **安全**：
  - 只允许 http / https。
  - 拒绝内网、本机、链路本地地址（防 SSRF），每次重定向都重新检查。
  - 限制下载大小。
- **隐私**：
  - 日志不记录完整查询词和 API key。
  - Jina 会把 URL 发给第三方，`local_only` 模式下完全不调用 Jina。
- **轻量**：
  - 核心安装约 11 个包、35 MB，且都有预编译的安装包。
  - 不装可选组件时，CLI 和 Python 函数完整可用。
- **可测试**：所有网络调用都能被 mock，默认测试不访问外网。

### 1.4 不做（v1 范围外）

- 结果缓存、多来源合并排序：放到 M3 视 A/B 结果再定。
- 需要执行 JavaScript 才能显示内容的网页（如 SPA）、PDF 本地解析：交给 Jina 处理。

### 1.5 实测依据

**搜索与抓取**（2026-11-04）：

| 测试 | 结果 |
|---|---|
| DDG 搜索 + 只用 Jina 抓取 | 中文站点 5/12 成功，其余 Jina 超时（10s） |
| 本机直接抓取 | gov.cn 正常；CSDN 返回 521（样本少，证据不足，所以抓取顺序做成可配置，留给 M3 对比） |
| Exa 托管 MCP | 4/4 成功，约 2 万字，0.4~1.9s |
| Parallel 托管 MCP | 4/4 成功，约 2.6 万字，约 1s |

**依赖体积**（2026-11-05，Python 3.13.2，macOS arm64，uv 实装）：

| 依赖组 | 包数 | 体积 | 说明 |
|---|---|---|---|
| ddgs + httpx | 11 | 35 MB | 已包含 lxml（19 MB） |
| trafilatura | 17 | 67 MB | 其中 babel 32 MB 是经 htmldate 间接引入的，抽正文用不到 |
| mcp | 28 | 27 MB | 含 uvicorn / starlette / cryptography，stdio 模式用不到 |

## 2. 总体架构

```
            ┌──────────────────── 入口层 ────────────────────┐
 agent  ──▶ │  MCP server (stdio, 可选)   CLI (JSON)   Python API │
            └───────────────────────┬────────────────────────┘
                                    ▼
             ┌────────────── 核心层 (api.py) ──────────────┐
             │ search_and_fetch()   search()   fetch()     │  参数校验、统一错误
             └───────┬──────────────────────────┬──────────┘
                     ▼                          ▼
     ┌── 搜索 providers ──┐        ┌──────── 抓取管线 ────────┐
     │ registry           │        │ url_guard (SSRF)         │
     │  └ ddg (子进程)    │        │ 顺序：local / jina 可配置│
     │  └ exa / parallel  │        │ local: httpx + extract   │
     │    （M3 预留）     │        │   extract: trafilatura   │
     └────────────────────┘        │   (若已安装) 否则 lxml   │
                                   │ jina: r.jina.ai          │
                                   └──────────────────────────┘
```

**分层原则：**
- 入口层只做参数转换和输出格式化。
- 三个入口调用同一套 `api` 函数。
- `search_and_fetch` 复用 `search` 和 `fetch`，不重复实现。

### 2.1 目录结构

```
tools/websearch/
├── DESIGN.md
├── README.md
├── pyproject.toml              # 包元数据、依赖与可选依赖、入口脚本、pytest/coverage 配置
├── .flake8
├── src/agent_websearch/
│   ├── __init__.py             # 导出 search_and_fetch, search, fetch, 数据类, 异常
│   ├── models.py               # SearchResult, Page, FetchFailure, SearchAndFetchResult
│   ├── errors.py               # WebSearchError 及子类
│   ├── settings.py             # Settings.from_env()
│   ├── api.py                  # 三个函数：校验 + 调度
│   ├── url_guard.py            # URL 校验、内网地址拦截
│   ├── extract.py              # HTML → Markdown：trafilatura（可选）/ lxml 内置实现
│   ├── fetch_local.py          # httpx 抓取，调用 extract
│   ├── fetch_jina.py           # Jina Reader
│   ├── providers/
│   │   ├── __init__.py         # provider 接口 + 注册表
│   │   ├── ddg.py              # 父进程侧：起子进程、超时、解析
│   │   └── _ddg_worker.py      # 子进程侧：真正调用 ddgs
│   ├── operations.py           # CLI 与 MCP 共用：调用 api，包装成统一 JSON（3.8）
│   ├── cli.py                  # websearch search-fetch / search / fetch
│   └── mcp_server.py           # websearch-mcp（需要安装 [mcp]）
├── skills/websearch/SKILL.md   # 给 pi 这类不支持 MCP 的 agent：教它用 bash 调 CLI
└── tests/
    ├── conftest.py
    ├── fixtures/               # 测试用 HTML 样本（文章页、带导航广告的页、表格、代码块）
    ├── test_models.py
    ├── test_settings.py
    ├── test_api.py
    ├── test_search_and_fetch.py
    ├── test_url_guard.py
    ├── test_extract.py
    ├── test_fetch_local.py
    ├── test_fetch_jina.py
    ├── test_provider_ddg.py
    ├── test_cli.py
    ├── test_mcp_server.py      # 未安装 mcp 时跳过
    └── test_live.py            # 访问真实网络，默认跳过（-m live 才跑）
```

## 3. 详细设计

### 3.1 数据模型（`models.py`，dataclass，可转 dict）

```python
@dataclass(frozen=True)
class SearchResult:
    title: str
    url: str
    snippet: str
    provider: str          # "ddg" / "exa" / ...
    rank: int              # 从 1 开始

@dataclass(frozen=True)
class Page:
    url: str               # 请求的 URL
    final_url: str         # 重定向后的 URL
    title: str
    content: str           # Markdown 正文
    method: str            # "local" / "jina"
    chars: int             # 截断前的正文字数
    truncated: bool
    elapsed_ms: int

@dataclass(frozen=True)
class FetchFailure:
    url: str
    code: str              # 错误码，见 3.3
    message: str

@dataclass(frozen=True)
class SearchAndFetchResult:
    query: str
    provider: str
    results: list[SearchResult]     # 全部搜索结果（含未抓取的）
    pages: list[Page]               # 抓取成功的页面，按搜索排名排序
    failures: list[FetchFailure]    # 抓取失败的页面
```

### 3.2 Python API（`api.py`）

```python
def search_and_fetch(query: str, max_results: int = 3, max_chars_per_page: int = 3000,
                     provider: str | None = None,
                     settings: Settings | None = None) -> SearchAndFetchResult: ...

def search(query: str, max_results: int = 8, provider: str | None = None,
           settings: Settings | None = None) -> list[SearchResult]: ...

def fetch(url: str, max_chars: int = 8000,
          settings: Settings | None = None) -> Page: ...
```

**参数校验**（不合法时抛 `InvalidArgument`）：

| 参数 | 规则 |
|---|---|
| `query` | 去掉首尾空白后非空，长度 ≤ 500 |
| `max_results` | 1 ~ 20（`search_and_fetch` 为 1 ~ 8，防止并发抓取过多） |
| `max_chars` / `max_chars_per_page` | 500 ~ 100000 |
| `url` | 长度 ≤ 2048，其余见 3.5 |
| `provider` | 必须是已注册的名称，默认取 `settings.provider` |

**`search_and_fetch` 流程：**
1. 调用 `search(query, max_results)`。搜索本身失败（超时、限流）时直接抛错，没有结果可抓。
2. 用 `concurrent.futures.ThreadPoolExecutor(max_workers=min(n, 4))` 并发调用 `fetch(url, max_chars_per_page)`。
3. 单页抛出的 `WebSearchError` 记入 `failures`，不中断整体；成功的页面按搜索排名放入 `pages`。
4. 所有页面都失败时仍然正常返回，`pages` 为空，调用方可以退而使用 `results` 里的摘要。
5. 最坏耗时约为搜索超时加单页最长抓取时间，默认 30s + (10s + 20s)。

三个函数都是同步的。MCP server 内部用 `anyio.to_thread.run_sync` 调用，避免阻塞事件循环。

### 3.3 错误（`errors.py`）

所有异常继承 `WebSearchError`，带 `code` 属性：

| 异常 | code | 场景 |
|---|---|---|
| `InvalidArgument` | `invalid_argument` | 参数不合法 |
| `BlockedUrl` | `blocked_url` | 非 http(s)、内网地址、重定向到内网 |
| `Timeout` | `timeout` | 搜索子进程或抓取超时 |
| `ProviderError` | `provider_error` | 搜索来源报错或被限流 |
| `FetchFailed` | `fetch_failed` | 按配置顺序尝试过的方式都失败，`message` 汇总每种方式的原因 |

### 3.4 搜索 provider（`providers/`）

```python
class SearchProvider(Protocol):
    name: str
    def search(self, query: str, max_results: int, timeout: float) -> list[SearchResult]: ...

def register(provider: SearchProvider) -> None: ...
def get_provider(name: str) -> SearchProvider: ...    # 未注册抛 InvalidArgument
def provider_names() -> list[str]: ...
```

**DDG 实现**（`ddg.py` + `_ddg_worker.py`）：
1. 父进程用 `subprocess.run([sys.executable, "-m", "agent_websearch.providers._ddg_worker"], input=json, timeout=...)` 启动子进程。查询通过 stdin 以 JSON 传入，不放在命令行参数里，以免被 `ps` 看到。
2. 子进程调用 `DDGS().text(query, max_results=n)`，把结果以 JSON 写到 stdout；出错时写 `{"error": "..."}`，以非零码退出。
3. 超时后 `subprocess.run` 会杀掉子进程，父进程抛 `Timeout`。默认 30s，与 Hermes 相同。
4. 把 `href/title/body` 映射成 `SearchResult`，跳过没有 URL 的结果，并按 URL 去重。
5. **重试（实测后新增）**：`ddgs` 的 auto 后端对同一个查询时好时坏，会随机报 "No results found" 或被限流。子进程内对"无结果 / 限流 / 超时"最多重试 3 次（间隔 1s），其他错误不重试；仍无结果时返回空列表，限流则报错。2026-11-05 实测 12 个查询 11 个有结果，平均 3.5s，最慢 8.3s。

Exa（`providers/exa.py`）调用托管 MCP `https://mcp.exa.ai/mcp` 的 `web_search_exa`，不需要 key。响应是 SSE，正文按 `Title` / `URL` / `Highlights` 解析成 `SearchResult`。页面正文不使用 Exa 附带的长文，仍走第 3.5 节的抓取管线。

Parallel 仍留到 M3：再写一个 provider 文件并注册，`api.py` 和入口层不用改。

### 3.5 URL 校验（`url_guard.py`）

```python
def check_url(url: str, allow_private: bool = False) -> None   # 不合法抛 BlockedUrl / InvalidArgument
```
1. `urllib.parse` 解析，scheme 只允许 `http` / `https`，必须有主机名。
2. 用 `socket.getaddrinfo` 解析主机名，任一地址满足以下条件就拒绝：`ipaddress` 的 `is_private`、`is_loopback`、`is_link_local`、`is_multicast`、`is_reserved`、`is_unspecified`。
3. 本机抓取时关闭自动重定向，手动跟随，最多 5 次，每一跳都重新调用 `check_url`。
4. 走 Jina 时同样先调用 `check_url`，避免把内网 URL 发给第三方。
5. `WEBSEARCH_ALLOW_PRIVATE=1` 可放行，仅供测试和内网部署使用，默认关闭。

已知限制：DNS 重绑定，即校验和真正连接时解析结果不同。v1 不处理，在 README 里写明。

### 3.6 抓取管线（`api.fetch`）

```
check_url ─▶ 按 fetch_order 依次尝试：
               local_first : local → jina
               jina_first  : jina  → local
               jina_only   : jina
               local_only  : local
             任一方式成功 ─▶ 截断 ─▶ Page(method=实际使用的方式)
             全部失败     ─▶ 抛 FetchFailed(每种方式的原因)
```

**本机抓取**（`fetch_local.py`）：
- 使用 `httpx.Client`，带浏览器 User-Agent，超时默认 10s，手动跟随重定向。
- 流式读取，超过 `max_bytes`（默认 3 MB）就停止。
- 按 Content-Type 处理：

| Content-Type | 处理方式 |
|---|---|
| `text/html` | 调用 `extract.html_to_markdown` |
| `text/plain`、`text/markdown`、`application/json` | 原样返回 |
| 其他（PDF、图片等） | 视为失败 |

- 以下情况都视为失败：HTTP 状态码 ≥ 400，正文少于 `min_chars`（默认 200 字），或连接 / 超时异常。

**正文提取**（`extract.py`）：

```python
def html_to_markdown(html: str, base_url: str) -> tuple[str, str]   # (title, markdown)
```

- `WEBSEARCH_EXTRACTOR=auto`（默认）：已安装 trafilatura 就用它，否则用内置的 lxml 实现。也可以强制指定 `lxml` 或 `trafilatura`。
- **内置 lxml 实现**（不新增依赖，lxml 已随 ddgs 安装）：
  1. `lxml.html` 解析；删除 `script/style/noscript/iframe/svg/form/nav/header/footer/aside`，以及 class/id 明显属于导航、侧栏、广告、评论的元素。
  2. 选正文根节点：优先 `<article>`，其次 `<main>`，否则取文本量最大的 `div/section`。
  3. 转 Markdown：
     - `h1`~`h6` 转为 `#` 标题；`p` 转为段落；`ul/ol/li` 转为列表；
     - `a` 转为 `[文字](绝对 URL)`；`pre/code` 转为代码块；`table` 转为简单的 Markdown 表格；`br` 转为换行；
     - 合并多余空白。
  4. 标题取 `<title>`，没有时取第一个 `h1`。
- 内置实现的目标是"够用"。正文质量不够时，再安装 `[extract]` 或依靠 Jina，M3 时会对比效果。

**Jina**（`fetch_jina.py`）：
- 请求 `GET https://r.jina.ai/{url}`，超时默认 20s。
- 环境变量中有 `JINA_API_KEY` 时，加上 `Authorization: Bearer ...`。key 不写日志，也不写进错误信息。
- 返回内容已经是 Markdown，标题取第一行的 `Title:`。

**截断**：
- `content` 截到 `max_chars`，尽量在段落边界截断，并设置 `truncated=True`。
- `chars` 记录截断前的字数，调用方可以据此决定是否单独再抓取完整页面。

### 3.7 配置（`settings.py`）

`Settings` 是一个 dataclass，`from_env()` 从环境变量读取，非法值回退到默认值，并在日志中给出警告。

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `WEBSEARCH_PROVIDER` | `ddg` | 默认搜索来源。已注册 `ddg`、`exa` |
| `WEBSEARCH_FETCH_ORDER` | `local_first` | `local_first` / `jina_first` / `jina_only` / `local_only` |
| `WEBSEARCH_EXTRACTOR` | `auto` | `auto` / `lxml` / `trafilatura` |
| `WEBSEARCH_SEARCH_TIMEOUT` | `30` | 搜索硬超时（秒） |
| `WEBSEARCH_FETCH_TIMEOUT` | `10` | 本机抓取超时（秒） |
| `WEBSEARCH_JINA_TIMEOUT` | `20` | Jina 超时（秒） |
| `WEBSEARCH_MAX_BYTES` | `3145728` | 本机抓取下载上限 |
| `WEBSEARCH_MIN_CHARS` | `200` | 正文少于这个字数视为失败 |
| `WEBSEARCH_ALLOW_PRIVATE` | `0` | 是否允许访问内网地址 |
| `WEBSEARCH_LOG_LEVEL` | `WARNING` | 日志级别 |
| `JINA_API_KEY` | 无 | 可选；不设置则使用免费额度 |

### 3.8 统一 JSON 输出（CLI 与 MCP 共用）

```json
{"status": "success", "data": {...}, "message": ""}
{"status": "error",   "data": {"code": "blocked_url"}, "message": "不允许访问内网地址"}
```

各命令 / 工具的 `data` 内容：

| 命令 / 工具 | `data` |
|---|---|
| `search_and_fetch` | `SearchAndFetchResult` 的全部字段 |
| `search` | `{"query": "...", "provider": "ddg", "results": [SearchResult...]}` |
| `fetch` | `Page` 的全部字段 |

`search_and_fetch` 有部分页面失败时，`status` 仍是 `success`，`message` 写明"N 个页面抓取失败"。

`cli.py` 和 `mcp_server.py` 共用一个 `to_envelope(fn)`，负责把返回值或 `WebSearchError` 转成上面的结构。其他未预期的异常转成 `code="internal"`，message 只给异常类型，不暴露堆栈。

### 3.9 CLI（`cli.py`，argparse，入口脚本 `websearch`）

```
websearch search-fetch "llama.cpp kv cache" [-n 3] [--max-chars 3000] [--provider ddg]
websearch search "llama.cpp kv cache" [-n 8] [--provider ddg]
websearch fetch https://example.com [--max-chars 8000]
```

- stdout 只输出一行 JSON（3.8 的结构），日志走 stderr。
- 退出码：成功为 0，`WebSearchError` 为 1，参数解析错误为 2（argparse 默认）。
- M3 的 Rust 端就按这个协议调用：执行子进程，读取 stdout 并解析 JSON。

### 3.10 MCP server（`mcp_server.py`，入口脚本 `websearch-mcp`，需要安装 `[mcp]`）

- 使用官方 `mcp` SDK（>=2.0）的 `MCPServer`（2.x 里 FastMCP 已改名），走 stdio 传输。stdout 留给协议使用，日志一律输出到 stderr。
- 没有安装 `mcp` 时，运行 `websearch-mcp` 会提示安装命令并以退出码 1 退出。
- 提供三个工具：

| 工具 | 参数 | 说明（给模型看的描述） |
|---|---|---|
| `web_search_and_fetch` | `query: str`, `max_results: int = 3`, `max_chars_per_page: int = 3000` | **首选**。搜索并返回前几条结果的正文 |
| `web_search` | `query: str`, `max_results: int = 8` | 只返回标题、URL、摘要，用于先看有哪些来源 |
| `web_fetch` | `url: str`, `max_chars: int = 8000` | 抓取指定网页正文，返回 Markdown |

- 返回值是 3.8 的 JSON 字符串。错误也以 `status: error` 返回，不抛协议层异常，这样模型能读到原因。
- pi / opencode 的接入配置示例写在 README 里。

### 3.11 日志

- 使用标准库 `logging`，logger 名为 `agent_websearch`，级别由 `WEBSEARCH_LOG_LEVEL` 控制。
- INFO 级别只记录：
  - provider 名、结果数、耗时；
  - 抓取的主机名（不含路径和查询参数）、使用的方式、耗时。
- 完整查询词和完整 URL 只在 DEBUG 级别输出；任何级别都不输出 key。

## 4. 关键技术与第三方依赖

优先使用标准库：`subprocess`、`concurrent.futures`、`json`、`argparse`、`ipaddress`、`socket`、`urllib.parse`、`logging`、`dataclasses`。

| 依赖 | 安装方式 | 用途 | 推荐理由 |
|---|---|---|---|
| `ddgs` | 核心 | DuckDuckGo 搜索 | 原 `duckduckgo_search` 改名后的延续版本，无需 key；旧包已停止维护 |
| `httpx` | 核心 | 本机抓取、Jina 请求 | 超时可细分到连接和读取，支持流式读取和手动重定向，测试可用 respx mock；标准库 urllib 做不到按字节数限流，也不好 mock |
| `lxml` | 核心（随 ddgs 安装） | 内置正文提取 | ddgs 已经依赖它，显式声明不增加任何体积 |
| `trafilatura` | 可选 `[extract]` | 高质量正文提取 | 正文抽取效果好，但多 17 个包、约 67 MB，所以作为可选 |
| `mcp` | 可选 `[mcp]` | MCP server | 官方 Python SDK；多 28 个包、约 27 MB，只有需要 MCP 时才装 |

开发依赖：`pytest`、`pytest-cov`、`respx`（mock httpx）、`flake8`。

版本在 `pyproject.toml` 中设定下限，`uv.lock` 锁定精确版本并提交。

**部署方式（都不需要 Docker）：**

```
uv tool install ./tools/websearch               # 只要 CLI / Python 函数（AgentNexus 用这个）
uv tool install "./tools/websearch[mcp]"        # 还需要给 pi / opencode 用 MCP
uv tool install "./tools/websearch[mcp,extract]"  # 全部功能
```

以后独立成仓库后，可以改成 `uv tool install git+https://...`，或者发布到 PyPI。

## 5. 测试策略（TDD）

- **先写测试，冻结后只改业务代码**。网络全部 mock：
  - httpx 用 respx；
  - DDG 子进程用 monkeypatch 替换 `subprocess.run`，另有一个测试真实启动 worker，并在 worker 内 mock `DDGS`；
  - `getaddrinfo` 用 monkeypatch 替换。
- **需要覆盖的关键用例：**

| 模块 | 用例 |
|---|---|
| api | 各参数边界（0、1、上限、上限 +1，空串，501 字）；provider 未注册 |
| fetch 顺序 | 四种 `fetch_order` 各自的尝试顺序；第一种方式成功时不调用第二种；全部失败时 message 包含每种方式的原因；`local_only` 时完全不请求 jina.ai |
| search_and_fetch | 并发抓取后按排名排序；部分失败时记入 failures 而 status 仍为 success；全部失败时 pages 为空但 results 保留；搜索失败直接报错；每页按 `max_chars_per_page` 截断 |
| url_guard | `file://`、`ftp://`、无主机名；127.0.0.1、10.x、192.168.x、169.254.x、`::1`、域名解析到内网；`allow_private` 放行；走 Jina 前也会校验 |
| extract | 文章页只保留正文；去掉导航和页脚；标题、列表、链接（转为绝对 URL）、代码块、表格的转换；`auto` 模式下有无 trafilatura 时的选择 |
| fetch_local | text/plain 直通；PDF 视为失败；4xx/5xx；超过 max_bytes；正文太短；重定向到内网被拦；重定向超过 5 次 |
| fetch_jina | 带 / 不带 key 的请求头；错误信息里不含 key；超时 |
| ddg | 正常解析；去重；超时抛 Timeout；子进程报错抛 ProviderError；非法 JSON |
| cli / mcp | 成功和错误时的 JSON 结构；退出码；stdout 只有一行 JSON；未安装 mcp 时给出提示 |
| 截断 | 在段落边界截断；`chars` 与 `truncated` 正确 |

- `test_live.py` 标记为 `@pytest.mark.live`，默认跳过。手动运行 `pytest -m live`，会真实搜索 4 个固定查询，并抓取 gov.cn、CSDN、GitHub 各一页，分别用 `local_first` 和 `jina_first` 运行。
- 门槛：`pytest --cov=agent_websearch --cov-fail-under=80`，以及 `flake8 src tests`。

## 6. 开发步骤（细化 PLAN.md 的 M2）

每一步完成后更新 `docs/PLAN.md` 并等待确认。

| 步骤 | 内容 | 产出 |
|---|---|---|
| 2.1 | 本设计文档确认 | `DESIGN.md` 定稿 ✅ |
| 2.2 | 包骨架：`pyproject.toml`（核心依赖 + `[extract]`、`[mcp]` 可选依赖）、uv venv、flake8 配置；models / errors / settings 及其测试 | `uv run pytest` 可运行 |
| 2.3 | `url_guard` + `extract`（lxml 内置实现，trafilatura 可选）（先写测试） | 正文提取完成 |
| 2.4 | `fetch_local` + `fetch_jina` + 可配置顺序的 `api.fetch`（先写测试） | 抓取管线完成 |
| 2.5 | provider 接口 + DDG 子进程实现 + `api.search` + `api.search_and_fetch`（先写测试） | 搜索与一次调用完成 |
| 2.6 | CLI + 统一 JSON 输出 + MCP server（先写测试） | `websearch`、`websearch-mcp` 可用 |
| 2.7 | 覆盖率 ≥ 80%、flake8、live 测试实测、README（安装、CLI、MCP 接入 pi / opencode 的示例） | 验收 |

## 7. 已确认事项（2026-11-05）

1. **包名**：发布名 `agent-websearch`，模块名 `agent_websearch`，命令 `websearch` / `websearch-mcp`。
2. **依赖**：
   - 核心只有 `ddgs` + `httpx`（加上随 ddgs 安装的 `lxml`），正文提取用内置的 lxml 实现；
   - `trafilatura` 为可选 `[extract]`，`mcp` 为可选 `[mcp]`。
3. **抓取顺序**：可配置，默认 `local_first`，M3 时对比 `jina_first` 和 `local_first`。
4. **一次调用**：新增 `search_and_fetch`（MCP 工具 `web_search_and_fetch`），作为 agent 的首选工具：
   - 默认取 3 条结果，每页 3000 字；
   - 并发抓取，失败的页面跳过但列出原因。
5. **默认值**：
   - `search` 返回 8 条，`fetch` 截断 8000 字；
   - 搜索超时 30s，本机抓取 10s，Jina 20s。
6. **Jina**：默认参与抓取（作为 `local_first` 的第二选择）；不能外发 URL 时设置 `WEBSEARCH_FETCH_ORDER=local_only`。
