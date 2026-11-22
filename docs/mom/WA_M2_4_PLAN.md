# M2-4：DES 闭环与 RT-DES/DESC 回归（服务层）

承接 [WA_M2_TASKS.md](WA_M2_TASKS.md) M2-4、[WORLD_ADAPTER.md](WORLD_ADAPTER.md) WA-05 / §6.2～6.3、`E2E-WADYN` 与 `RT-WADES-14`。  
**默认仍走已批服务层路径**；不改 engine / DesConnector 内部。`E2E-WADYN` 浏览器 Green 属 **M2-5**。

## 1. 与既有卡的关系

| 项 | 现状 | M2-4 |
|---|---|---|
| RT-WADES-14 | 干预后前缀不变、未来改变；审核可见答案 | 加深：**答案与干预一致** + **同步失败可补偿重试** |
| RT-DES-08～11 | `an-runtime` `mom_des` 已有 | 本卡跑回归并留证据 |
| RT-DESC-* | `mom_des_connector` / `mom_des_http_mcp` | 本卡跑回归并留证据 |
| E2E-WADYN | Playwright 空壳 | **M2-5** |

## 2. 实现（仅 `mom_world_des`）

1. 干预后写入 `ground_truth`：含 `execution_id` / `intervention_id`、`sync_status=synced`、答案与干预一致。
2. 测试控件：`fail-answer-sync` → `pending`；`retry-answer-sync` → 按最近干预补偿为 `synced`。
3. Observe / Confirm / Subscribe 仍不序列化标准答案（运营员拒读）。

## 3. 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 只扩 `mom_world_des` + 回归脚本/证据 | 符合 |
| 高风险先方案 | 不改 engine/DesConnector/store/orchestrator | 无需再批 |
| E2E 浏览器 | M2-5 | 本卡不做 |

## 4. 验收

- 合同：`RT-WADES-18`（答案与干预一致）、`RT-WADES-19`（答案同步失败可重试）
- `./scripts/wa-m2-regression.sh`：RT-DES-08～11 + RT-DESC（connector）+ `mom_world_des`/`_write` 绿
- 证据 `wa-m2-4-closed-loop.md`

## 5. 非目标

不改引擎物理闭环实现；不宣称 Playwright WADYN Green；不接生产 MES。
