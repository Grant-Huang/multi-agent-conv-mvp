# MOM 车间 DES 阶段① Red → Green 证据

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

该提交推送后，远端 CI 的 rust 作业因这 7 个用例失败（预期的 Red，未跳过或忽略）。

## Green

实现位于 `crates/runtime/src/mom/des/`（`rng.rs` 随机流、`physics.rs` 规律模型、`scenario.rs` 场景生成与开环仿真、`engine_link.rs` 引擎接入），另在 `model.rs` 的快照中新增两个带默认值的字段，在 `simulation.rs` 中增加 DES 投递调用与仿真列表字段。没有修改 RT-DES 的任何断言。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_03_alarms_latch_without_recovery ... ok
test rt_des_02_spindle_wear_drives_vibration_before_cpk_only_when_mechanism_on ... ok
test rt_des_01_same_inputs_replay_byte_identical_events_and_truth ... ok
test rt_des_17_adding_unrelated_machine_keeps_other_random_streams ... ok
test rt_des_16_scenario_patterns_satisfy_constraints_or_fail_loudly ... ok
test rt_des_05_des_simulation_leaves_scripted_simulation_unchanged ... ok
test rt_des_04_ground_truth_is_isolated_from_agent_and_human_views ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
```

### 台账条目数断言（经批准的需求变更 A1）

登记 M27、M28 后，两处既有断言"台账共 24 项"按预期失败，改为 26 项。这是用户批准的需求变更，不是为了让测试通过而改测试；其他既有断言均未改动。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom   （改断言前）
test seed_changes_generated_events_without_fabricating_real_evidence ... FAILED
assertion `left == right` failed  left: 26  right: 24
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast  （改断言前）
test configurable_simulation_queues_events_and_exposes_gap_register ... FAILED   (crates/server/tests/mom.rs:175，left: 26 right: 24)
```

- `crates/runtime/tests/mom.rs::seed_changes_generated_events_without_fabricating_real_evidence`：24 → 26
- `crates/server/tests/mom.rs::configurable_simulation_queues_events_and_exposes_gap_register`：24 → 26

### 全量检查

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → exit=0；43 个测试结果汇总，413 passed，0 failed
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets             → Finished，无警告
cargo fmt --all -- --check                                               → 通过
cd web && npm run test:map                                               → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                                  → ✓ built，exit=0
```

### 合理性抽查（临时探针，未提交）

实现后用临时程序抽查各模式生成的场景，发现并修正了两处不合理：刀具到寿命末期必然报警（改为预防性换刀 + 实际寿命离散）、起点就报警（改为起点受控）。修正后：无起点报警；只开主轴机制、12 小时、10 个 seed × 3 台设备共出现 14 次振动报警、5 次质量报警，质量报警均晚于同设备振动报警。详见设计文档 §4.4 后的修订说明。

### 超时、取消和清理
- 不涉及子进程或异步操作。
- 计算量有上限：仿真时长 ≤ 2880 虚拟分钟，模式拒绝抽样 ≤ 256 次；工厂设备数目前未设上限（阶段①只有运行时接口，没有 HTTP 入口），阶段②开放 HTTP 时补上限与用例。
- DES 状态保存在快照中，重启后按现有逻辑暂停播放、继续投递未到期事件。

## 说明
- RT-DES-04 的标准答案字段名（root_causes、mechanisms、ground_truth、spindle_wear、wear_to_dimension）在现有 MOM 源码与夹具中均未出现（`grep` 核对），因此该用例不会被既有视图误判失败。
- "每个案例每小时观测数上限"依赖引擎的案例归并，放到阶段③实现并补用例（见设计文档 §5.4）。
- 设计偏差：DES 使用独立入口 `Engine::create_des_simulation`，不在 `SimulationConfig` 上加 `mode` 字段，原因见设计文档 §6.1 的修订说明。
