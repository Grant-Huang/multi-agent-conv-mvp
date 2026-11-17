# MOM 车间 DES 阶段③ Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（阶段② 提交 c7a6a99 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §6.5 之前的阶段③修订说明；用例：[CASES.md](../CASES.md)「MOM 车间 DES 阶段③」。

范围：新增用例 RT-DES-08、09、10、11、18 与 HTTP-DES-03；接口骨架：`DesFault`、`Engine::select_des_plan`、`Engine::des_inject_fault`（返回"未实现"错误），HTTP 命令 `select_plan`。没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_08_selected_plan_b_changes_the_simulated_world ... FAILED
  plan B must be selectable: DES plan selection is not implemented yet
test rt_des_09_post_execution_same_source_observation_is_verification_evidence ... FAILED
  plan C must be selectable: DES plan selection is not implemented yet
test rt_des_10_new_disturbance_after_execution_still_invalidates_plan ... FAILED
  plan C must be selectable: DES plan selection is not implemented yet
test rt_des_11_tool_transfer_failure_comes_from_the_simulation ... FAILED
  plan C selectable: DES plan selection is not implemented yet
test rt_des_18_cases_sharing_resources_are_linked ... FAILED
  case CASE-s1-2 must be linked to another alarm case: []
test result: FAILED. 11 passed; 5 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des
test http_des_03_select_approve_execute_and_close_over_http ... FAILED
  command {"command":"select_plan","plan":"B"} failed: {"error":"DES plan selection is not implemented yet"}  left: 400
test result: FAILED. 2 passed; 1 failed
```

说明：
- RT-DES-10 检验的是既有行为（执行后新插单仍使计划失效），属于基线回归保护；它在 Red 中失败，是因为前置步骤"选定方案"尚未实现，不是因为既有行为有误。
- RT-DES-18 的前置条件（资源竞争场景中至少两台设备有报警案例）已满足，失败点在于没有记录关联。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 419 passed，仅上述 6 个新用例失败，exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                                   → ✓ built
```

## Green

待实现后补充。实现时不修改上述断言；如发现用例本身有误，先说明并经用户确认。
