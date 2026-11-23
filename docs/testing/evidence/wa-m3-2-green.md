# World Adapter M3-2 — Bind / Observe / 水位服务层 Green

分支：`cursor/wa-m3-2-prod-read-green-f2df`。按 [WA_M3_1_PLAN.md](../../mom/WA_M3_1_PLAN.md) / 决定 `2026-11-22-wa-m3-1-prod-read-service-layer`（合入 #213 批准）：**服务层 `/api/world/prod` + 生产 stub，不改 engine / store / 凭据 egress**。

## 范围

- `mom_world_prod`：`/api/world/prod/bind|observe|subscribe`（内存权威源登记）
- `main`：在 `[mom] world_mock` 门控下挂载（与 mock/des 同演示开关）
- 合同测试入库：`crates/server/tests/mom_world_prod.rs` → `RT-WAPROD-01`/`02`
- `features.json` `runtime_cases` 挂入上述 RT
- Playwright / 权限 / 延迟 / 写关闭属 **M3-3/M3-4**，本卡不宣称

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_prod -- --nocapture
test rt_waprod_01_bind_requires_registered_authority ... ok
test rt_waprod_02_observe_subscribe_watermark ... ok
test result: ok. 2 passed; 0 failed
```

相对 Red（`wa-m3-1-red.md`）：同断言由 404 → 业务通过。

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先，不改 engine/store/凭据 | 仅新增 `mom_world_prod` + main merge | 符合 |
| 新行为 Red→Green | M3-1 Red 已合；本卡 Green | 符合 |
| E2E 浏览器生产读 | 任务卡 M3-4 | 本卡不做 |

## 相对 M3 准出的进度与差距

| 准出项 | 状态 |
|---|---|
| 可绑定生产世界并只读观测（来源与水位） | **本卡部分完成**（合同 RT-01/02；浏览器待 M3-4） |
| 权限/跨范围拒绝 | 待 M3-3（RT-03） |
| 断线/游标过期续读无漏无重复 | 待 M3-3（RT-04；Subscribe 骨架已有） |
| 多来源延迟与 as_of | 待 M3-3（RT-05） |
| 写能力在生产世界上关闭 | 待 M3-3（RT-06；本卡未挂 Propose/Execute） |
| M0～M2 全量回归绿 | 待 M3-4/M3-5 |

## 非目标

不改 `engine.rs` / `store` 表结构 / `orchestrator` / DesConnector；不接真实 MES 凭据；不弱化 M0～M2；本卡不宣称 Playwright 生产读 Green。
