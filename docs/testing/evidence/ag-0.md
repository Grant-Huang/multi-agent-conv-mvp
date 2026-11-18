# AG.0 ACP 自动发现预设证据

日期：2026-11-16。基线：fddfdc1。用户在会话中批准方案（预设清单、Cursor 只认 `cursor-agent`、5 条验收条件）。

## Red → Green

`CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test acp_discovery`

实现前：`FAILED. 3 passed; 2 failed`。
- RT-ACPD-01：`已安装 claude-agent-acp，但没有自动发现 agent \`claude\``（业务断言）
- RT-ACPD-05：`新预设应排在原有预设之后`，实际为空列表（业务断言）
- RT-ACPD-02 / 03 / 04 在实现前已通过，属于防回归的守护用例（通用名 `agent` 不认作 Cursor、用户同 id 配置优先、未安装不注册）

实现后（只改 `crates/agents/src/acp.rs` 的 `presets()`，断言未改）：`ok. 5 passed; 0 failed`。
另加单元测试 `acp::tests::new_presets_use_documented_commands`（命令与参数、不认 `agent`、id 不与内置 `codex` 冲突）。

## 生命周期与边界

不新增子进程逻辑：发现阶段只检查文件是否存在且可执行，不运行；注册后的连通性探测沿用现有 `AcpAgent::probe()` 的超时与清理（连接 drop 时 SDK 结束 agent 进程组）。

测试修改进程级 PATH / HOME，用文件内的锁串行执行；本文件是独立测试二进制。`/usr/local/bin`、`/opt/homebrew/bin` 无法在测试里隔离，RT-ACPD-04 按同一查找规则核对"卡片出现 ⇔ 命令可找到"。

未验证：真实 claude-agent-acp / codex-acp / cursor-agent / zcode-acp-server 的 ACP 握手与任务执行（沙箱未安装），需在用户本机验证。`an-node` 启动提示文字仍只列 opencode / hermes / pi-acp（`crates/node/` 属 L3，未在本次修改），节点的自动发现逻辑复用同一份 `presets()`，已自动包含新预设。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked：403 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：Finished dev profile；exit=0
web npm test：Test Files 7 passed (7)，Tests 21 passed (21)
web npm run build：✓ built in 261ms；exit=0
web npm run test:map：Feature map valid: 18 features, 14 browser cases
```

本地结果不代替 GitHub CI。
