# World Adapter M2-4 — DES 闭环与回归（T-57）

分支：`cursor/wa-m2-4-des-closed-loop-f2df`。方案：[WA_M2_4_PLAN.md](../../mom/WA_M2_4_PLAN.md)。沿用已批服务层路径。

## 范围

- `RT-WADES-18`：答案与干预一致（`synced`）
- `RT-WADES-19`：答案同步失败 → 补偿重试
- `./scripts/wa-m2-regression.sh`：WADES 读/写 + RT-DES-08～11 + RT-DESC connector
- Playwright `E2E-WADYN` 仍属 **M2-5**

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture
… rt_wades_18_answer_matches_intervention ... ok
… rt_wades_19_answer_sync_retry_after_failure ... ok
test result: ok. 8 passed

$ ./scripts/wa-m2-regression.sh
== RT-DES-08～11 == 4 passed
== RT-DESC connector == 6 passed
== wa-m2 regression green ==
```

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 仅扩 `mom_world_des` + 回归脚本 | 符合 |
| 高风险先方案 | 未改 engine/DesConnector | 符合 |
| E2E 浏览器 | M2-5 | 本卡不做 |

## 非目标

不改引擎物理闭环；不宣称 Playwright WADYN Green；HTTP MCP（RT-DESC-06～11）未纳入本卡套件（connector 覆盖核心 DESC）。
