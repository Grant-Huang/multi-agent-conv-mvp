# MOM 车间 DES 阶段① 实现前 Red 证据

分支：`claude/relaxed-fermi-u38fye`；基线：`origin/master` a5f9a99。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md)；用例：[CASES.md](../CASES.md)「MOM 车间 DES 阶段①」。

范围：只新增用例、功能地图条目、接口骨架 `crates/runtime/src/mom/des.rs`（返回空结果或"未实现"错误）和测试 `crates/runtime/tests/mom_des.rs`。未修改已有测试、HTTP 接口、数据库结构、配置字段或高风险文件。

## Red

在未实现行为时运行。7 个用例全部失败；失败都来自业务断言或"未实现"的业务错误，不是编译或环境失败。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_01_same_inputs_replay_byte_identical_events_and_truth ... FAILED
test rt_des_03_alarms_latch_without_recovery ... FAILED
test rt_des_02_spindle_wear_drives_vibration_before_cpk_only_when_mechanism_on ... FAILED
test rt_des_16_scenario_patterns_satisfy_constraints_or_fail_loudly ... FAILED
test rt_des_17_adding_unrelated_machine_keeps_other_random_streams ... FAILED
test rt_des_04_ground_truth_is_isolated_from_agent_and_human_views ... FAILED
test rt_des_05_des_simulation_leaves_scripted_simulation_unchanged ... FAILED
seed 1: concurrent cluster must produce events, got 0
seed 1: telemetry required
seed 1: telemetry required
assertion `left == right` failed: seed 1: script pattern must reproduce the S1 timeline
new machine must appear in telemetry
RT-DES-04: DES simulation must be created: DES simulation is not implemented yet
RT-DES-05: DES simulation must be created next to a scripted one: DES simulation is not implemented yet
test result: FAILED. 0 passed; 7 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
```

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast
其余 42 个测试二进制全部 ok（共 406 个测试通过）；仅 mom_des 的 7 个新用例失败，exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets   → Finished，无警告，exit=0
cargo fmt --all -- --check                                     → exit=0
cd web && npm run test:map                                     → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                        → ✓ built，exit=0
```

推送后远端 CI 的 rust 作业会因这 7 个用例失败，这是预期的 Red，不跳过、不忽略，实现后转绿。

## Green

待实现后补充。实现时不得修改上述断言；如发现用例本身有误，先在 PR 中说明并经用户确认。

## 说明
- RT-DES-04 的标准答案字段名（root_causes、mechanisms、ground_truth、spindle_wear、wear_to_dimension）在现有 MOM 源码与夹具中均未出现（`grep` 核对），因此该用例不会被既有视图误判失败。
- "每个案例每小时观测数上限"依赖引擎的案例归并，放到阶段③实现并补用例（见设计文档 §5.4）。
- 设计偏差：DES 使用独立入口 `Engine::create_des_simulation`，不在 `SimulationConfig` 上加 `mode` 字段，原因见设计文档 §6.1 的修订说明。
