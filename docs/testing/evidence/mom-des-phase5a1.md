# 模拟工厂与 MCP 挂载 阶段⑤a-1 Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（阶段④实现 a1ffc96 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §11a；用例：[CASES.md](../CASES.md)「模拟工厂与 MCP 挂载（阶段⑤a-1）」。

范围：本提交只包含以下内容，行为尚未实现：
- 用例；
- 设计修订：工具名改用下划线，分两步实施；
- 两个新 crate 的骨架：
  - `an-des`：工具类别与目录类型、`FactoryService::call`；
  - `an-des-mcp`：可执行文件，目前直接退出；
- 失败测试。

`Cargo.lock` 新增 `an-des`、`an-des-mcp`，以及 rmcp 服务端特性引入的依赖。没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-des -p an-des-mcp --no-fail-fast
test rt_desf_01_tool_catalog_is_generic_and_categorised ... FAILED
  catalog must match the design table  left: {}  right: {"equipment_status": Observe, … "simulation_ground_truth": Admin, …}
test rt_desf_02 … rt_desf_08 ... FAILED
  simulation_create: tool simulation_create is not implemented yet
test result: FAILED. 0 passed; 8 failed
test rt_desm_01_mcp_lists_the_factory_catalog ... FAILED
test rt_desm_02_mcp_results_match_the_in_process_service ... FAILED
test rt_desm_03_errors_are_tool_errors_and_stdin_close_exits ... FAILED
  MCP handshake must succeed: ConnectionClosed("initialize response")
test result: FAILED. 0 passed; 3 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des rt_des_30
test rt_des_30_factory_service_matches_the_in_process_engine ... FAILED
  simulation_create: tool simulation_create is not implemented yet
```

失败原因都是业务未实现，不是环境问题：
- 目录为空；
- 工具未实现；
- 服务程序没有完成 MCP 握手。

MCP 测试的每个等待都有 20 秒超时，服务未实现时测试会失败，不会挂起。

## 其他检查（同一提交）

见提交说明与 PR 描述。

## Green

用户确认用例后实现，分两个提交，没有修改任何测试断言：

1. `refactor(des)`：仿真内核迁入 `an-des`，行为不变。MOM 侧重新导出原名称，原有测试一行未改，全部通过。迁移后 an-runtime 的测试中只有 RT-DES-30 失败，仍是预期的 Red。
2. `feat(des-factory)`：实现 `FactoryService` 的 13 个工具，以及 `an-des-mcp`（rmcp 3.9 服务端，stdio）。

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 452 passed，0 failed，exit=0
  rt_desf_01_tool_catalog_is_generic_and_categorised ... ok
  rt_desf_02_same_seed_same_results_and_cursor_paging ... ok
  rt_desf_03_observation_tools_are_read_only ... ok
  rt_desf_04_write_tools_need_approval_and_are_idempotent ... ok
  rt_desf_05_tool_transfer_and_injected_fault ... ok
  rt_desf_06_ground_truth_needs_admin_token_and_never_leaks ... ok
  rt_desf_07_invalid_requests_fail_fast_without_side_effects ... ok
  rt_desf_08_simulation_count_is_bounded_and_ids_are_sequential ... ok
  rt_desm_01_mcp_lists_the_factory_catalog ... ok
  rt_desm_02_mcp_results_match_the_in_process_service ... ok
  rt_desm_03_errors_are_tool_errors_and_stdin_close_exits ... ok
  rt_des_30_factory_service_matches_the_in_process_engine ... ok
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 19 features, 14 browser cases
cd web && npm run build                                                   → ✓ built
```

本机磁盘额度有限，以上命令在设置 `CARGO_PROFILE_DEV_DEBUG=0` 的环境中运行。这个变量只去掉调试信息，不影响测试行为，也没有写进仓库配置；CI 仍按默认配置构建。

**超时、取消与清理**
- `an-des-mcp`：不启动子进程，不访问网络与文件；stdin 关闭后会话结束、进程退出（RT-DESM-03 验证 5 秒内退出）。
- 仿真计算在 `spawn_blocking` 中执行。同一时刻只处理一个调用；计算量受仿真时长、设备数与推进上限约束。
- 测试启动的子进程设置了 `kill_on_drop`；每个等待都有 20 秒超时。

