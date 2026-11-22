# World Adapter M2-3 — 恢复与幂等加深（T-55）

分支：`cursor/wa-m2-3-recovery-idempotent-f2df`。方案：[WA_M2_3_PLAN.md](../../mom/WA_M2_3_PLAN.md)。沿用已批服务层路径。

## 范围

- Confirm 支持 `correlation_key`（`RT-WADES-15`）
- `test/complete-partial` 部分回执补齐（`RT-WADES-16`）
- 工厂重启：历史可查 + pending 不误投（`RT-WADES-17`）
- Playwright `E2E-WARECV` 仍属 M2-5

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture
test rt_wades_09_propose_binds_plan_revision ... ok
test rt_wades_10_execute_idempotent ... ok
test rt_wades_11_cancel_pending_not_dispatched ... ok
test rt_wades_12_confirm_after_lost_receipt ... ok
test rt_wades_13_factory_instance_change_rejects_stale ... ok
test rt_wades_14_intervention_changes_future_events ... ok
test rt_wades_15_recover_by_correlation_key ... ok
test rt_wades_16_complete_partial_receipts ... ok
test rt_wades_17_factory_restart_history_no_misdispatch ... ok
test result: ok. 9 passed; 0 failed
```

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 仅扩 `mom_world_des` | 符合 |
| 高风险先方案 | 未改 engine/DesConnector/store/orchestrator | 符合 |
| E2E 浏览器 | M2-5 | 本卡不做 |

## 非目标

不改 engine / DesConnector / store 表；不宣称 Playwright WARECV Green。
