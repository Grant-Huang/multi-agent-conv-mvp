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

待实现后补充。实现时不修改上述断言；如果发现用例本身有误，先说明并经用户确认。
