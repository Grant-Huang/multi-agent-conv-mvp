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

## 补充：握手检测"进程一启动就退出"的偶发误判（2026-11-18，RT-AGCAT-10）

- **现象**：CI 上 RT-AGDIAG-01 偶发失败：缺 node 被判成 `handshake_failed`，detail 为 `Internal error: "Broken pipe (os error 32)"`。本地用当时的代码复现：40 次里 RT-AGDIAG-01 失败 7 次，RT-AGCAT-05 失败 9 次。把 ACP 库退回 5.0.0 后分别失败 7 次和 5 次，说明与依赖升级无关，是 AG.1 起就有的问题。
- **根因**：进程打印错误后立刻退出，写握手请求遇到 Broken pipe；SDK 有时先返回这个写错误，而不是"进程退出 + 错误输出末尾"，于是分类时看不到错误输出。
- **Red**：`cargo test -p an-agents --test acp_probe`（每轮 8 个并发检测、连续 50 轮）。修复前连续 3 次，400 次检测里分别有 13、15、9 次被判为 HandshakeFailed。注意：只逐个串行检测时 50 次全对，复现不出来；CI 机器上这条用例也曾碰巧通过。
- **试过但无效**：用 SDK 的 `with_debug` 回调自己留一份错误输出。出错返回时 SDK 还没读到错误输出，连接一结束读取也随之停止。改后仍有 5～12 次分错，已撤回。
- **修复**（用户批准；改动在高风险文件 `crates/agents/src/acp.rs`，只改 `probe_detailed`）：
  - 检测用的子进程由我们自己启动（`proc_group`：进程组、取消时 SIGKILL 整个组），把 stdin、stdout 通过 `ByteStreams` 接给 SDK，自己读完 stderr（只保留末尾 2000 字符）。
  - 握手结束后最多等 0.5 秒让进程退出，退出了就清理组内残留进程，否则 SIGTERM，等 3 秒，再 SIGKILL。超时时同样结束整个进程组。
  - 失败时，详情 = SDK 错误 + 退出状态 + 自己读到的错误输出，再分类。
- **Green**：RT-AGCAT-10 连续 10 遍全部通过（4000 次检测无误判）。服务端 RT-AGDIAG-01、RT-AGCAT-05 各跑 40 次，失败均为 0。RT-AGCAT-01～09、RT-AGDIAG 全部通过，断言未改。
- **完整检查**：`cargo fmt --check` exit=0；`cargo test --locked --no-fail-fast` passed=516 failed=0 ignored=2；`cargo clippy --locked --all-targets -D warnings` exit=0（本机工具链已是 1.100，与 CI 相同）；web：`npm test` 21 passed，`npm run build` ✓，`npm run test:map` 25 features。
- **依赖**：开启已有依赖 `tokio-util` 的 `compat` 功能，`Cargo.lock` 没有新增包。
