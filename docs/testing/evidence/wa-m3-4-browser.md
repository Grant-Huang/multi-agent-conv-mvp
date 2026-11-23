# World Adapter M3-4 — 浏览器生产读 E2E

分支：`cursor/wa-m3-4-browser-e2e-f2df`。按 [WA_M3_1_PLAN.md](../../mom/WA_M3_1_PLAN.md) §5 / #220 交接：工作台生产世界种子 + Playwright `E2E-WAPROD*`；**不改** engine / store / orchestrator；禁止真站点凭据/egress；WAM-07 准出收口属 **M3-5**。

## 范围

- 种子：`momWorkbenchData`「华东精加工 · 生产」/`provenance=production`
- 前端：`api.worldProd` + `MomWorkbench` Bind/Observe/Subscribe、水位/`as_of`、续读、写关闭
- 门控：`world_mock` 下 `WorldProdState::with_test_controls()`（与 DES 内存桩一致）
- Playwright：`web/e2e/wa-prod-read.spec.ts` → `E2E-WAPROD`/`WAPISO`/`WAPREAD`/`WAPDELAY`
- 套件：`scripts/wa-m3-suite.sh`（含 `wa-m2-suite` → M0/M1）

## Green 证据（VERIFIED / T2）

```text
$ cd web && npx playwright test e2e/wa-prod-read.spec.ts
  ✓ E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY
  4 passed

$ cd web && npx playwright test e2e/wa-des-remote.spec.ts e2e/mom-workbench.spec.ts
  ✓ E2E-WABIND / WADES / WAREAD / WAOBS
  4 passed

$ ./scripts/wa-m2-regression.sh && ./scripts/wa-m0-suite.sh && ./scripts/wa-m1-suite.sh
== wa-m2 regression green ==
== wa-m0 suite green ==
== wa-m1 suite green ==

$ cd web && npx playwright test e2e/wa-des-write.spec.ts
  ✓ E2E-WAEXEC / WARECV / WADYN / WALOOP
  4 passed

$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_prod
  6 passed
```

相对 M3-1 Red：同断言由工作台无生产世界 / 无水位控件 → UI Green。套件入口：`./scripts/wa-m3-suite.sh`。

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 只扩前端 + main 挂载测试控件；不改 engine/store | 符合 |
| 不断言弱化 | 保持冻结 Playwright Then；仅改父级 aria-label 避免与 `水位` 定位冲突 | 符合 |
| 真站点凭据/egress | 未引入 | 符合 |

## 相对 M3 准出的进度与差距

| 准出项 | 状态 |
|---|---|
| 可绑定生产世界并只读观测 | 合同 + **浏览器本卡完成** |
| 权限/跨范围拒绝 | 合同 RT-03 + E2E-WAPISO **本卡完成** |
| 断线/游标过期续读 | 合同 RT-04 + E2E-WAPREAD **本卡完成** |
| 多来源延迟与 as_of | 合同 RT-05 + E2E-WAPDELAY **本卡完成** |
| 写能力在生产世界上关闭 | 合同 RT-06 + E2E-WAPROD 写入口禁用 **本卡完成** |
| M0～M2 全量回归绿 | 套件脚本已挂；见本 PR 验证证据 |
| WAM-07 / 准出检查清单 | **待 M3-5** |

## 非目标

不改 `engine.rs` / `store` / `orchestrator`；不接真实 MES；不做 M3-5 准出收口；不自行开下一卡。
