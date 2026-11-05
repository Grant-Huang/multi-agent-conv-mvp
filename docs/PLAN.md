# AgentNexus MVP 开发计划与 TodoList

> 最后更新：2026-11-05（pre-M3 方案确认、35B 改 6 槽位 = 前台 2 + 后台 4、不借用、9B 下线）
> 规则：按里程碑逐步推进；每完成一步更新本文件（勾选进度、做必要的最小调整），用户确认后再进入下一步。
> 新功能先写测试（TDD），测试冻结后只改业务代码；Python 部分覆盖率 ≥ 80%，通过 flake8。

## 0. 当前环境

| 项目 | 配置 |
|---|---|
| 所有 LLM 角色（前台判断 / 回复 / 汇总、路由、General、审查） | llama.cpp `127.0.0.1:8200`，Qwen3.8-35B-A3B Q4_K_M；**6 个槽位**（目标；现在运行的进程还是 5 个，pre-M3.3 开始前重启）共享 262144 上下文池（`--kv-unified`，多开槽位不增加 KV 内存），K/V 缓存 `q4_0` + `-fa on`；计划：前台用 slot 0~1，后台用 slot 2~5，互不借用（pre-M3.3） |
| coding agent | pi（`~/.npm-global/bin/pi`），只用 8200 上的 35B；pi 的 `contextWindow` 已同步为 262144 |
| 9B（Qwen3.8-9B Q5_K_M） | **已下线**（2026-11-05，释放内存）；需要时通过 llama.cpp router 按请求唤起，不常驻 |
| 启动方式 | 8200 是手动启动的独立进程（不是 launchd）：`scripts/llama_server_35b.sh`（环境变量可覆盖参数，`LLAMA_LOG` 指定日志文件）；日志 `~/models/llama_8200.log` |
| llama.cpp | `0.6.0-dev (commit 23b0202)`；**流式请求断开会取消生成，非流式不会**；槽位参数名 `id_slot`（2026-11-05 实测） |
| 后端 / 前端 | `agentnexus` 监听 8787；前端 `web/`（Vite + React） |
| 构建 | 所有 cargo 命令加 `CARGO_TARGET_DIR=target` |

## 1. 里程碑总览

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 | 本地部署 + e2e 发现的 5 个问题 + 派发边界规则 | ✅ 完成 |
| M1 | 派发"越用越聪明"：纠错反馈、相似经验检索、纠错回归 | 待开始 |
| M2 | 独立的网页搜索工具包（Python，MCP / CLI / 函数） | 2.1~2.7 完成，2.8 待定 |
| pre-M3 | 稳定性加固：流式统一、槽位调度、上下文预算、前端 / 存储（已确认提前） | pre-M3.0 ~ .3 完成，.4 起待做 |
| 设计评审 | 能力卡片与路由扩展（会话 / OCR / PPT·文档 / 画图；只出设计、等确认） | pre-M3 之后、M3 之前 |
| M3 | 搜索接入 General agent + 多方案 A/B | 待开始 |
| M4 | 多问题消息逐条对应回答 | 待开始 |
| M5 | 9B vs 35B A/B（路由判断 + 最终回答） | 暂停：9B 已下线（2026-11-05），初步结果留档 |
| M6 | 复杂话题 e2e 综合评估 + 10 用户压测 | 待开始 |

顺序：M2 → **pre-M3** → M3 → M4 → M6，M1 可与 M2 并行（互不依赖）。
M6 依赖 M3、M4、pre-M3 完成；M5 暂停，需要时再按请求唤起 9B 复测。

---

## M0 已完成（2026-11-04）

- [x] 本地部署，接入 8200 本地模型，跑通端到端
- [x] 问题 1：失败交付消息说明真实原因（`composer.rs` 带上进程输出末尾，材料里看不出原因就说未知）
- [x] 问题 2：coding 的 git diff 只包含本次任务的改动（`util.rs` 用临时索引在任务前做快照，不动真实索引）
- [x] 问题 3：前端渲染 Markdown（react-markdown + remark-gfm，不渲染原始 HTML，避免 XSS）
- [x] 问题 4：前台不夸大能力（回答提示词列出真实能力，没有的能力明确说没有）
- [x] 问题 5：coding 使用本地模型（`CodingConfig` 支持 id / name / agent_type，接入 pi）
- [x] 第 1 层边界规则：网页（HTML/CSS/JS）、脚本、程序等代码产物 → coding；Markdown / 纯文本文档 → general；前台不自行指定保存位置
  - 评测集新增 E2 组 5 条边界样本（`art-01`~`art-05`），共 95 条
  - 35B 边界子集 29 条：角色 100%，动作 96.6%，整体 89.7%
  - 真实接口验证：瀑布式开发 HTML 网页 → coding（pi）生成 693 行页面，审查通过
- [x] 35B 全量路由基线（改边界规则前的提示词，90 条）：整体 88.9%，动作 95.6%，角色 96.1%，类别 95.8%，判断延迟 P50 4.9s / P95 27.0s

---

## M1 派发越用越聪明

现状：派发全靠 LLM 判断（Turn Controller → Router → Policy），没有正则；判断错了不会留下任何经验。

- [ ] **1.1 纠错反馈**
  - 前端任务卡片加"派错了"按钮，可选正确角色 / 动作 + 一句说明
  - 新表 `routing_feedback`（消息 id、原判断、正确判断、说明、时间），附数据库迁移说明
  - API：`POST /api/feedback`，统一返回 `{status, data, message}`，参数校验
  - 测试：存取、非法参数拒绝
- [ ] **1.2 相似经验检索（few-shot）**
  - 判断前从纠错表中取与当前消息最相似的 3~5 条，作为示例拼进 Turn / Router 提示词
  - 相似度先用字符 n-gram（标准库实现，不引入向量库）；效果不够再评估 embedding
  - 测试：检索排序、提示词包含示例、无纠错时提示词不变
- [ ] **1.3 纠错进入回归集**
  - 提供导出命令，把纠错记录转成 `evals/routing.jsonl` 格式，人工确认后合入 `make_dataset.py`
  - 每次改提示词跑一遍回归，对比基线
- [ ] **1.4 验收**：用 10 条故意构造的误派消息，纠错后再发相似消息，派发正确率提升

---

## M2 网页搜索工具包（独立 Python 包）

决策（已确认）：Python 包，同时提供 MCP server、CLI、Python 函数；任何 agent 都能调；以后可独立成项目。

- 位置：`tools/websearch/`（独立 `pyproject.toml`，uv + venv，Python 3.13.2）
- 依赖（已确认）：核心 `ddgs` + `httpx`（lxml 随 ddgs 安装）；可选 `[extract]` trafilatura、`[mcp]` mcp
- 测试放 `tools/websearch/tests/test_*.py`，网络调用全部可 mock
- 步骤与 `tools/websearch/DESIGN.md` 第 6 节一致

- [x] **2.1 需求与接口文档**：`search(query, max_results) -> list[Result]`、`fetch(url, max_chars) -> Page`；统一错误结构
  - 已定稿：`tools/websearch/DESIGN.md` 第二版
    - 包名 agent-websearch；核心依赖只有 ddgs + httpx，trafilatura / mcp 为可选安装
    - 抓取顺序可配置，默认本机优先
    - 新增 `search_and_fetch` 一次调用（每页 3000 字，并发抓取）；`fetch` 默认 8000 字
- [x] **2.2 包骨架**：`pyproject.toml`（核心依赖 + `[extract]` / `[mcp]` 可选）、uv venv（`.python-version` 固定 3.13.2）、`.flake8`；models / errors / settings
  - 39 个测试通过，覆盖率 100%，flake8 通过（2026-11-05）
- [x] **2.3 URL 校验 + 正文提取**：`url_guard`（只允许 http/https，拒绝内网地址）；`extract`（lxml 内置实现，trafilatura 可选）
- [x] **2.4 抓取管线**：`fetch_local` + `fetch_jina` + 按 `WEBSEARCH_FETCH_ORDER` 依次尝试的 `api.fetch`
- [x] **2.5 搜索**：provider 接口 + DDG 子进程实现（硬超时 30s）+ `api.search` + `api.search_and_fetch`（并发抓取）
  - 实测发现：`ddgs` 对同一查询会随机报"无结果"/限流 → 子进程内对此自动重试最多 3 次
  - 12 个查询实测 11 个有结果，平均 3.5s，最慢 8.3s；偶发空结果，留给 M3 与 Exa / Parallel 对比
- [x] **2.6 入口**：CLI（`websearch search-fetch / search / fetch`，统一 JSON 输出）+ MCP server（`web_search_and_fetch` / `web_search` / `web_fetch`）
  - MCP 用 `mcp` 2.x（`MCPServer`，不是旧版 FastMCP）；已用真实 stdio 协议验证握手、列工具、调用
- [x] **2.7 验收**：299 个单元测试、覆盖率 97%、flake8 通过；11 个 live 测试联网通过；README；pi 用 skill（`tools/websearch/skills/websearch`）、MCP agent 用 `websearch-mcp`
- [x] **2.7a 对齐 pi 的 brave-search skill（2026-11-05）**
  - 核查：pi 本身没有内置联网；官方 `badlogic/pi-skills` 的 `brave-search` 需要 `BRAVE_API_KEY`（无 key 直接退出）；本机没装它，会话记录里 89 个会话从未调用过搜索工具。用户看到的"免费搜索"来源未查明，待用户提供截图 / 工具名
  - 正文提取对比（`.tmp/ref/cmp_extract.py`，9 个可抓取页面）：大体持平；HN、阮一峰博客我更好（pi 输出被破坏的表格 / 只拿到 105 字），GitHub / Hugging Face 我带少量导航噪声
  - 修复缺陷：无 `<article>/<main>` 的 Sphinx 类文档站只提到一个小节（docs.python.org 9946 字 → 52960 字）：`role="main"`、`itemprop="articleBody"` 也作为语义候选；新增 2 个测试，301 个测试通过，flake8 通过
  - 已确认只用 ddgs，不加 Brave provider；**freshness / region 参数待做**（用户已选，排在 pre-M3 之后）
- [ ] **2.8 与 pi 原生联网方式的端到端对比**（同一个 35B 模型）：搁置，等用户说明 pi 的免费搜索来源后，用它作基准；脚本见 `.tmp/search_ab/`

## pre-M3 稳定性加固（原 M7，2026-11-05 确认提前到 M3 之前）

依据：2026-11-05 代码核查 + 实测。M3 会把网页正文（每次约 1 万字）塞进 General 的上下文，M6 要 10 用户并发，
所以必须先修，否则 M3 / M6 的结论会被"系统自己放大的负载"污染。

### 已确认的决定（2026-11-05）
1. 提前到 M3 之前，命名 pre-M3。
2. **所有 LLM 调用统一用流式**（SSE）；**Server 侧（推理服务）和 Client 侧（AgentNexus）优化都要做**；**每一步都加埋点**，方便排查。
3. **35B 开槽位；9B 不再部署**（释放内存，需要时通过 router 按请求唤起）。
   ~~5 个槽位：前台 slot 0、后台 slot 1~4~~ → **2026-11-05 改为 6 个槽位：前台 slot 0~1、后台 slot 2~5**，前台和后台互不借用（`borrow_idle = false`，用户要求"尽量不要借用"）。
4. 同第 3 条：前台判断 / 回复 / 汇总都回到 35B 的前台槽位（0~1），不再用 9B。
5. **后端与路由做成配置项**：以后任务 agent 可能用云端模型，本地模型只开 2~4 个槽位专门做 conversation；槽位、并发、限速、角色路由都由配置决定（设计见下文"后端与调度配置化"）。

### 实测事实（用 `scripts/probe_llama_slots.py`、`scripts/probe_llama_cancel.py` 可复测）
- **上下文**：`n_slots=5, n_ctx_slot=262144, kv_unified=true`；每个槽位 `n_ctx` 都显示 262144，实际是 5 个槽位**共享**同一个 262144 的池子。
- **断连取消**（llama.cpp `0.6.0-dev` commit `23b0202`）：流式断开后 0.2~0.3s 槽位空闲；**非流式断开后继续生成**（20s 内 129→607、14→569、107→627 个 token）。→ 必须统一流式。
- **指定槽位**：
  - 请求参数名是 **`id_slot`**，不是 `slot_id`（`slot_id` 被忽略）。`id_slot=0/3/4` 都精确落到对应槽位。
  - **不指定时落到哪个槽位无法保证**（6 次都落到槽位 4）；槽位忙时也可能落到 slot 0。所以"后台传 None"**不能**保护前台槽位，后台请求也必须显式指定后台槽位。
  - **指定的槽位被占时，请求会一直排队**，即使别的槽位空闲（`id_slot=0` 被占时短请求 15s 内无首 token；`id_slot=1` 只要 0.43s）。
    → "前台槽位绝不排队"只在同时的前台请求数不超过前台槽位数时成立；超过就必须由客户端调度器分配空闲槽位或排队（见 pre-M3.3）。前台开 2 个槽位，可同时处理 2 个用户的判断 / 回复。
- **pi 可以固定槽位**：pi 有 `before_provider_request` 扩展钩子，可以改请求体；AgentNexus 启动 pi 时通过环境变量传槽位，扩展把它写进 `id_slot`。
- **SQLite**：单条 `INSERT` `synchronous=FULL` 0.040ms（约 2.5 万条/s）、`NORMAL` 0.016ms、批量 100 条 0.003ms，写入本身不是瓶颈。
- **9B**（已下线，留档）：生成约 23 token/s，低于 35B-A3B（MoE 只激活约 3B）。

### 高风险项核查结论
| 编号 | 结论 | 代码证据 |
|---|---|---|
| H1 | 属实 | `LlmClient` 没有并发限制 / 优先级；General 4 路 + Reviewer 4 路 + pi + Router 同时打 35B，llama.cpp 内部先来先服务 |
| H2 | 属实 | 历史按**条数**（12 条）而非总量；每条原文进 `decide_turn` / `generate_reply`；用户长粘贴原样进提示词；General 工具返回完整追加；服务端 400（超上下文）时 `chat_json` 原样重试 3 次 |
| H3 | 属实且更糟 | `complete()` 非流式，180s 总超时覆盖"排队 + 生成"；任何错误立即重试无退避，最坏 3×180=540s/轮；超时断开后服务端不取消（实测）→ 旧请求还占槽位，负载翻倍；不看 `finish_reason`，被 `max_tokens` 截断当"格式错误"并把截断内容拼回提示词 |
| H4 | 部分成立，降为中 | 写入开销很小；真风险是 `Bus::publish` 同步函数在 async 任务里持 `std::sync::Mutex` 的唯一连接，`list_tasks`（200 行含结果）/ `list_events` 等读取共用这把锁；coding agent 对 pi 每一行输出写一次库；未设 `busy_timeout` / `synchronous` |
| H5 | 属实 | 每个非 delta 事件 `eventTick++` → 展开的任务卡片、诊断页、路由日志页各重取一次（诊断还请求一次模型服务）；每个流式片段让 `ChatView` 整体重渲染且无 `memo`；`/api/health` 每 10s 每个浏览器请求一次模型服务；`web` 没有测试框架 |
| 新发现 | 属实 | `/api/health` 与诊断只检查 `[llm].base_url`，不会逐端点检查 |

### 埋点规范（所有步骤共用，统一 tracing 事件 + 错误码，诊断页可按错误码统计）
- `llm.call`（每次调用一条）：`req_id, role, priority, schema, backend, model, base_url, slot_id, attempt, queue_wait_ms, ttft_ms, total_ms, prompt_tokens, completion_tokens, finish_reason, outcome`
  - `outcome` ∈ `ok | timeout_first_token | timeout_idle | cancelled | ctx_exceeded | http_4xx | http_5xx | unreachable | format | truncated | breaker_open`
- `llm.slot_acquire`（云端后端为并发许可）：`req_id, backend, priority, want_slot, got_slot, wait_ms, stolen(前台借用后台槽位), queue_depth`；`llm.slot_release`：`held_ms`
- `llm.context_trim`：`req_id, role, tokens_before, tokens_after, dropped_messages, truncated_messages`
- `llm.ratelimit`：`backend, reason(rpm/tpm/429), wait_ms, retry_after_ms`
- `llm.breaker`：`state(open/half_open/closed), consecutive_failures`
- `llm.retry`：`req_id, attempt, reason, backoff_ms`
- `store.event_batch`：`size, flush_ms, queue_len`；`store.read_slow`：`op, ms`（> 50ms）
- 前端：`ui.refresh` 计数（按类型、被节流丢弃数），开发模式下显示在诊断页
- 诊断接口新增：每个 base_url 的连通性、`/slots` 快照（忙碌槽位、各槽位已生成数）、`/metrics` 摘要（处理中 / 排队请求数、生成速度）、最近 N 次 `llm.call` 的 P50/P95、按 `outcome` 的计数
- 新增错误码放在 `an_core::codes`；日志里不打印提示词全文和密钥，只打印长度和截断后的前 200 字（失败时）

### 后端与调度配置化（设计，2026-11-05 确认）

目标：同一套代码支持三种部署，只改配置：

| 部署方式 | 前台（conversation / reply / router / 汇总） | 任务（general / reviewer / coding） |
|---|---|---|
| A 全本地（现状） | 本地 35B，slot 0~1（`interactive_slots=[0,1]`） | 本地 35B，slot 2~5（`background_slots=[2,3,4,5]`），前后台互不借用 |
| B 本地前台 + 云端任务 | 本地模型开 2~4 个槽位，**全部**给前台（`interactive_slots` 取全部槽位，没有后台槽位） | 云端模型，用并发上限 + 速率限制，没有槽位概念 |
| C 混合 | 本地 | 部分角色云端、部分本地（如 coding 用本地 pi，general 用云端） |

核心抽象：**后端（backend）= 一个推理服务 + 它的限流方式**；**路由（routing）= 角色 → 后端名**。
调度器只认后端配置，不关心是本地还是云端；`id_slot` 只对 `kind = "llama_cpp"` 的后端发送。

```toml
# ── 后端：可以定义多个，名字自取 ──
[backends.local]
kind = "llama_cpp"                 # llama_cpp：有槽位，支持 /slots、/metrics、id_slot；openai：通用 OpenAI 兼容接口（云端）
base_url = "http://127.0.0.1:8200/v1"
model = "./qwen3.8-35b-a3b/Qwen3.8-35B-A3B-Q4_K_M.gguf"
slots = 6                          # llama_cpp 才有：槽位编号 0..slots-1，必须和 llama-server 的 -np 一致（启动时用 /props 校验）
interactive_slots = [0, 1]         # 前台使用的槽位
background_slots = [2, 3, 4, 5]    # 后台只能用这些槽位；部署 B 里写 []，表示这个后端不接后台角色
borrow_idle = false                # 默认不借用；设为 true 时前台槽位全忙可借用空闲的后台槽位（后台永远不借用前台槽位）
context_tokens = 0                 # 0 = 启动时从 /props 读取；云端写模型的上限
structured_mode = "json_schema"
extra_body = {}                    # 附加到每个请求体的字段，例如 {chat_template_kwargs = {enable_thinking = false}}

[backends.cloud]
kind = "openai"
base_url = "https://api.example.com/v1"
model = "some-cloud-model"
api_key_env = "CLOUD_LLM_API_KEY"  # 密钥只从环境变量读取，配置文件里不写；日志、诊断都不打印
max_inflight = 8                   # 客户端并发上限（没有槽位概念时用它）
reserved_interactive = 1           # 其中预留给前台的许可数（该后端同时接前台和后台角色时才有意义）
rpm = 60                           # 每分钟请求数上限（令牌桶），0 = 不限
tpm = 0                            # 每分钟 token 上限，0 = 不限
context_tokens = 128000

# 每个后端都可以单独设置超时（不写则用默认）
# connect_secs = 5, first_token_secs = 60, idle_secs = 60, total_secs = 600

# ── 路由：角色 → 后端名 ──
[routing]
conversation = "local"
reply = "local"
router = "local"
general = "local"                  # 部署 B 改成 "cloud"
reviewer = "local"
```

规则：
- **向后兼容**：没有 `[backends]` 时，用现有 `[llm]` 合成名为 `default` 的后端，`[models]`（模型名）继续生效，旧 `config.toml` 不用改。`[models.base_urls]` 视为旧写法，继续支持。
- **启动时校验并给出明确的错误**：路由引用了不存在的后端；槽位列表越界或重复；后台角色路由到 `background_slots = []` 的后端；`slots` 与服务端实际槽位数不一致；`api_key_env` 指向的环境变量未设置。启动日志打印"每个角色 → 后端 / 模型 / 限流方式"（不含密钥）。
- **云端后端的调度**：没有 `id_slot`，用 `max_inflight` + `reserved_interactive` + `rpm/tpm` 令牌桶；遇到 429 按 `Retry-After` 退避，并记 `llm.ratelimit` 埋点；前台优先拿许可，后台排队。
- **本地后端的调度**：槽位调度（见 pre-M3.3）。
- **上下文预算按后端分别计算**（`context_tokens`），云端大上下文和本地小上下文可以共存。
- **取消语义**：统一流式后，云端和本地都靠"断开连接"取消；云端是否因此停止计费取决于供应商，埋点里记录取消次数方便对账。
- **pi（coding agent）**：pi 用的模型在 `~/.pi/agent/models.json` 里，不在本配置里。coding agent 配置新增可选项 `backend = "local"`：指向 `llama_cpp` 后端时，AgentNexus 为每个任务申请后台槽位租约、通过 `AN_LLM_SLOT` 传给 pi 的扩展；pi 用云端模型时不申请租约，只靠 `max_concurrent` 限制并发。
- **本地服务端配合**：部署 B 时 `scripts/llama_server_35b.sh` 用 `SLOTS=2~4`、较小的 `CTX`（例如 `CTX=65536~131072`）启动，可以省出大量 KV 内存；`slots` 配置必须同步修改。
- **暂不做**（需要时再加）：云端不可用时自动回退到本地（`fallback`）、按成本自动选后端。

### 能力扩展约束（2026-11-05 用户补充；pre-M3 各步骤设计时要留好口子，不在 pre-M3 里实现）

**部署 B 的边界（已确认）**
- `general` / `reviewer` = 本项目自带的"LLM + harness"执行者，走 AgentNexus 的 `[backends]`（可以是云端 `openai` 后端）。
- `coding` = 外部 coding agent（pi-dev / opencode / zcode / codex 等），通过连接器（现有 `AcpAgent` / `CodingAgent` 适配）接入；它们用哪个云端模型由它们自己的配置决定（如 pi 的 `models.json`），AgentNexus 只做并发限制（租约），不接管模型。

**未来要接入的能力**：会话、OCR、做 PPT / 文档、画图等。pre-M3 要避免把它们堵死：
1. **调度按后端、不按角色**：`BackendScheduler` / 租约 / 熔断 / 统计都以 `Backend` 为单位，角色只是"路由到哪个后端"的一层映射。新增能力（如 `ocr`、`image`）只是新增后端 + 路由项，不改调度器。
2. **角色集合目前是封闭枚举（`LlmRole`）**：能力扩展时改成"能力名（字符串）→ 后端"，`[routing]` 允许任意能力名；`Priority` 由能力卡片声明，不写死在枚举里（到那时再改，现在不动）。
3. **能力卡片（Capability Card，后续设计）**：每个 agent / tool 声明 `id`、`kind`（llm_agent / external_agent / tool）、输入输出类型（文本 / 图片 / 文件）、风险等级、优先级、并发上限、产物类型；`router` 的候选列表、`conversation` 的工具清单都从卡片生成，而不是写在 prompt 里。现有 `AgentCard` 和 `ToolRegistry::infos()` 是雏形，后续统一。
4. **conversation / router 能力扩展**：Turn 决策 schema 里的动作和 `required_tools` 来自卡片；新增能力不改 prompt 模板、不改 schema 结构，只加卡片。M4 改 Turn schema 时一并考虑。
5. 多模态后端（OCR / 画图）的调用形态与 chat completion 不同（图片输入输出、异步任务），所以 `LlmClient` 只管文本对话；其他能力用各自的客户端，但共用 `BackendScheduler`（槽位 / 并发 / 限速）与埋点规范。

→ 在 pre-M3 之后、M3 之前增加一个"能力卡片与路由扩展"的**设计评审**（只出设计文档、等你确认，不写代码）。

### 步骤（每步独立可验收，TDD；先止血、再限流、最后前端）

- [x] **pre-M3.0 基础设施调整**（2026-11-05 完成）
  - 35B：`scripts/llama_server_35b.sh` 启动，`-np 5 -c 262144 --kv-unified`，K/V 缓存 `q4_0` + `-fa on`，`--cache-ram 8192`，`--metrics`；验证 5 个槽位、`n_ctx=262144`、`/metrics` 可用
  - 9B 下线，空闲内存约 28.5GB → 35B 加载后约 24GB；`config.toml` 撤销 conversation 指向 9B
  - 代码里"按角色配置 `base_urls`"保留（以后接别的服务仍可用），目前不使用
  - 探测脚本：`scripts/probe_llama_slots.py`、`scripts/probe_llama_cancel.py`（升级 llama.cpp 后要重跑）
- [x] **pre-M3.1 后端与路由配置化（先做，后面的步骤都读这份配置）**（2026-11-05 完成）
  - 实现：`crates/llm/src/backend.rs`（`BackendConfig` / `validate` / `effective_slots` / `accepts_background`）、`crates/runtime/src/backends.rs`（`Backends::build`、`LlmRole`、`RoleBinding`、`log_bindings`）；`Runtime` 用 `backends.client(role)` 取客户端；`LlmClient` 增加 `backend / role / priority`（调度和埋点要用）
  - 默认值：`slots` 已填、两个槽位列表都不写时，`slots=1` 前后台共用槽位 0，否则前台 `[0]`、后台其余；`reply` 路由不写则跟 `conversation`
  - 限制：写了 `[backends]` 就不能再写 `[models.base_urls]`（报错，说明改用哪种写法）；`/props` 校验槽位数需要异步请求，**挪到 pre-M3.6**
  - 本机 `config.toml` 已迁移为方案 A；`config.example.toml` 含方案 A / B 示例（注释）
  - 测试：`an-llm` 6 个、`an-runtime::backends` 14 个（旧配置兼容、A / B / C、非法组合、环境变量缺失、未知字段）
  - 在 `AppConfig` 增加 `[backends.<名字>]` 与 `[routing]`（schema 见上文"后端与调度配置化"）；`runtime` 装配时按角色查路由、取后端配置，构建 `LlmClient`
  - **兼容旧配置**：没有 `[backends]` 时用 `[llm]` 合成 `default` 后端，`[models]`、`[models.base_urls]` 继续生效；现有 `config.toml` 不改也能跑，行为不变
  - `api_key_env` 只读环境变量，配置文件和日志都不出现密钥
  - 启动校验（错误信息要说明改哪一项）：路由引用不存在的后端、槽位越界 / 重复、后台角色指向没有后台槽位的后端、环境变量缺失；`llama_cpp` 后端启动时用 `/props` 校验 `slots`
  - 埋点：启动时一条 `config.backends` 事件，列出每个角色 → 后端 / 模型 / 限流方式（不含密钥）
  - 测试：解析、旧配置兼容（现有示例与 e2e 配置不变）、三种部署示例（A / B / C）各一份 fixture 能解析、非法组合报错、环境变量缺失、示例配置无未知字段
- [x] **pre-M3.2 H3：流式统一 + 超时 / 重试重构（先做，它放大其它问题）**（2026-11-05 完成）
  - 实现：`crates/llm/src/error.rs`（错误分类 `ErrorKind` / `LlmError`、上下文错误识别、`Retry-After`）、`policy.rs`（`Timeouts`、`backoff_delay`、`Breaker`、`Stats` 环形缓冲）、`lib.rs`（`stream_once` / `call_once` / `run`、`chat` / `chat_stream` / `chat_json` 全部走流式）；`Backend` 带共享的熔断器和统计；新增错误码 `E_LLM_TIMEOUT / CONTEXT / TRUNCATED / STREAM / BREAKER`、`W_LLM_RATE_LIMITED / RETRY`（已写入 DESIGN.md 错误码表）
  - 默认值：连接 5s、首 token 120s、token 间隔 60s、总时长 `[llm].timeout_secs`；前台调用期限 60s（含重试，只约束"等响应开始"，不截断正在生成的长回复），后台不限；均可在 `[backends.*]` 覆盖（`connect_secs / first_token_secs / idle_secs / total_secs / deadline_secs`）
  - 重试：只对连接失败 / 超时 / 429 / 5xx / 流中断重试；前台最多 1 次、后台 `max_retries` 次；指数退避 0.5s→8s ±25% 抖动，遵守 `Retry-After`；已向用户推送过文本的流式调用不再重试；网络重试**不再占用格式重试次数**（最坏耗时从 3×180s 降到前台 ≤ 60s 期限内）
  - 截断：`finish_reason=length` 不当格式错误、不把半截内容拼回；`chat_json` 放大一次 `max_tokens`（×2，上限 16384），仍不行返回 `E_LLM_TRUNCATED`；普通对话保留已生成文本
  - 熔断：连续 3 次后端故障（连接 / 超时 / 5xx / 流中断）熔断 10s，期间快速失败 `E_LLM_BREAKER`；到期放行试探
  - 其他：SSE 按字节切行（修复多字节字符被分包截断）；`reasoning_content` 计入"仍在生成"；请求带 `stream_options.include_usage` 取 token 用量；`extra_body` 合并进请求体（不能覆盖 model / messages / stream）；`StructuredError` 新增 `code`，`general` 失败码不再固定为 `E_LLM_FORMAT`
  - 埋点：`llm.call`（含 `req_id / role / priority / schema / backend / model / attempt / retry / ttft_ms / total_ms / prompt_tokens / completion_tokens / finish_reason / outcome`；放弃的调用记 `cancelled`）、`llm.retry`、`llm.breaker`、`llm.truncated_retry`；`slot_id` / `queue_wait_ms` 在 pre-M3.3 加入
  - 验证：`an-llm` 单元 22 + mock 服务集成测试 20（卡死 / 慢首 token / idle 卡住 / 429 / 400 超上下文 / 5xx / 流中断 / 总时长 / 期限 / 取消后服务端看到断开 / 熔断 / 截断 / 格式反馈）；真实 35B 上 `an-eval --limit 12` 全部通过（P50 约 1.2s）；真实 SSE 形态已核对（`reasoning_content`、末尾 `usage`）
  - 原计划条目（供对照）：
  - `complete()` 与 `chat_json` 内部统一走流式（SSE 累积）：断开即取消；超时拆成**连接 5s、首 token 等待（排队）、token 间隔 60s**，另设总时长上限
  - 读取 `finish_reason`：`length` 不当格式错误，不把截断内容拼回；最多放大一次 `max_tokens`，仍不行报"输出过长"
  - 错误分类：超上下文（400）→ 不盲重试（交给 pre-M3.4 缩减后重试 1 次）；超时 / 429 / 5xx / 连接拒绝 → 指数退避 + 抖动，交互类最多重试 1 次；格式错误 → 保持现有反馈重试
  - 每轮对话设总期限（交互类默认 60s），重试预算跟着总期限走；熔断：连续失败后快速失败 10s
  - 埋点：`llm.call`、`llm.retry`、`llm.breaker`
  - 测试（mock 服务）：卡死 / 慢首 token / 429 / 400 / 截断输出 / 断开后服务端看到取消；断言"超时后不会立即再发请求"、总耗时 ≤ 期限
- [x] **pre-M3.3 H1：后端调度器 `BackendScheduler`（放 `an-llm`，所有调用统一经过它；按后端配置做槽位 / 并发 / 限速）**（2026-11-05 完成）
  - 实现：`crates/llm/src/scheduler.rs`（`Scheduler` / `Lease` / `Who` / `SchedSnapshot`，每个 `Backend` 一个，`call_once` 在发请求前申请、结束或被取消时归还）、`tokens.rs`（粗估 token，限流和 pre-M3.4 共用）、`crates/runtime/src/conv_lock.rs`（会话级串行，先来先得，用完清理；`handle_message` 入口加锁，排队 ≥200ms 记 `conversation.queued`）
  - 行为：前台只用前台槽位、后台只用后台槽位；`borrow_idle` 默认关；同优先级先来先得，后台在有前台排队时让路；等待有期限（`queue_secs`，默认前台 30s / 后台 600s，并受调用期限约束），超时返回 `E_LLM_BUSY`，**不发请求**（避免落到别的槽位上死等）；排队中被取消会退出队伍；`id_slot` 只发给 `llama_cpp` 后端
  - 云端：`max_inflight` + `reserved_interactive`（后台最多占 max−reserved）、`rpm` / `tpm` 令牌桶（tpm 用响应里的真实用量修正）、429 后整个后端按 `Retry-After` 暂停（`llm.ratelimit`）
  - 埋点：`llm.slot_acquire`（等待 ≥100ms 或借用时 info，否则 debug）、`llm.slot_release`、`llm.call` 新增 `slot_id / queue_wait_ms`；`Scheduler::snapshot()` 给诊断页用（pre-M3.6 接入）
  - 验证：`scheduler` 单元 13 个（前台只拿前台槽位、借用开关、后台不拿前台槽位、先来先得 + 取消退队、10 并发前台峰值 = 2、云端预留 / 前台优先 / 不限 / rpm / 429 暂停 / 桶算术）、客户端集成 5 个（`id_slot` 落点、并发后台各用不同槽位且第 4 个排队、忙时不发请求、取消归还、429 暂停）、会话锁 3 个；**真实 35B（`-np 6`）**：1 前台 + 4 后台并发，`/slots` 观察到忙碌槽位 = {0,2,3,4,5}，槽位 1 未被占用，结束后全部空闲（`cargo test -p an-llm --test live_slots -- --ignored --nocapture`）
  - 现象记录：5 路并发生成时每路约 20 token/s（单路更快），后台并发会拉低前台的生成速度——这是算力共享，不是调度问题；前台延迟的最终指标在 pre-M3.10 验收
  - 原计划条目（供对照）：
  - 每个后端一个调度器，策略由后端 `kind` 决定：`llama_cpp` → 槽位调度（请求体带 `id_slot`）；`openai` → `max_inflight` + `reserved_interactive` + `rpm/tpm` 令牌桶，429 按 `Retry-After` 退避，不发 `id_slot`
  - 前台（conversation / reply / router / 汇总）：只用 `interactive_slots`（默认 [0,1]），由客户端选**空闲**槽位（实测指定槽位被占会死等）；全忙则排队，谁先空闲用谁，排队有期限，超时返回明确的"忙"。`borrow_idle` 默认 **false**（用户要求尽量不借用），保留为可选项：设为 true 才会借用空闲的后台槽位
  - 后台（general / reviewer / pi）：只用 `background_slots`，绝不用前台槽位；**不允许不指定槽位**；后端没有后台槽位（部署 B）时，后台角色路由到它会在启动校验时报错
  - 同一会话同一时刻只处理一条消息（会话级锁）
  - 租约（lease）用 RAII：请求结束或被取消时自动归还槽位；调度器记录每个槽位的持有者和持有时长
  - 埋点：`llm.slot_acquire` / `llm.slot_release`；诊断页显示各槽位当前占用
  - 测试：前台只拿前台槽位（默认不借用）、`borrow_idle = true` 时才借用、后台永不拿前台槽位、归还、排队超时、并发 10 个前台请求在 2 个槽位上的分配；云端后端：`max_inflight`、预留许可、`rpm` 令牌桶、429 的 `Retry-After`；三种部署示例各跑一遍
- [ ] **pre-M3.4 H2：上下文预算 `ContextBudget`（放 `an-llm`）**
  - 估算 token（中文约 1 字 1 token，其它约 3.5 字符 1 token，偏保守）；预算按**后端**分别计算：`context_tokens`（`llama_cpp` 默认启动时读 `/props`，云端写模型上限）÷ 预计并发 − `max_tokens` − 余量
  - 裁剪顺序：系统提示 + 最新用户消息保底 → 从最旧历史开始丢 → 单条超长消息保留头尾并注明"已截断 N 字"（数据库里仍存全文）
  - 用户长粘贴给前台只放头尾，完整内容通过任务上下文交给执行者；General 工具返回设单次上限和总预算（M3 网页内容按此分配）
  - 400 超上下文：缩减后重试 1 次，仍失败则明确报错
  - 埋点：`llm.context_trim`
  - 测试：预算计算、裁剪顺序、保底消息不丢、头尾保留、400 缩减重试
- [ ] **pre-M3.5 pi 固定槽位**（做法已确认选 A，见"待用户确认"第 7 项）
  - **为什么要固定**：pi 自己直连 llama.cpp，AgentNexus 管不到它的请求；不指定 `id_slot` 时请求会落到哪个槽位无法保证（实测可能落到前台槽位），前台槽位被 pi 占住，前台对话就会排队。固定槽位 = 把 pi 圈在后台槽位里，前台永远有空位。
  - **做法选项**：
    - A（推荐）pi 扩展：在 `~/.pi/agent/extensions/` 放一个很小的扩展（约 20 行，属于 pi 的用户配置，不进本仓库），在 `before_provider_request` 里读环境变量 `AN_LLM_SLOT` 写进 `id_slot`；AgentNexus 启动 pi 时按租约设置该变量。只影响 pi，没有该变量时 pi 行为不变；缺点：每个外部 coding agent 要各写一份适配（pi / opencode / codex 的扩展机制不同）。
    - B 不固定，只限制 pi 并发 ≤ 后台槽位数：零适配，但 pi 的请求仍可能落到前台槽位，只能降低概率，不能保证。
    - C 本机反向代理：AgentNexus 起一个本机代理，coding agent 的 `base_url` 指向代理，代理按租约注入 `id_slot`、统一做并发限制和埋点；对 pi / opencode / zcode / codex 一视同仁，符合"多种 coding agent 连接器"的方向；缺点：要改各 agent 自己的 provider 配置指向代理，代码量最大。
  - 已选 A，按下面的步骤做。
  - 写一个 pi 扩展（`before_provider_request`）：读环境变量 `AN_LLM_SLOT`，写入请求体 `id_slot`；没有该变量时不改
  - coding agent 新增可选项 `backend`：指向 `llama_cpp` 后端时，启动 pi 前向调度器申请后台槽位租约，把槽位号放进环境变量，任务结束归还；pi 用云端模型时不申请租约；`max_concurrent` 限制 ≤ 2
  - 验证：用 `/slots` 确认 pi 的请求确实落在被分配的槽位，且从不落在前台槽位
- [ ] **pre-M3.6 健康检查**：`list_models` 结果缓存 5~10s，多个浏览器共用；health / 诊断逐个检查实际使用的 base_url，并附 `/slots`、`/metrics` 摘要（见埋点规范）
- [ ] **pre-M3.7 H5 前端刷新风暴**
  - 事件刷新统一走一个节流 hook（尾部节流 ~1s）；只刷新当前可见的页签；进度类事件不再触发 `api.tasks`
  - 任务详情用事件本地追加，加载一次后用 `?after=<event_id>` 增量拉取
  - 诊断页 / 路由日志页：打开页签时取一次 + 仅在 `status_changed` / `assistant_message` 时节流刷新 + 手动刷新
  - 流式渲染：delta 用 `requestAnimationFrame` 合批；历史消息 `React.memo`；流式气泡按纯文本或 ≥250ms 才重新解析 Markdown，结束后完整渲染
  - 引入 vitest + @testing-library/react：断言"连发 50 个进度事件，接口请求次数 ≤ 节流上限"、"流式 100 个片段，Markdown 解析次数 ≤ 上限"
- [ ] **pre-M3.8 H4 存储（降级为中，先量后改）**
  - 先做并发基准：8 个任务 × 1000 事件同时写 + 并发读 `list_tasks` / `list_events`，记录读延迟 P95
  - 低成本修改：`synchronous=NORMAL`、`busy_timeout`；事件写入改成"有界通道 + 专用写线程 + 批量提交"（SSE 广播不等落库）；读用独立只读连接（WAL 支持并发读）
  - coding agent 逐行进度事件节流合并（≥200ms 或 N 行一条）；`events` 表加保留策略
  - 埋点：`store.event_batch`、`store.read_slow`
  - 验收：读延迟 P95 不随写入量线性增长；事件顺序不乱；退出时写完缓冲
- [ ] **pre-M3.9 Server 侧调优（逐项基准，一次只改一个，用探测脚本和评测集对比）**
  - 已做：Q4 KV + Flash Attention、`--cache-ram`、`--metrics`、5 槽位共享池
  - 待基准：`-b / -ub` 批大小、`--cache-reuse`、前台结构化调用关闭思考（请求里加 `chat_template_kwargs: {"enable_thinking": false}`，预期显著降低判断延迟，需用评测集确认准确率不降）、`--reasoning-budget`
  - 每项记录：首 token 延迟、生成速度、内存、评测通过率；结论写回本节
- [ ] **pre-M3.10 验收**：并发 10 会话（其中 3 个后台任务在跑）压测：
  - 前台判断延迟 P95 ≤ 30s；日志中无"超时后立即重复请求"；`/slots` 抽查无僵尸生成；前台槽位（0~1）上没有后台请求；前端请求数和渲染次数在上限内
  - 所有 `llm.call` 的 `outcome` 分布可从诊断页看到，失败都带错误码

建议顺序：pre-M3.1（配置化）→ .2（流式 + 超时重试）→ .3（调度）→ .4（上下文预算）→ .5（pi 槽位）→ .6（健康检查）→ .7（前端，可与后端并行）→ .8（存储）→ .9（Server 调优）→ .10（验收）。
pre-M3.3 之后，M5 里"9B vs 35B"的结论只作留档，不再影响部署。

---

## M3 接入 General agent + 搜索方案 A/B

- [ ] **3.1 接入**：在 `crates/agents/src/tools.rs` 按现有 `Tool` 接口实现 `web_search` / `web_fetch`，内部调用 M2 的 CLI `websearch search-fetch`（JSON 输出），不在 Rust 里重复实现搜索逻辑
  - 配置项：CLI 路径、超时；未安装时工具不注册，前台能力清单自动不显示联网
  - 测试：CLI 缺失、超时、正常 JSON 解析
- [ ] **3.2 三个方案作为 provider 接入**（同一接口，配置切换）
  - A：DuckDuckGo + 本地抓取 + Jina 兜底
  - B：Exa 托管 MCP（`web_search_exa`，无 key）
  - C：Parallel 托管 MCP（`web_search`）
- [ ] **3.3 A/B 评测**：固定话题集（见 M6.1），每个方案跑同一批查询
  - 指标：成功率、结果数、正文字数、中文站点可用率、延迟 P50/P95、答案质量评分（见 M6.2）
  - 预调研数据：DDG+Jina 中文站抓取 5/12 成功；Exa 4/4，约 2 万字，0.4~1.9s；Parallel 4/4，约 2.6 万字，约 1s
- [ ] **3.4 结论**：选默认方案，其余保留为备选 provider

## M4 多问题消息逐条对应回答

- [ ] **4.1 评测样本**：新增 F 组样本（一条消息含 2~4 个问题；混合"直接回答 + 需要委派"的情况）
- [ ] **4.2 直接回答**：回答提示词要求识别多个问题，按编号逐条回答，不遗漏
  - 测试：提示词包含规则；e2e 检查回答编号数 = 问题数
- [ ] **4.3 混合情况**：Turn 判断输出拆分后的子项（每项 respond / delegate），Composer 汇总时按原问题编号对应
  - 这一步改动 Turn 输出 schema，开始前单独确认设计
- [ ] **4.4 验收**：F 组样本逐条覆盖率 ≥ 95%

## M5 9B vs 35B A/B

- [x] **5.1 代码改动**：`[models]` 支持按角色配置 `base_url`（现在只能改模型名），使回答阶段可单独指向 8201
  - 已完成（2026-11-05）：`LlmClient::with_base_url`、`ModelOverrides.base_urls`、`reply_target()`；配置解析 / 未知角色提示 / reply 回退规则共 5 个新测试，全工作区测试通过
  - 2026-11-05 晚些时候决定 9B 下线，`config.toml` 已撤销指向 9B 的配置；`base_urls` 功能保留备用
  - 遗留：`/api/health` 与诊断只检查 `[llm].base_url` → 见 pre-M3.6
- [ ] **5.2 路由判断 A/B**：用同一版 `an-eval`、同一份 95 条评测集，分别跑 9B 和 35B
  - 旧的 9B 结果（动作 72.2% / 角色 52.7% / 类别 32.0%）来自旧版程序，不能直接比较
  - **初步结果（2026-11-05，未定稿）**：9B 前台判断（95 条）整体 82.1%、动作 91.6%、角色 96.4%、类别 84.0%、风险 87.9%；
    35B 基线（90 条，不含新增 5 条边界样本）整体 88.9%、动作 95.6%、角色 96.1%、类别 95.8%、风险 92.9%
  - 不能直接下结论：两次条数不同（95 vs 90）；9B 这次跑的同时我在 8200 上做断连测试，路由延迟被污染；**需要 35B 在 95 条上重跑一次同口径基线**
  - 9B 的主要错误：over_delegate 4 / under_delegate 4 / wrong_class 4；弱项标签 snippet 0%、risk 20%、calc 50%、review-repo/review-short 50%
  - 9B 生成速度约 23 token/s，低于 35B-A3B（MoE 只激活约 3B），所以 9B 不一定更快；判断延迟 P95 9B 11.4s vs 35B 27.0s（受并发干扰，仅供参考）
- [ ] **5.3 最终回答 A/B**：同一批问题（含 M4 多问题样本、M6 话题），回答分别由 9B / 35B 生成
  - 指标：首字延迟、总延迟、逐条覆盖率、质量评分（35B 作评委 + 人工抽查）
- [ ] **5.4 结论**：判断 / 回答各用哪个模型；如果 9B 回答够好，可把 35B 槽位留给 coding 和 General

## M6 复杂话题 e2e 综合评估 + 10 用户压测

- [ ] **6.1 话题集**：5~8 个复杂话题（需要多来源、有时效性、中英文混合），例如本地小模型选型对比、某行业政策解读
- [ ] **6.2 评分标准**（1~5 分）：来源相关性、信息准确性、覆盖完整度、条理结构、引用可追溯
  - 35B 作评委打分 + 人工抽查 20%
- [ ] **6.3 端到端评估**：前端发消息 → 派发 → 搜索 → 抓取 → 整理 → 审查 → 交付，记录每段耗时
- [ ] **6.4 10 用户压测**：10 个并发会话，混合闲聊 / 搜索 / 文档 / 代码任务，持续 10 分钟
  - 指标：成功率、判断延迟 P50/P95、任务完成时间、错误率、8200 槽位排队情况
  - 初定门槛（需确认）：成功率 ≥ 95%，判断延迟 P95 ≤ 30s，无崩溃
  - 不达标的预案：减少 LLM 调用次数（9B 已下线，如要用需先按需唤起并复测）、搜索换方案、调整槽位与上下文
- [ ] **6.5 报告**：结果写入 `docs/`，给出最终方案建议

---

## 待用户确认

1. ~~M2 的第三方依赖~~（已确认：核心 ddgs + httpx；trafilatura、mcp 为可选安装）
2. M6 的压测门槛和话题集
3. 用 35B 作评委打分是否可接受（会占用 8200 槽位）
4. M4.3 改 Turn 输出 schema 的设计（开始前单独确认）
5. ~~M7 是否排在 M3 之前 / 统一流式 / 槽位数~~（已确认：提前并改名 pre-M3；统一流式；35B 槽位 5→6 见第 6 项；9B 下线）
6. ~~前台借用后台槽位~~（已确认 2026-11-05：**不借用**，改为 6 槽位 = 前台 2 + 后台 4；`borrow_idle` 默认 false）
7. ~~pre-M3.5 怎样固定 pi 的槽位~~（已确认 2026-11-05：选 A，pi 扩展 + 环境变量 `AN_LLM_SLOT`；其他 coding agent 以后各自适配，通用代理方案 C 留到连接器设计评审时再议）
8. ~~pre-M3.9 关闭思考对比实验~~（已确认 2026-11-05：前台结构化调用 `enable_thinking=false`，**必须用评测集确认准确率不降**才采用）
9. ~~pre-M3.1 配置设计~~（已确认 2026-11-05：`[backends.<名>]` + `[routing]`、`kind = llama_cpp | openai`、密钥只读环境变量、无 `[backends]` 沿用 `[llm]`；`router` 归前台）
10. ~~部署 B 的云端边界~~（已确认 2026-11-05，并补充了能力扩展要求，见 pre-M3"能力扩展约束"）

## 已知问题

- llama.cpp `0.6.0-dev`：非流式请求断开后不取消生成（见 pre-M3 实测），升级后用 `scripts/probe_llama_cancel.py` 复测

- pi 的 Slack 扩展 `@arvoretech/pi-slack-bridge` 偶发崩溃（exit 1），按要求暂不关闭
- Codex / opencode 的 GLM 额度在 2026-11-06 17:15 重置前会返回 429
- `~/.config/opencode/opencode.json` 有明文 Slack token 和 DeepSeek key，建议改成环境变量
