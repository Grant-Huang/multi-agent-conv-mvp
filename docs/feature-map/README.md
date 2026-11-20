# AgentNexus Feature Map（功能地图）

> 给 AI Agent（和开发者）读的“产品地图”：软件有哪些功能、用户怎么走到、Agent 怎么驱动、怎么判断正常、哪里容易踩坑。
> 它不是用户手册（用户手册见 [docs/manual](../manual/README.md)），也不是需求规格；它回答的是：**“针对这个具体功能，我该去哪、怎么操作、怎么确认成功？”**
> 接到“任务执行完了但看不到结果”这类模糊反馈时，先在下面的索引里定位功能，再进入对应文件。

## 与现有测试文档的关系

| 文档 | 角色 |
|---|---|
| 本目录 | 面向 Agent 的**操作地图**：入口、驱动方法、判据、坑 |
| [COVERAGE.md](COVERAGE.md)（原 `docs/testing/FEATURE_MAP.md`） | 各功能的用例 ID、证据与边界，**权威的覆盖/证据状态在这里** |
| [docs/testing/features.json](../testing/features.json) | 机器索引，`npm run test:map` 校验源文件与用例引用 |
| [docs/testing/CASES.md](../testing/CASES.md) | 用例定义（E2E-* / RT-*） |

本目录不重复记录证据状态。每个功能文件的“Backed by”只列出源码和用例 ID，证据细节看 COVERAGE.md。**本目录的操作步骤未逐条重新执行**，标注“已有自动化覆盖”的是由对应 E2E 用例证明过的路径，其余是依据源码与配置整理的“草稿”，使用时以实际运行为准。

## 功能索引

“覆盖”列：**浏览器** = 已有 Playwright 用例走过真实页面；**HTTP/运行时** = 只有后端测试；**无** = 仅设计或需真实外部依赖。

| 功能文件 | 包含的功能（与 features.json 的 id 对应） | 用户入口 | 覆盖 |
|---|---|---|---|
| [conversation.md](features/conversation.md) | conversation, navigation | 左栏“＋ 新会话”；输入框“输入消息” | 浏览器 |
| [tasks.md](features/tasks.md) | tasks, assignment, cancellation | 对话里的任务状态/走马灯；右栏“任务”页签 | 浏览器 |
| [approval.md](features/approval.md) | approval | 对话/任务卡“批准执行” | 浏览器 |
| [agents.md](features/agents.md) | agent-links, acp-discovery, agent-catalog, agent-config, agent-ui, agent-chat, agent-diagnose | 左下角连接状态；右栏“助手”页签 | 浏览器 + HTTP |
| [nodes.md](features/nodes.md) | nodes | 右栏“节点”页签 | HTTP（真实 LAN 未覆盖） |
| [observability.md](features/observability.md) | observability, feedback, http-host | 右栏底部“开发者模式”→“诊断”“路由日志”；任务卡“派错了” | 浏览器 + HTTP |
| [research-and-tools.md](features/research-and-tools.md) | reports, tools-evals, MCP, templates | 对话中提需求；`config.toml` 的 `[[mcp.servers]]` | 运行时 |
| [egress-dlp.md](features/egress-dlp.md) | dlp-egress, egress | `config.toml` 的 `[egress]` | 运行时（替身） |
| [routing-and-admission.md](features/routing-and-admission.md) | front-thinking, admission | 配置与 HTTP `/api/admission/*`，无界面 | 运行时 |
| [git-evidence.md](features/git-evidence.md) | git-evidence | 无（执行后的任务证据） | 后端 |
| [mom.md](features/mom.md) | mom, mom-des, des-factory, mom-workbench-prototype | 左栏“MOM 运营沙盘”→`/mom/` | 浏览器（模拟） |
| [world-adapter.md](features/world-adapter.md) | world-adapter（DES 实验/运营 E2E-DES*） | 「DES 仿真实验」+「运营频道」；看板/任务/资源 | 浏览器用例已冻结（实现前 Red）；协议/Mock 待 M0-1+ |

## 配套文档

| 文件 | 用途 |
|---|---|
| [SCENARIOS.md](SCENARIOS.md) | 跨功能的端到端场景，标明哪些已有自动化、哪些缺口，指导下一批 E2E |
| [SYSTEM_GAPS.md](SYSTEM_GAPS.md) | 系统完整性分析：模块间断开的地方、整体场景缺口、Feature Map 审查结论 |
| [COVERAGE.md](COVERAGE.md) | 各功能的用例 ID、证据与边界 |

## 当前不是已实现功能

不要为这些去找入口：M4.3 模板并行执行与 Composer 汇合回复（只有校验部分）、生产级 MOM 连接器、知识库、Benny 式自动 Bug 复现与修复、多用户登录与鉴权、通用动态 DAG、执行中断续跑、跨互联网节点与节点 TLS。

## 驱动工具速查（Harness）

### 1. 受控浏览器环境（首选）

```bash
cd web
npm ci
npm run test:map     # 功能地图一致性
npm run verify       # 地图校验 → TS/Vite 构建 → cargo build agentnexus → Playwright E2E
npm run test:e2e     # 只跑已构建后的浏览器用例
```

- 实际执行的是 `web/e2e/harness.mjs`：启动真实 `agentnexus`，监听 **127.0.0.1:18787**，**只把模型换成受控替身**；HTTP / SSE / SQLite / 文件工具都是真实的。端口被占先定位，不要终止不属于本次的进程。
- 数据在一次性临时目录，状态与日志在 `web/test-results/runtime/`（`state.json`、`backend.log`、`model.jsonl`），报告在 `web/playwright-report/`、`web/test-results/results.json`。
- 受控模型的**触发词**（写新用例时要用）：消息以“你好”开头 → 直接回答；其他 → 委派；目标含“审批” → high 风险；含“改代码验证” → 路由到 coding；含“失败验证任务” → 模型服务返回 500；含“慢验证任务” → 120 秒不返回（测取消）；含“接进来验证”/“接个写代码的助手验证” → 触发助手管理。
- MOM 用例（`web/e2e/mom.spec.ts`）单独启动 `an-mom`，端口 **18790**。
- 通过**不代表**真实模型质量达标，也不代表真实 ACP agent、真实搜索、真实局域网节点可用。

### 2. 稳定的元素定位

仓库里没有 `data-testid`（仅测试替身里有）。使用语义定位并**限定作用域**，重复的按钮不要全局盲点：

| 目标 | 定位 |
|---|---|
| 输入框 | `getByRole('textbox', { name: '输入消息' })` |
| 发送 | `getByRole('button', { name: '发送', exact: true })` |
| 新会话 | `getByRole('button', { name: '＋ 新会话', exact: true })` |
| 会话列表 | `getByRole('complementary', { name: '会话列表' })` |
| 右栏页签 | `getByRole('tab', { name: '任务' \| '助手' \| '节点' \| '诊断' \| '路由日志' })` |
| 对话区操作 | `page.locator('main').getByRole('button', { name: '批准执行' \| '取消任务' \| '确认取消' \| '下载' })` |
| 开发者模式 | `getByRole('button', { name: '开发者模式：关' })` |
| 助手列表 | `getByRole('region', { name: '助手列表' })`；卡片 `region` 名为“确认修改”“修改结果”“发现的助手”“安装步骤” |
| 窄屏抽屉 | `getByRole('button', { name: '☰ 会话' })`（视口窄时） |

开发者模式开关与“已看过发现卡”、当前会话 ID 存在浏览器 `localStorage`（键 `agentnexus.devMode`、`an.agentsIntroSeen`、`an.currentConversation`），测试请使用全新 context。

### 3. 直接调 HTTP（不经过页面）

基址默认 `http://127.0.0.1:8787`（E2E 为 18787）。Host 头必须是 localhost/127.0.0.1/[::1] 且端口匹配。

```bash
curl -s localhost:8787/api/health | jq .                                  # LLM 与各 agent 的 link.status
curl -s -XPOST localhost:8787/api/conversations                            # 新建会话 → {id,...}
curl -s -XPOST localhost:8787/api/conversations/$CID/messages \
     -H 'content-type: application/json' -d '{"text":"你好"}'             # 发送消息
curl -s "localhost:8787/api/tasks?conversation_id=$CID" | jq .             # 任务列表
curl -s localhost:8787/api/tasks/$TID | jq '.task.status, .events'         # 详情 + 事件
curl -s -XPOST localhost:8787/api/tasks/$TID/approve                       # 批准
curl -s -XPOST localhost:8787/api/tasks/$TID/cancel                        # 取消
curl -sN "localhost:8787/api/events?conversation_id=$CID"                  # SSE 事件流
```

完整路由以 `crates/server/src/lib.rs` 为准；状态与中文文案以 `web/src/labels.ts` 为准。

### 4. 后端用例

```bash
CARGO_TARGET_DIR=target cargo test --locked                 # 全部（受控 LLM 替身，无需真模型）
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test <名称>
```

## 排查路径（从“看不到结果”到根因）

按层逐步缩小，每层都有可观测证据：

1. **页面层**：左下角连接状态 → 对话里的任务状态行 → 右栏“任务”卡片时间线。
2. **API 层**：`GET /api/tasks/{id}` 看 `status`、`events`、产物；与页面是否一致（不一致 → 前端同步问题，查 `/api/events` SSE）。
3. **路由层**：开发者模式 → “路由日志”，看判断、角色、风险与模型原始输出。
4. **执行层**：“诊断”页的后端/执行组；`logs/agentnexus.jsonl.*` 里按 `task_id` 过滤 `event`/`code`。
5. **持久化层**：任务“完成”不等于产物已落盘。核对产物下载内容、`workspace/files`（或代码目录）实际文件、数据库任务记录三者一致。

## 维护

- 改功能：先更新 `docs/testing/CASES.md` 与可执行 E2E，再改实现，同步 `features.json`，最后更新这里对应的功能文件。
- 页面文案、入口或定位方式改了，必须同步“How to get to it”和“Driving it”。
- 草稿 → 已验证：用本目录的步骤实际驱动一次并保留证据后，在该功能文件的“验证状态”里写明日期与证据路径；不要在没运行过的情况下写“已验证”。
