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
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl
```

只跑 Turn Controller + Router（不执行任务），输出动作 / 角色 / 类别准确率、格式重试次数、P50/P95 延迟，详细结果（含模型原始输出）写入 `evals/results/`。换模型后重跑即可对比。把真实使用中路由错误的消息补进 `evals/routing.jsonl`，逐步积累评测集。

界面右侧的「路由日志」也能看到每轮的决策和模型原始输出。

## 小模型调优建议

| 现象 | 处理 |
|---|---|
| 格式经常出错、重试多 | 确认 `structured_mode = "json_schema"` 生效（llama.cpp 日志里能看到 grammar）；不支持时试 `json_object`；仍不行再用 `prompt_only` |
| llama.cpp 返回 400 | 多半是不支持该 `response_format` 写法，换另一种 `structured_mode` |
| 响应很慢 | 推理类模型（带 `<think>` 输出）会先思考很久，可以换非推理模型或在 llama.cpp 侧关闭思考；Router 可以单独配更小的模型（`[models]`） |
| 简单问题也被委派 | 调整 `crates/runtime/src/conversation.rs` 中的 system prompt，或换更大的模型做 conversation |
| 工具循环不收敛 | 调小 `agents.general.max_steps`，或给 General 配更大的模型 |

## Codex CLI

`config.toml` 里的 `[agents.coding]` 决定怎么调用 codex，占位符 `{prompt}` `{workdir}` `{last_message_file}` 会被替换。不同版本的参数可能不同，**请先运行 `codex exec --help` 核对**。`workdir` 建议指向一个独立的 git 仓库：执行后会收集 `git diff` 作为审查证据。找不到命令时 Coding Agent 会被标记为不可用，Router 不会选它。

## 目录结构

```
crates/
  core/      数据结构、状态机、AgentAdapter trait（导出 TS 类型到 web/src/bindings）
  llm/       OpenAI 兼容客户端：结构化输出、<think> 清理、JSON 提取、校验重试
  agents/    general（工具循环）/ coding（codex 子进程）/ reviewer（只读审查）/ tools
  runtime/   conversation / router / policy / orchestrator / composer / store / bus
  server/    HTTP API + SSE + 静态前端；bin/eval.rs 路由评测；tests/e2e.rs 端到端测试
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

任务 DAG / 并行子任务、长期记忆（RelOS）、MCP 工具接入（`Tool` trait 已预留）、断点续跑（服务重启时中断的任务标记为 FAILED）、多用户与鉴权、Slack 入口。

## API

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | /api/health | LLM 连通性、模型列表、Agent 状态 |
| GET / POST | /api/conversations | 会话列表 / 新建会话 |
| GET / POST | /api/conversations/{id}/messages | 消息列表 / 发送消息 `{"text": "..."}` |
| GET | /api/tasks?conversation_id= | 任务列表 |
| GET | /api/tasks/{id} | 任务详情 + 事件 |
| POST | /api/tasks/{id}/cancel、/approve | 取消 / 人工批准 |
| GET | /api/route-logs?limit= | 路由日志 |
| GET | /api/events?conversation_id= | SSE 事件流 |
