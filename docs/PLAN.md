# AgentNexus MVP 开发计划与 TodoList

> 最后更新：2026-11-05
> 规则：按里程碑逐步推进；每完成一步更新本文件（勾选进度、做必要的最小调整），用户确认后再进入下一步。
> 新功能先写测试（TDD），测试冻结后只改业务代码；Python 部分覆盖率 ≥ 80%，通过 flake8。

## 0. 当前环境

| 项目 | 配置 |
|---|---|
| 判断 / 路由 / 回答 / General / 审查 | llama.cpp `127.0.0.1:8200`，Qwen3.8-35B-A3B Q4_K_M，4 槽位共享 64K 上下文 |
| 备选小模型 | llama.cpp `127.0.0.1:8201`，Qwen3.8-9B Q5_K_M，2 槽位 |
| coding agent | pi（`~/.npm-global/bin/pi`），只用 8200 上的 35B |
| 后端 / 前端 | `agentnexus` 监听 8787；前端 `web/`（Vite + React） |
| 构建 | 所有 cargo 命令加 `CARGO_TARGET_DIR=target` |

## 1. 里程碑总览

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 | 本地部署 + e2e 发现的 5 个问题 + 派发边界规则 | ✅ 完成 |
| M1 | 派发"越用越聪明"：纠错反馈、相似经验检索、纠错回归 | 待开始 |
| M2 | 独立的网页搜索工具包（Python，MCP / CLI / 函数） | 待开始 |
| M3 | 搜索接入 General agent + 多方案 A/B | 待开始 |
| M4 | 多问题消息逐条对应回答 | 待开始 |
| M5 | 9B vs 35B A/B（路由判断 + 最终回答） | 待开始 |
| M6 | 复杂话题 e2e 综合评估 + 10 用户压测 | 待开始 |

建议顺序：M2 → M3 → M4 → M5 → M6，M1 可与 M2 并行（互不依赖）。
M6 依赖 M3、M4 完成；M5 的回答部分依赖"按角色配置模型地址"的代码改动。

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
- 依赖（需确认）：`ddgs`（DuckDuckGo 搜索，无需 key）、`httpx`（抓取）、`markdownify` 或 `trafilatura`（HTML 转 Markdown）、`mcp`（MCP server SDK）
- 测试放 `tools/websearch/tests/test_*.py`，网络调用全部可 mock

- [ ] **2.1 需求与接口文档**：`search(query, max_results) -> list[Result]`、`fetch(url, max_chars) -> Page`；统一错误结构
- [ ] **2.2 搜索 provider 接口**（可插拔）+ DuckDuckGo 实现
  - ddgs 在子进程里跑，硬超时 30s（参考 Hermes：primp 可能持有 GIL 卡死）
- [ ] **2.3 抓取**：本地抓取 → HTML 转 Markdown；失败（超时 / 4xx / 5xx / 内容过短）再走 Jina `r.jina.ai` 兜底
  - URL 校验：只允许 http/https，拒绝内网地址（防 SSRF）
- [ ] **2.4 CLI**：`websearch search "..." --json`、`websearch fetch URL --json`
- [ ] **2.5 MCP server**：暴露 `web_search`、`web_fetch` 两个工具（stdio）
- [ ] **2.6 质量门槛**：pytest 覆盖率 ≥ 80%，flake8 通过
- [ ] **2.7 验收**：pi / opencode 通过 MCP 能调用；CLI 输出 JSON 正确

## M3 接入 General agent + 搜索方案 A/B

- [ ] **3.1 接入**：在 `crates/agents/src/tools.rs` 按现有 `Tool` 接口实现 `web_search` / `web_fetch`，内部调用 2.4 的 CLI（JSON 输出），不在 Rust 里重复实现搜索逻辑
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

- [ ] **5.1 代码改动**：`[models]` 支持按角色配置 `base_url`（现在只能改模型名），使回答阶段可单独指向 8201
  - 测试：配置解析、未配置时沿用全局
- [ ] **5.2 路由判断 A/B**：用同一版 `an-eval`、同一份 95 条评测集，分别跑 9B 和 35B
  - 旧的 9B 结果（动作 72.2% / 角色 52.7% / 类别 32.0%）来自旧版程序，不能直接比较
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
  - 不达标的预案：判断改用 9B、减少 LLM 调用次数、搜索换方案、调整槽位与上下文
- [ ] **6.5 报告**：结果写入 `docs/`，给出最终方案建议

---

## 待用户确认

1. M2 的第三方依赖（`ddgs`、`httpx`、HTML 转 Markdown 库、`mcp`）
2. M6 的压测门槛和话题集
3. 用 35B 作评委打分是否可接受（会占用 8200 槽位）
4. M4.3 改 Turn 输出 schema 的设计（开始前单独确认）

## 已知问题

- pi 的 Slack 扩展 `@arvoretech/pi-slack-bridge` 偶发崩溃（exit 1），按要求暂不关闭
- Codex / opencode 的 GLM 额度在 2026-11-06 17:15 重置前会返回 429
- `~/.config/opencode/opencode.json` 有明文 Slack token 和 DeepSeek key，建议改成环境变量
