# World Adapter M3 执行任务卡

本文件是 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) **M3 生产只读**的执行版。硬约束仍见该文 §0（DES-32）；生产适配见 [WORLD_ADAPTER.md](WORLD_ADAPTER.md) §7；Mock 替换见 [WA_MOCK_REGISTRY.md](WA_MOCK_REGISTRY.md) WAM-02/03/07。M0～M2 已准出。

**准出（缺一不可）**：可绑定生产世界并只读观测（含来源与截至水位）；权限/跨范围拒绝可执行；断线或游标过期后续读无漏无重复；多来源延迟与 `as_of` 可辨；写能力在生产世界上声明关闭或仍走隔离 Mock/DES；全量 M0～M2 套件回归绿。

**里程碑结论（M3-5）**：上述准出项在 **桩数据** `/api/world/prod` 路径上已齐；证据 [wa-m3-5.md](../testing/evidence/wa-m3-5.md)。真站点只读验收另开。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| M3-0 | 冻结用例草案与任务卡；同步 PLAN / Feature Map / 注册表 | **完成**（#209） | M2 准出 |
| M3-1 | 正式 CASES + Red 空壳；生产读合同（高风险先方案） | **完成**（#213） | M3-0 |
| M3-2 | 绑定 / Observe / 水位服务层 Green | **完成**（#218） | M3-1 |
| M3-3 | 权限、跨范围、延迟与多源 | **完成**（#220） | M3-2 |
| M3-4 | 浏览器生产读场景 + M0～M2 全量回归 | **完成**（#224） | M3-3 |
| M3-5 | 证据、WAM-07 读路径状态、准出检查 | **进行中**（本 PR） | M3-4 |

一张卡 ≈ 一个 PR。每卡先 Red 再 Green；断言冻结后不得弱化。

## 1. 正式 id（已写入 CASES 表格，M3-1）

设计项侧重 **生产只读**（WA-02 续读在生产源、WA-07 能力可见性、WAM-07 生产来源读替换）。**WA-06 生产写确认属 M4**，本里程碑只保证写演示仍在隔离世界。

| 设计 | 正式 id | 要点 |
|---|---|---|
| 生产绑定与观测 | `E2E-WAPROD` | 选择生产世界 → 看板/观测可见正式读；`provenance=production`；写入口关闭或指向隔离世界 |
| 权限与隔离 | `E2E-WAPISO` | 无权限/跨站点读拒绝；列表与详情不泄露 |
| 断线续读 | `E2E-WAPREAD` | 断线或游标过期后重新读取；同事件只应用一次；水位可辨 |
| 多源延迟 | `E2E-WAPDELAY` | 多来源 `as_of`/延迟可见；过期与正常空态可区分 |
| 合同：绑定 | `RT-WAPROD-01` | 生产世界 bind 成功；未配置权威源明确错误 |
| 合同：观测水位 | `RT-WAPROD-02` | Observe/Subscribe 对齐正式水位；含 source / as_of |
| 合同：权限 | `RT-WAPROD-03` | 跨 scope / 无授权 → 明确拒绝（非 200） |
| 合同：续读 | `RT-WAPROD-04` | 过期游标 → `CURSOR_EXPIRED`；恢复后续读无漏无重复 |
| 合同：延迟多源 | `RT-WAPROD-05` | 注入延迟后水位与载荷截至时间一致 |
| 合同：写关闭 | `RT-WAPROD-06` | 生产世界 Propose/Execute 能力关闭或拒绝；Mock/DES 写路径仍可用 |
| 回归 | `wa-m0/m1/m2/m3-suite` + 既有 RT-WA/WADES/DES/DESC | M3 合入前全绿 |

## 2. M3-0～M3-4（已完成）

| 卡 | 证据 |
|---|---|
| M3-0 | `docs/testing/evidence/wa-m3-0.md`（#209） |
| M3-1 | `wa-m3-1-red.md`；方案 [`WA_M3_1_PLAN.md`](WA_M3_1_PLAN.md)（#213） |
| M3-2 | `wa-m3-2-green.md`（#218） |
| M3-3 | `wa-m3-3-green.md`（#220） |
| M3-4 | `wa-m3-4-browser.md`；套件 `scripts/wa-m3-suite.sh`（#224） |

## 3. M3-5：证据 / WAM-07 / 准出（本卡）

1. 准出证据 `docs/testing/evidence/wa-m3-5.md`；方案 [`WA_M3_5_PLAN.md`](WA_M3_5_PLAN.md)。
2. `WA_MOCK_REGISTRY` WAM-07 **读**路径定稿；入口 `wa-m3-suite` / `wa-m3-5.md`。
3. 同步 PLAN / Feature Map / `features.json` / CASES。
4. **仍不改** engine / store / orchestrator / 真站点凭据；不自行开 M4。

## 4. 不做（M3）

- 不做真实 MES 写 / WA-06 生产确认（M4）。
- 不退役 `/api/world/mock` 与 `/api/world/des`。
- 不弱化 M0～M2 已冻结断言。
- 不开放跨工厂 `durable` 声明（独立能力）。
