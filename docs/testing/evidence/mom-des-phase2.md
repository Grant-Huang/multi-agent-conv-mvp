# MOM 车间 DES 阶段② Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`；基线：`origin/master` 59db0d6（阶段① 已合并）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §6.3、§6.4、§9 及阶段②修订说明；用例：[CASES.md](../CASES.md)「MOM 车间 DES 阶段②」。

范围：新增用例 RT-DES-06、07、20、21 与 HTTP-DES-01/02；接口骨架包括 `FactoryConfig.resources`（带默认值）、`WhatIfRequest`、`MAX_MACHINES`、`Engine::des_whatif` / `des_factory_view` / `des_forecast`（返回"未实现"错误），以及新文件 `crates/server/src/mom_sim.rs` 的路由（`mom.rs` 中只合并一行）。没有修改已有测试的断言。

## Red

未实现行为时运行。6 个新用例全部失败，原因都是业务层面的：接口未实现、DES 案例的工具仍读旧夹具（来源 EAM）、超限设备数被接受。不是编译或环境失败。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_06_whatif_is_read_only_and_compares_plans ... FAILED
  RT-DES-06: what-if must answer: DES what-if is not implemented yet
test rt_des_07_infeasible_plans_list_hard_constraint_violations ... FAILED
  what-if answers: DES what-if is not implemented yet
test rt_des_20_tools_in_des_cases_read_simulation_state ... FAILED
  assertion `left == right` failed: node maintenance must read DES  left: String("EAM")
test rt_des_21_factory_size_is_bounded ... FAILED
  33 machines must be rejected
（阶段①的 7 个用例仍全部通过）
test result: FAILED. 7 passed; 4 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des
test http_des_01_factory_forecast_and_whatif_are_read_only_simulation_views ... FAILED
  assertion `left == right` failed: factory view must be available  left: 400
test http_des_02_invalid_requests_fail_fast_without_breaking_service ... FAILED
  assertion `left == right` failed: 33 machines must be rejected  left: 200
test result: FAILED. 0 passed; 2 failed
```

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 413 passed，仅上述 6 个新用例失败，exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                                   → ✓ built
```

推送后远端 CI 的 rust 作业会因这 6 个用例失败（预期的 Red，未跳过或忽略）。

## Green

待实现后补充。实现时不修改上述断言；如发现用例本身有误，先说明并经用户确认。
