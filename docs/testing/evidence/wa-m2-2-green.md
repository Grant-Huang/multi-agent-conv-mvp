# World Adapter M2-2 — 写路径服务层 Green（T-54）

分支：`cursor/wa-m2-2-green-write-f2df`。按 [WA_M2_1_PLAN.md](../../mom/WA_M2_1_PLAN.md) / 决定 `2026-11-21-wa-m2-1-green-service-layer`：**服务层接线，不改 engine / DesConnector 内部**。

## 范围

- `mom_world_des`：`/api/world/des/propose|execute|confirm` + 测试控件（lose/restore/partial-fail/factory-restart/bump-plan/cancel/flush）
- 合同测试入库：`crates/server/tests/mom_world_des_write.rs` → `RT-WADES-09`～`14`
- `features.json` `runtime_cases` 挂入上述 RT
- Playwright E2E（`E2E-WAEXEC` 等）仍属 **M2-5**，本卡不宣称浏览器 Green

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture
test rt_wades_09_propose_binds_plan_revision ... ok
test rt_wades_10_execute_idempotent ... ok
test rt_wades_11_cancel_pending_not_dispatched ... ok
test rt_wades_12_confirm_after_lost_receipt ... ok
test rt_wades_13_factory_instance_change_rejects_stale ... ok
test rt_wades_14_intervention_changes_future_events ... ok
test result: ok. 6 passed; 0 failed
```

相对 Red（`wa-m2-1-red.md`）：同断言由 404 → 业务通过。

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先，不改 engine/DesConnector | 仅扩 `mom_world_des` | 符合 |
| 新行为 Red→Green | M2-1 Red 已合；本卡 Green | 符合 |
| E2E 浏览器闭环 | 任务卡 M2-5 | 本卡不做（未偏离，按卡切分） |

## 非目标

不改 `engine.rs` / `DesConnector` 内部 / `orchestrator` / `store` 表结构；不弱化 M0/M1；不接生产 MES；本卡不宣称 Playwright 写路径 Green。
