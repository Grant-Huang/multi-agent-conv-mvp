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

实现：
- `des/scenario.rs`：仿真支持干预（维修期间停机，结束时主轴磨损复位为 0.2 并解除报警锁存；停机期间不判 Cpk）；`rerun` 按尝试序号和干预重新推演；执行后世界继续推演到查询时刻，事件只保留场景窗口内的。
- `des/observe.rs`：可观测快照反映已到达的刀具调拨、已占用的备件与备件丢失故障。
- `des/engine_link.rs`：`select_des_plan`（只能选可行方案，选定后作废原批准）、`des_inject_fault`、`des_after_execute`（方案 → 干预 → 重新推演，替换未投递事件与标准答案，设置动作完成时刻；备件不足时调拨失败、PARTIAL、`CompensationRequired`）、同线报警案例互相关联（`RelatedCaseLinked`）。
- `engine.rs`（17 行）：`execute` 成功后调用 DES 回调；`ingest` 中 DES 案例执行后同一设备的振动/质量观测记为核验证据。两处只对有 `world.des` 的案例生效。
- `server/src/mom.rs`：命令 `select_plan`。

没有修改任何断言。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_08_selected_plan_b_changes_the_simulated_world ... ok
test rt_des_09_post_execution_same_source_observation_is_verification_evidence ... ok
test rt_des_10_new_disturbance_after_execution_still_invalidates_plan ... ok
test rt_des_11_tool_transfer_failure_comes_from_the_simulation ... ok
test rt_des_18_cases_sharing_resources_are_linked ... ok
test result: ok. 16 passed; 0 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des
test http_des_03_select_approve_execute_and_close_over_http ... ok
test result: ok. 3 passed; 0 failed
```

实现过程中发现的问题：RT-DES-08 第一次运行失败（`CNC-XX back in service`）。原因是 P0 场景只有 240 分钟，而方案 B 的维修在场景结束后才完成，查询被截在最后一个样本（仍在维修）。修正为执行后世界继续推演到查询时刻（见设计文档阶段③修订说明），断言未改。

### 全量检查

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → exit=0；425 passed，0 failed
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets             → 无警告
cargo fmt --all -- --check                                               → 通过
cd web && npm run test:map                                               → Feature map valid
cd web && npm run build                                                  → ✓ built
```

### 超时、取消和清理
- 不涉及子进程或异步操作；`execute` 回调和关联在引擎锁内同步完成。
- 计算上限：每次重新推演 ≤ 3 × 2880 分钟 × ≤ 32 台设备；回调只在 DES 案例的首次生产执行时运行一次（以计划版本去重）。
- 回调失败时 `execute` 返回错误；此时 `execute` 本身的动作记录已提交，按现有失败对账流程处理（与原有下发失败语义一致）。

