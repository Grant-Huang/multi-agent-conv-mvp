# egress-dlp：外发检查与脱敏

宿主在向外部发送内容前（登记的工具调用、云端 LLM 请求、排查的第 3/4 层）先交给 `dlp-egress` 判定。无界面入口，只有配置与后端行为。

## Sub-features

- 模式：`off`（默认）/ `audit`（检查并记录，从不拦截）/ `enforce`（拦截，只发送脱敏内容）
- 登记制：`[[egress.tools]]`（内置工具名；MCP 工具写 `服务id.工具名`）、`[[egress.llm]]`（非本机地址的 LLM 后端必须登记）
- 判定：只有 `ready_to_send` 才发，且只发脱敏文本；REVIEW / BLOCK / DLP 异常一律不发
- 回复还原：帮手/云端 LLM 回复里的 Token 在本机还原，仅用于显示；结束后撤销映射
- DLP 工具不出现在模型可见的工具清单
- 独立工具 `tools/dlp-egress`：MCP 协议、规则 + 评测集（700 条 + 留出集；GLiNER 未评测）

## How to get to it (user POV)

用户看不到界面。入口是 `config.toml` 的 `[egress]`（模板见 `config.example.toml`）；效果出现在：任务证据里的外发记录、排查“请其他助手帮忙”的预览、被拒绝的外发提示。

## Driving it with Playwright / HTTP

- 后端用例（DLP 与模型服务均为替身）：

  ```bash
  CARGO_TARGET_DIR=target cargo test --locked -p an-server --test agent_diagnose_send   # RT-DLP
  CARGO_TARGET_DIR=target cargo test --locked -p an-agents --test egress        # RT-EGR（工具守卫）
  CARGO_TARGET_DIR=target cargo test --locked -p an-llm --test stream_calls      # RT-EGR（云端 LLM 检查）
  AN_TEST_DLP_COMMAND=<真实命令> <同上 cargo test>                            # 设置后才会运行与真实 dlp-egress 的联调
  ```
- 浏览器：E2E-AGHELP 在 `harness.mjs` 里用 `crates/server/tests/fixtures/dlp_server.py` 把“ACME”换成占位符，验证诊断包与帮手回复还原。
- 工具侧：`tools/dlp-egress/docs/LOCAL_TEST_GUIDE.md`、`INTERFACE.md`；契约 `docs/dlp/CONTRACT.md`。

## Verify（判据）

- `enforce`：发出的请求体里没有原始敏感串，只有占位符；`REVIEW/BLOCK/异常` 路径上外部服务**收到 0 个请求**。
- 嵌套 dict/list 里的凭证被 BLOCK；8 种凭证类型全部 BLOCK；漏分析的字符串 fail closed；非凭证 Unicode 保留。
- 启动校验：`enforce` 下存在未登记的非本机 LLM 后端时拒绝启动。
- `audit`：永不拦截，但有记录。

## Gotchas

- 先用 `audit` 观察，再上 `enforce`。
- 宿主使用的 DLP 配置需 `allow_restore: true`，否则无法还原回复。
- 真实 dlp-egress 需等工具侧按 CONTRACT 第 8 节改完后才做联调验收；目前自动化主要用替身。
- 外发路径必须登记，漏登记 = 没有检查（`enforce` 对 LLM 后端会拒绝启动，对工具则要自己核对登记清单）。

## Backed by

源码：`crates/agents`（外发守卫，见 `tests/egress.rs`）、`crates/runtime`、`tools/dlp-egress/`
用例：RT-EGR-01～16、RT-DLP-01～06；PY-DLP-H1、PY-DLP-M3
文档：`docs/mcp/DLP_EGRESS_DEPLOYMENT.md`、`DLP_EGRESS_ENFORCEMENT_PLAN.md`、`docs/dlp/CONTRACT.md`
验证状态：替身下自动化覆盖；真实部署需按部署说明验收。
