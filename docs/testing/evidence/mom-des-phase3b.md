# MOM 车间 DES 阶段③b Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（审查修复 a9b3d45 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §6.8；用例：[CASES.md](../CASES.md)「MOM 车间 DES 阶段③b」。

范围：用户确认的 A+B 结合方案——按报警触发的模板（单设备层）与产线协同事项（协调层）。本提交只包含用例、设计、`FactoryConfig.coordination` 配置结构（默认 60 分钟、2 个案例）和测试；行为尚未实现。没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des rt_des_2
test rt_des_25_every_alarm_case_gets_a_plan_or_joins_an_incident ... FAILED
  ConcurrentCluster { window_minutes: 60, min_disruptions: 3 } seed 1: CASE-s1-3 has no plan ({"vibration"})
test rt_des_26_line_incident_coordinates_concurrent_cases ... FAILED
  seed 1: concurrent cases on one line need an incident
test rt_des_27_incident_decision_applies_to_all_alarmed_machines ... FAILED
  resource contention must yield an incident with two alarmed machines and a recommended plan within seeds 1–10
test rt_des_28_p0_keeps_single_case_without_incident ... ok      （基线）
test rt_des_29_closed_des_case_is_not_reused_or_refreshed ... ok （基线，来自 2026-11-17 审查）
```

RT-DES-28、29 是既有行为的回归保护，补录为基线，不算 Red。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 431 passed；失败的只有阶段④的 6 个与阶段③b 的 3 个实现前 Red 用例，exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid
cd web && npm run build                                                   → ✓ built
```

## Green

实现：`des/coordination.rs`（模板选择）、`des/engine_link.rs`（事项建立/加入、撤回决策、多设备依次维修、关闭级联）、`des/observe.rs`（事项排产、维修后恢复生产、已报告报警持续有效）、`engine.rs`（instantiate 与 close 的 DES 钩子）。

**断言修订（用户批准）。** 实现中发现：单设备案例收到报警当场请人决策，几分钟后同线报警到来时它已在等待决策；按原断言"成员从不请求决策"，这类案例不能加入事项，资源竞争模式 seed 1–10 中 0/10 能形成"多台设备 + 有推荐方案"的事项，RT-DES-27 无法满足。向用户给出三个方案（A 撤回后加入 / B 协调等待 / C 维持严格规则并扩大种子范围），用户选择 A（2026-11-17）。RT-DES-26 的这一条断言改为"加入前已请求的决策须在加入时撤回（`DecisionWithdrawn`），加入后不再请求决策"；其余断言未改。方案 A 下 seed 1–10 有 8 个形成上述事项。

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast
test rt_des_25_every_alarm_case_gets_a_plan_or_joins_an_incident ... ok
test rt_des_26_line_incident_coordinates_concurrent_cases ... ok
test rt_des_27_incident_decision_applies_to_all_alarmed_machines ... ok
test rt_des_28_p0_keeps_single_case_without_incident ... ok
test rt_des_29_closed_des_case_is_not_reused_or_refreshed ... ok
合计 434 passed，6 failed，exit=101；失败的只有阶段④尚未实现的 Red 用例：
  http_des_04_review_flow_over_http、rt_des_12、rt_des_13、rt_des_14、rt_des_15、rt_des_19

CARGO_TARGET_DIR=target cargo clippy --locked --all-targets   → 无警告
cargo fmt --all -- --check                                     → 通过
cd web && npm run test:map                                     → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                        → ✓ built
```

阶段①～③（RT-DES-01～11、16、20～24、HTTP-DES-01～03）全部保持通过；P0 固定剧本不启用 A/B，行为逐事件不变。

**超时、取消与清理。** 本阶段没有新增子进程或异步操作：事项协调在 `deliver_des` 的同一次引擎调用内同步完成，计算量受案例数与 `MAX_MACHINES` 限制；HTTP 入口沿用 `with_engine` 的阻塞线程封装。
