# research-and-tools：研究报告、工具与 MCP、模板

覆盖 features.json 的 reports、tools-evals、templates，以及 MCP 配置式接入。

## Sub-features

- **reports（研究报告）**：同时要求 `web_search` + `write_file` 时走代码控制流程：规划 → 检索（≤6 次、去重）→ 章节并行写作 → 汇合；过半章成功才完成；无效引用剔除；取消后不再请求。只要求 `web_search` 时走逐步工具循环（先搜后写）
- **General 工具**：读写文件（只限 `[agents.general].workspace_dir`）、计算、`websearch`（命令不在 PATH 则不注册）
- **MCP**：`[[mcp.servers]]` stdio / Streamable HTTP；启动时握手 + `tools/list` 发现 Schema；仅开放显式列出的工具；调用前校验参数；`GET /api/capabilities` 能力目录
- **tools-evals**：终端工具 `websearch`、`an-eval`（路由评测）、`dream`；失败草稿，不自动改主评测集
- **templates（仅校验）**：模板校验、Router 拆分决定、分支信封与汇合输入在 Runtime 集成层验证；**没有界面入口，并行执行与 Composer 汇合回复未实现**

## How to get to it (user POV)

对话里说“写一份报告，引用公开资料/最新信息，保存成 Markdown” → 任务走马灯依次出现规划、检索、写作 → 产物里出现 `.md`。MCP 工具没有单独页面：配置后由 General 在任务中调用，调用记录进入任务证据。

## Driving it with Playwright / HTTP

- 浏览器 E2E 里模型是受控替身，只能验证“工具真实写文件 → 审查 pass → COMPLETED → 下载字节一致”（E2E-FILE，见 [tasks.md](tasks.md)）；研究报告的控制流由 Rust 集成测试用受控模型驱动：

  ```bash
  CARGO_TARGET_DIR=target cargo test --locked -p an-agents --test report_flow   # RT-REP-01～09
  CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test template_cases   # RT-TPL-01～10
  CARGO_TARGET_DIR=target cargo test --locked -p an-agents --test mcp
  CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mcp
  ```

- MCP 能力查看：`curl -s localhost:8787/api/capabilities | jq .`（含 provider 与输入输出 Schema）
- 路由评测：`cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --label X`

## Verify（判据）

- 报告：检索次数 ≤ `report_max_searches`；章节数与大纲一致；引用都能在检索结果中找到；取消后模型请求数不再增加。
- MCP：配置缺失/Schema 无效/连接失败时**启动即报错**；`external_write` 与 `destructive` 调用必定被拒；超时或取消后直接 stdio 子进程已终止、不自动重试。
- 产物：文本资源可进入任务产物；二进制不会被当作可下载文件。

## Gotchas

- 报告质量与搜索时效必须用真实模型与真实搜索验收；自动化只证明控制流。
- `effect` 是配置里人工声明的，不是服务 annotations 推断；`workspace_write` 不是 OS 级隔离；stdio 进程拥有当前用户权限。
- 外发脱敏 `dlp-egress` **不要**配成 `[[mcp.servers]]`，它由宿主经 `[egress]` 调用，不进模型可见清单（见 [egress-dlp.md](egress-dlp.md)）。
- 图片/PPTX 等二进制产物、真实提供方集成仍在后续计划（见 `docs/mcp/PLAN.md`）。
- 并发写作占后台槽位：`report_parallel` 默认 3，给其他任务留 1 个。

## Backed by

源码：`crates/agents/src/general.rs`、`tools`、MCP 相关模块（`crates/agents/`）、`crates/server/src/bin/eval.rs`
用例：RT-REP-01～09（`crates/agents/tests/report_flow.rs`）、RT-TPL-01～10（`crates/runtime/tests/template_cases.rs`）；`crates/agents/tests/mcp.rs`、`crates/server/tests/mcp.rs`
文档：`docs/mcp/README.md`、`DESIGN.md`、`PLAN.md`
验证状态：控制流自动化已覆盖；真实模型质量/搜索时效未验收。
