# AG.1 Agent 目录与握手检测证据

日期：2026-11-16。用户在会话中批准 AG.1 方案（含修改高风险文件 `acp.rs` 的握手检测）和 10 条验收条件。

## Red → Green

`CARGO_TARGET_DIR=target cargo test --locked -p an-server --test agent_catalog`

实现前：`FAILED. 0 passed; 9 failed`。全部是业务断言：两个新接口返回 404（例如 RT-AGCAT-01 `目录接口应可用`：left 404，right 200；RT-AGCAT-08 `带 command 字段的请求应被拒绝（4xx，非 404），实际 404`）。

实现后（断言未改）：`ok. 9 passed; 0 failed; finished in 10.39s`（RT-AGCAT-06 等待 10 秒超时）。

另加单元测试：`acp::tests::probe_errors_are_classified`（-32000 → 需要登录；`env: node`、`ERR_UNKNOWN_BUILTIN_MODULE` → 缺依赖；只带 `node:internal` / `Node.js v22` 的普通报错不算缺依赖）、`acp::tests::detail_keeps_the_tail`、`catalog::tests::every_preset_has_meta`。

测试辅助：模拟 ACP agent 增加 `AN_MOCK_ACP_AUTH=required` 模式，新建会话时返回 `acp::Error::auth_required()`（-32000）。

## 改动与边界

- `acp.rs`：新增 `probe_detailed(timeout)` 返回分类结果；子进程的启动、握手请求和清理方式与原来相同。原 `probe()` 的签名和错误文字保留（经 `probe_result` 转换），后台探测与连接状态行为不变；握手失败时错误文字现在会附上 SDK 提供的进程错误输出末尾（最多 2000 字符）。
- 依据：`agent-client-protocol` 5.0.0 源码，`ErrorCode::AUTH_REQUIRED = -32000`（`src/error.rs:73`）；进程非零退出时错误的 `data` 带错误输出末尾（`src/acp_agent.rs:186`、`:600-606`）。
- 超时、取消与清理：每次检测 10 秒；超时或 HTTP 请求被取消时丢弃连接，SDK 结束 agent 进程组（RT-AGCAT-06 验证进程已结束）。同一 agent 串行，全局最多 2 个并发检测；未安装时不启动进程、不占名额。
- 只接受 id：请求体 `deny_unknown_fields`，带 `command` 返回 422，命令未执行（RT-AGCAT-08）。
- 不改 `AcpConfig`，不新增配置字段；不写配置，不改注册表。
- 新公开类型（an-core，已生成 TS）：`InstallStep`、`CatalogStatus`、`CatalogItem`、`ProbeKind`、`ProbeOutcome`。新事件：`agent.probe`（DESIGN.md 10.3）。

未验证：真实 claude-agent-acp / codex-acp / cursor-agent / zcode-acp-server 的未登录、缺依赖时的实际报错文字（沙箱未安装）；缺依赖的特征列表可能需要按用户本机实测补充。opencode、Cursor CLI 的安装命令未能在当前环境核实，目录里只写说明和文档链接。没有测试"同一 agent 并发检测串行化"和"全局并发上限"。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked：420 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：第一次报 2 处 single_char_add_str（acp.rs 新代码），改为 push('：') 后 Finished，exit=0
web npm test：Test Files 7 passed (7)，Tests 21 passed (21)
web npm run build：✓ built in 877ms；exit=0
web npm run test:map：Feature map valid: 19 features, 14 browser cases
CARGO_TARGET_DIR=target cargo test -p an-core：重新生成 web/src/bindings（新增 5 个类型文件）
```

本地结果不代替 GitHub CI。
