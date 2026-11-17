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

实现：
- `crates/runtime/src/mom/des/observe.rs`（新增）：可观测快照、线性趋势预测、A/B/C 计划模型、DES 案例的工具读取。只用振动、Cpk、刀具计数、设备状态和资源配置，不读主轴磨损，不复跑带随机数的仿真。
- `scenario.rs`：记录选定的尝试序号并可重放；遥测增加刀具计数；设备数上限 32；P0 固定剧本中 CNC-XX 初始主轴磨损固定为 1.62，使背景遥测与剧本事件一致，事件读数取自遥测。
- `engine_link.rs`：DES 仿真保存重放输入（只在快照中，不进入视图）；每个虚拟分钟把可观测快照写入关联案例的 `world.des`；实现 `des_whatif` / `des_factory_view` / `des_forecast`。
- `tools.rs`：DES 案例的维护、质量、资源、排产四个工具改读快照（4 行分支）；脚本案例不变。

没有修改任何断言（RT-DES-21 在 Red 提交中为满足 clippy 把 `.err().expect()` 改写为 `let ... else`，断言含义不变，已在 Red 提交中完成）。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_06_whatif_is_read_only_and_compares_plans ... ok
test rt_des_07_infeasible_plans_list_hard_constraint_violations ... ok
test rt_des_20_tools_in_des_cases_read_simulation_state ... ok
test rt_des_21_factory_size_is_bounded ... ok
（阶段①的 7 个用例仍通过）
test result: ok. 11 passed; 0 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des
test http_des_01_factory_forecast_and_whatif_are_read_only_simulation_views ... ok
test http_des_02_invalid_requests_fail_fast_without_breaking_service ... ok
test result: ok. 2 passed; 0 failed
```

### 全量检查

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → exit=0；419 passed，0 failed
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets             → 无警告
cargo fmt --all -- --check                                               → 通过
cd web && npm run test:map                                               → Feature map valid
cd web && npm run build                                                  → ✓ built
```

### 合理性抽查（临时探针，未提交）

P0 固定剧本推进到 08:37（第 517 分钟），对 CNC-XX、500 件、16:00 交期做 what-if：
- A 继续使用：15:22 完成（第 802 分钟），但在报警设备上加工约 177 件风险件，带 `quality_risk:CNC-XX`，不可行。
- B 批后停机：15:20 完成（第 920 分钟），风险件 20，可行。
- C 立即停机：15:35 完成（第 935 分钟），风险件 0，可行，被推荐。
- B、C 都需要先调拨 CNC-XZ 的刀具。工厂视图：CNC-XX 振动 3.6 mm/s（C 区）、Cpk 1.152；CNC-XZ 缺刀。级联场景第 400 分钟的预测：CNC-XX 约 30 分钟后进入振动 C 区，CNC-XZ 约 29 分钟后 Cpk 跌破 1.33。

### 超时、取消和清理
- 不涉及子进程或异步操作；HTTP 处理在引擎锁内做有上限的同步计算。
- 计算上限：重放 ≤ 2880 分钟 × ≤ 32 台设备；what-if 推演 ≤ 2880 分钟，数量 ≤ 100000。
- 可观测快照每分钟只为有关联活动案例的 DES 仿真重放一次。
