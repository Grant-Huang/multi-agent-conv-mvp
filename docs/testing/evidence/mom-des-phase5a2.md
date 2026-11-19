# 模拟工厂连接器 阶段⑤a-2 Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（master 8bede3a 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §11a"⑤a-2 设计"；决策：[DECISIONS.md](../../DECISIONS.md) DES-27；用例：[CASES.md](../CASES.md)「模拟工厂连接器（阶段⑤a-2）」。

范围：本提交只包含以下内容，行为尚未实现：
- 用例；
- 设计与决策；
- 骨架：
  - 引擎远程模式 `crates/runtime/src/mom/des/remote.rs`，方法都直接报"尚未实现"；
  - 服务层连接器 `crates/server/src/mom_des_connector.rs`，启动即报 `E_DES_UNAVAILABLE`；
- 失败测试。

没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_connector
test rt_desc_01_mcp_mode_matches_the_in_process_engine ... FAILED
test rt_desc_02_writes_only_after_approval_and_idempotent ... FAILED
test rt_desc_03_agents_only_see_observation_tools ... FAILED
test rt_desc_04_offline_factory_fails_loudly_without_fallback ... FAILED
test rt_desc_05_review_truth_comes_from_the_factory_admin_tool ... FAILED
  the simulated factory starts: E_DES_UNAVAILABLE: connector is not implemented yet
test result: FAILED. 0 passed; 5 failed
```

失败原因是连接器尚未实现（启动即返回错误），不是环境问题：`an-des-mcp` 已构建，可执行文件存在。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 554 passed，5 failed（只有上述 5 个新用例），exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 27 features, 24 browser cases
cd web && npm run build                                                   → ✓ built
```

## Green

待用户确认用例后实现并补充。
