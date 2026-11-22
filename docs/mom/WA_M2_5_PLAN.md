# M2-5：浏览器远程写场景（服务层 + 前端）

承接 [WA_M2_TASKS.md](WA_M2_TASKS.md) M2-5、冻结 Playwright `web/e2e/wa-des-write.spec.ts`（`E2E-WAEXEC`/`WARECV`/`WADYN`/`WALOOP`）。  
**默认仍走已批服务层路径**；前端补齐定位契约。不改 engine / DesConnector 内部。  
证据：[wa-m2-5-browser.md](../testing/evidence/wa-m2-5-browser.md)。

## 1. 缺口

| 用例 | API | UI / 投影 |
|---|---|---|
| E2E-WARECV | 已 Green（M2-2/3） | 无 UI |
| E2E-WAEXEC | Propose/Execute 已绿 | 运营卡缺 `execution-evidence`；Execute 后须投影到频道 |
| E2E-WADYN | 干预/Observe 已绿 | 工作台无标准答案（M1 已有） |
| E2E-WALOOP | official 已绿 | 卡上「批准」须走 DES 执行；`execution-id` /「查证结果」/ `data-provenance=des` |

## 2. 实现

1. **服务层**（`mom_world_des`）：Execute 成功后 upsert 运营频道 `OpsCase`（`provenance=des`，summary 含 `execution_id=…`）；新增 `POST /api/world/des/ops/approve-execute`（批准+登记执行，更新原卡）。
2. **前端**（`OpsCards`）：`provenance=des` 时渲染 `execution-id`、`execution-evidence`（`data-provenance=des`）、按钮「查证结果」（Confirm）；「批准」走 DES approve-execute。
3. **套件**：`scripts/wa-m2-suite.sh` = Playwright `wa-des-write` + `wa-m2-regression` + `wa-m0`/`wa-m1` 回归。

## 3. 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 只扩 `mom_world_des` + `OpsCards` | 符合 |
| 不改 engine/DesConnector | 不改 | 符合 |
| 不断言弱化 | 保持冻结 Playwright 断言 | 符合 |

## 4. 非目标

不改 ChatCard 生成类型（execution_id 放 summary）；不接生产 MES；不改引擎状态机。
