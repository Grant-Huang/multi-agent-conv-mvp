# AgentNexus MVP

Conversation Agent + Router + 3 个执行 Agent 的最小可运行骨架。后端 Rust（axum + tokio + SQLite），前端 TypeScript（React + Vite），模型走 OpenAI 兼容接口（默认对接本地 llama.cpp）。

```
用户 ──▶ Conversation（Turn Controller：respond / clarify / delegate / follow_up）
              │ delegate
              ▼
           Router（LLM 分类）──▶ Policy（确定性规则：角色可用？风险审批？是否审查？）
              │
              ▼
         Orchestrator ──▶ General  （内置：LLM + 工具循环，读写文件、计算）
              │       ──▶ Coding   （外部：Codex CLI 子进程，收集 git diff）
              │       ──▶ Reviewer （内置：只读审查，pass / fail / needs_human）
              ▼
   审查不通过 → 带问题清单返工 → Composer 生成交付消息 → SSE 推送到前端
```

完整设计（架构、数据模型、状态机、时序、接口、配置、已知限制、演进路线）见 [docs/DESIGN.md](docs/DESIGN.md)。

设计原则（来自前期讨论）：
- **Conversation 和 Router 分开**：Conversation 决定"要不要委派"，Router 决定"派给谁"。
- **Agents as Tools**：用户只面对 Conversation，Worker 只返回结构化结果。
- **模型负责理解，程序负责决策**：Router 的风险判断只是建议，审批和审查由 Policy 规则决定。
- **Worker 完成 ≠ 验收通过**：需要审查的任务（代码、文档）必须经过 Reviewer。
- **任务状态由 AgentNexus 掌握**：全部落 SQLite，事件可回放。

## 快速开始

前置：Rust 1.80+、Node 20+、已启动的 llama.cpp server（默认 `http://127.0.0.1:8098/v1`）、可选的 Codex CLI。

```bash
cp config.example.toml config.toml   # 按需修改模型地址、codex 路径、工作目录
```

```bash
cd web && npm install && npm run build && cd ..
```

```bash
cargo run --release --bin agentnexus
```

打开 http://127.0.0.1:8787 。左下角会显示 LLM 和各 Agent 的连接状态。

前端开发模式（热更新）：后端照常运行，另开终端 `cd web && npm run dev`，访问 http://127.0.0.1:5173 。

## 测试小模型的路由效果

```bash
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --review-mode reviewer --label 模型简称
```

只跑"前台判断 + Router 分类"（不执行任务），输出整体通过率、动作 / 角色 / 类别 / 工具 / 风险 / 跟进准确率、动作混淆矩阵、误判分类（多派、漏派、多问……）、按标签的通过率和延迟。逐条结果（含模型原始输出）和汇总 JSON 写入 `evals/results/`，换模型后重跑即可对比。

- `evals/routing.jsonl`：评测集 v2，90 条，由 `evals/make_dataset.py` 生成（补样本请改脚本）
- `evals/routing-v1.jsonl`：第一次评测的 26 条，用于和旧基线对比
- `evals/EVAL_PROMPT.md`：**完整的评测步骤和报告模板，可以直接交给本地 Claude 执行**

选项：`--review-mode reviewer|direct|auto`、`--tag 标签`（只跑某类）、`--limit N`、`--label 名称`。

界面右侧的「路由日志」也能看到每轮的决策和模型原始输出。

## 小模型调优建议

| 现象 | 处理 |
|---|---|
| 格式经常出错、重试多 | 确认 `structured_mode = "json_schema"` 生效（llama.cpp 日志里能看到 grammar）；不支持时试 `json_object`；仍不行再用 `prompt_only` |
| llama.cpp 返回 400 | 多半是不支持该 `response_format` 写法，换另一种 `structured_mode` |
| 响应很慢 | 判断和回答已经分两步、回答流式显示；还可以在 `[models]` 里给 `conversation`（判断）和 `router` 配更小更快的模型，`reply`（回答）用质量更好的模型。推理类模型（带 `<think>`）会先思考很久，可在 llama.cpp 侧关闭思考 |
| 简单问题也被委派 | 调整 `crates/runtime/src/conversation.rs` 中的 system prompt，或换更大的模型做 conversation |
| 工具循环不收敛 | 调小 `agents.general.max_steps`，或给 General 配更大的模型 |

## 接入新 agent（ACP）

AgentNexus 通过 [Agent Client Protocol](https://agentclientprotocol.com)（ACP）接入外部 agent：一个通用适配器，所有支持 ACP 的 agent 都能用。

**第 0 步：什么都不用做。** 本机装了下面这些 agent，启动时会被自动发现，左下角会显示出来：

| agent | 角色 | 启动方式 | 说明 |
|---|---|---|---|
| [OpenCode](https://opencode.ai/docs/acp) | coding | `opencode acp` | 原生支持 |
| [Hermes Agent](https://hermes-agent.nousresearch.com/docs/user-guide/features/acp) | general | `hermes acp` | 原生支持，需安装 ACP 扩展 |
| pi | coding | `pi-acp` | 需另装第三方适配器 pi-acp，成熟度较低 |

**第 1 步：指定由谁来做，加一行。** 不指定时，每类工作用第一个可用的 agent（内置和 Codex 排在前面）：

```toml
[agents.assign]
coding = "opencode"
```

**第 2 步：接入其他 ACP agent，写三行。**

```toml
[[acp.agents]]
id = "my-agent"
command = "my-agent"
args = ["acp"]
role = "coding"
```

agent 执行出错时会如实报错（带错误码），不会自动换别的 agent 重做。

**先体验一下？** 自带一个模拟 ACP agent，不需要安装任何东西：

```toml
[[acp.agents]]
id = "mock"
command = "./target/release/an-mock-acp"
role = "coding"

[agents.assign]
coding = "mock"
```

然后在界面里说"帮我改代码"，任务面板里能看到工具调用、权限请求、git diff 等完整过程。

注意：
- 这些 agent 用的是**它们自己的模型配置**。想让它们也用本地 llama.cpp，需要在各自的配置里设置。
- agent 请求权限时默认自动允许（任务已通过 Policy 和人工审批），每次请求都记录在证据里；可以用 `permission = "read_only"` 收紧。

## 用其他电脑上的 agent（节点）

agent 装在另一台电脑上（macOS / Linux / Windows 都可以）时：

1. 在那台电脑上运行节点程序（从本项目编译：`cargo build --release --bin an-node`，然后把 `target/release/an-node` 拷过去）：
   ```bash
   an-node
   ```
   它会自动发现那台电脑上的 agent，并显示地址和 **6 位配对码**。
2. 在 AgentNexus 界面右侧打开「节点」页：同一局域网的节点会自动出现，输入配对码点「配对」。
   网络不允许自动发现时，用「按地址添加」填 IP 和配对码。
3. 完成。那台电脑上的 agent 以 `agent名@节点名` 的形式出现，例如 `hermes@office-pc`。

几台电脑上有同一种 agent 时，可以组成执行组分担工作：

```toml
[agents.assign]
general = ["hermes@office-pc", "hermes@mac-mini"]   # 第一个是主力，其余待命
```

规则：只能放**同一类型**的 agent；主力忙满时交给空闲的备份，都忙就排队；离线的跳过；**执行出错直接报错，不换人重做**。

节点的配置（全部可选）：`an-node node.toml`，或用参数 `--port`、`--name`、`--workdir`、`--bind`、`--no-advertise`。详见 [设计文档第 8 节](docs/DESIGN.md#8-节点用其他电脑上的-agent)。

⚠️ 节点与 AgentNexus 之间目前是明文 HTTP，只在可信的局域网使用；跨网络请用 Tailscale 等私有网络。

## 日志与排查

- 界面右侧「诊断」页：模型连通性、执行组状态、最近的问题和错误码。点「复制诊断信息」可直接贴给 AI 或开发者。
- JSON 日志：`logs/agentnexus.jsonl.<日期>`（节点在 `~/.agentnexus-node/logs/`）。每条记录有固定的 `event`（事件名）和 `code`（错误码），并关联 `task_id` 等。AgentNexus 和节点用同一个 `task_id`，跨电脑也能串起来查：

```bash
jq -c 'select(.task_id == "task-xxxx") | [.timestamp, .event, .status, .code]' logs/agentnexus.jsonl.*
```

- 配置里写错字段名时，启动日志会提示（`W_CONFIG_UNKNOWN_KEY`）。
- 完整的事件清单和错误码说明见 [设计文档第 10 节](docs/DESIGN.md#10-日志与排查)。

## Codex CLI

`config.toml` 里的 `[agents.coding]` 决定怎么调用 codex，占位符 `{prompt}` `{workdir}` `{last_message_file}` 会被替换。不同版本的参数可能不同，**请先运行 `codex exec --help` 核对**。`workdir` 建议指向一个独立的 git 仓库：执行后会收集 `git diff` 作为审查证据。找不到命令时 Coding Agent 会被标记为不可用，Router 不会选它。

## 目录结构

```
crates/
  core/      数据结构、状态机、错误码、节点协议、AgentAdapter trait（导出 TS 类型到 web/src/bindings）
  observe/   日志与诊断（AgentNexus 和节点共用）
  llm/       OpenAI 兼容客户端：结构化输出、<think> 清理、JSON 提取、校验重试
  agents/    general（工具循环）/ coding（codex 子进程）/ reviewer（只读审查）/ acp（通用 ACP 适配器）/ remote（节点上的 agent）/ tools
  runtime/   conversation / router / policy / orchestrator / registry（执行组）/ nodes（节点管理）/ composer / store / bus
  node/      节点程序 an-node
  server/    HTTP API + SSE + 静态前端；bin/eval.rs 路由评测；bin/mock_acp_agent.rs 模拟 ACP agent；tests/e2e.rs 端到端测试
web/         React + Vite 前端
evals/       路由评测集
```

## 开发

```bash
cargo test            # 单元测试 + 端到端测试（用假的 LLM 服务，不需要真模型）
```

```bash
cargo test -p an-core # 修改 core 的类型后运行，重新生成 web/src/bindings 下的 TS 类型
```

## MVP 刻意没做的

任务 DAG / 并行子任务、跨互联网的节点、节点 TLS、长期记忆（RelOS）、ACP 的文件/终端代理能力、断点续跑（服务重启时中断的任务标记为 FAILED）、多用户与鉴权、Slack 入口。

MCP 已增加配置式接入：General 使用轻量 Harness 调用 stdio / Streamable HTTP 工具，启动时发现真实 Schema，并执行本地允许清单和参数校验。接入说明见 [docs/mcp/README.md](docs/mcp/README.md)，完整设计和阶段限制见 [docs/mcp/DESIGN.md](docs/mcp/DESIGN.md)。当前支持文本资源产物；PPTX 等二进制文件的存储、下载和真实提供方集成仍在后续计划。

## API

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | /api/health | LLM 连通性、模型列表、Agent 状态 |
| GET | /api/capabilities | 已连接工具的能力卡片、provider 和输入输出 Schema |
| GET / POST | /api/conversations | 会话列表 / 新建会话 |
| GET / POST | /api/conversations/{id}/messages | 消息列表 / 发送消息 `{"text": "..."}` |
| GET | /api/tasks?conversation_id= | 任务列表 |
| GET | /api/tasks/{id} | 任务详情 + 事件 |
| POST | /api/tasks/{id}/cancel、/approve | 取消 / 人工批准 |
| GET | /api/route-logs?limit= | 路由日志 |
| GET | /api/events?conversation_id= | SSE 事件流 |
| GET | /api/nodes | 局域网发现的节点 + 已配对节点 |
| POST | /api/nodes/pair | 配对节点 `{"address": "192.168.1.23", "code": "757563"}` |
| DELETE | /api/nodes/{id} | 解除配对 |
| GET | /api/diagnostics | 诊断：最近的问题、错误码统计、执行组与节点状态 |
