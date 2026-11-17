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

待实现后补充。实现时不修改上述断言；如发现用例本身有误，先说明并经用户确认。
