# World Adapter M3-3 — 权限 / 续读 / 延迟 / 写关闭 Green

分支：`cursor/wa-m3-3-prod-guards-f2df`。按 [WA_M3_1_PLAN.md](../../mom/WA_M3_1_PLAN.md) / #218 交接：服务层 stub 加深；**不改** engine / store / orchestrator；禁止真站点凭据/egress；浏览器属 M3-4。

## 范围

- `mom_world_prod`：`ACCESS_DENIED`（角色/跨站）、`CURSOR_EXPIRED` + 恢复续读、`test/inject-delay` 多源 `as_of`、`propose`/`execute` → `WRITE_DISABLED`
- 测试控件：`test/expire-cursor`、`test/inject-delay`（仅 `with_test_controls`）
- 合同入库：`crates/server/tests/mom_world_prod.rs` → `RT-WAPROD-03`～`06`（01/02 仍绿）
- Playwright 浏览器生产读属 **M3-4**，本卡不宣称

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_prod -- --nocapture
test rt_waprod_01_bind_requires_registered_authority ... ok
test rt_waprod_02_observe_subscribe_watermark ... ok
test rt_waprod_03_access_denied_cross_scope ... ok
test rt_waprod_04_cursor_expired_then_recover ... ok
test rt_waprod_05_inject_delay_aligns_as_of ... ok
test rt_waprod_06_write_closed_on_prod_mock_still_works ... ok
test result: ok. 6 passed; 0 failed
```

相对 M3-1 Red / M3-2 骨架：权限拒绝不泄露对象；过期游标 409；延迟源 `observed_at` 对齐水位；生产写明确拒绝且 Mock Propose 仍 200。

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先，不改 engine/store/凭据 | 仅扩展 `mom_world_prod` + 合同测试 | 符合 |
| 新行为 Red→Green | 断言先于实现；本卡 Green | 符合 |
| 测试控件默认关 | `WorldProdState::new()` 关；测试用 `with_test_controls` | 符合 |
| E2E 浏览器生产读 | 任务卡 M3-4 | 本卡不做 |

## 相对 M3 准出的进度与差距

| 准出项 | 状态 |
|---|---|
| 可绑定生产世界并只读观测（来源与水位） | 合同 RT-01/02 已绿（M3-2）；浏览器待 M3-4 |
| 权限/跨范围拒绝 | **本卡完成**（RT-03） |
| 断线/游标过期续读无漏无重复 | **本卡完成**（RT-04） |
| 多来源延迟与 as_of | **本卡完成**（RT-05） |
| 写能力在生产世界上关闭 | **本卡完成**（RT-06；Mock 写仍可用） |
| 浏览器生产读 + M0～M2 全量回归 | 待 M3-4 |
| 证据 / WAM-07 / 准出检查 | 待 M3-5 |

## 非目标

不改 `engine.rs` / `store` / `orchestrator`；不接真实 MES 凭据；不做 Playwright `E2E-WAPROD*` Green；不自行开 M3-4。
