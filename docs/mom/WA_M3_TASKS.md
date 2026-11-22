# World Adapter M3 执行任务卡

本文件是 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) **M3 生产只读**的执行版。硬约束仍见该文 §0（DES-32）；生产适配见 [WORLD_ADAPTER.md](WORLD_ADAPTER.md) §7；Mock 替换见 [WA_MOCK_REGISTRY.md](WA_MOCK_REGISTRY.md) WAM-02/03/07。M0～M2 已准出。

**准出（缺一不可）**：可绑定生产世界并只读观测（含来源与截至水位）；权限/跨范围拒绝可执行；断线或游标过期后续读无漏无重复；多来源延迟与 `as_of` 可辨；写能力在生产世界上声明关闭或仍走隔离 Mock/DES；全量 M0～M2 套件回归绿。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| M3-0 | 冻结用例草案与任务卡；同步 PLAN / Feature Map / 注册表 | **进行中**（本 PR） | M2 准出 |
| M3-1 | 正式 CASES + Red 空壳；生产读合同（高风险先方案） | 排队 | M3-0 |
| M3-2 | 绑定 / Observe / 水位服务层 Green | 排队 | M3-1 |
| M3-3 | 权限、跨范围、延迟与多源 | 排队 | M3-2 |
| M3-4 | 浏览器生产读场景 + M0～M2 全量回归 | 排队 | M3-3 |
| M3-5 | 证据、WAM-07 读路径状态、准出检查 | 排队 | M3-4 |

一张卡 ≈ 一个 PR。每卡先 Red 再 Green；断言冻结后不得弱化。

## 1. 拟正式 id（待确认后 M3-1 表格化）

设计项侧重 **生产只读**（WA-02 续读在生产源、WA-07 能力可见性、WAM-07 生产来源读替换）。**WA-06 生产写确认属 M4**，本里程碑只保证写演示仍在隔离世界。

| 设计 | 拟 id | 要点 |
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
| 回归 | `wa-m0/m1/m2-suite` + 既有 RT-WA/WADES/DES/DESC | M3 合入前全绿 |

## 2. M3-0：冻结与收尾（本卡）

1. 本任务卡入库；PLAN 勾选已合入的 M2-6 / **M2**；增加 M3-0 入口。
2. CASES 增加 M3 **草案 bullet**（不写表格行，避免 check-map 缺测试）。
3. Feature Map / COVERAGE / `WA_MOCK_REGISTRY` 指向 M3 生产读（WAM-07 读）。
4. 证据 `docs/testing/evidence/wa-m3-0.md`；`npm run test:map` 绿。

## 3. M3-1～M3-5：纲要

- **优先服务层**：`/api/world/prod`（或扩展既有世界读合同，字段对齐 `/api/world/des` 的 Observe/Subscribe）；生产 stub 先于真站点。
- **高风险**：若需改 `engine.rs`、`store` 表结构、对外凭据/egress、或生产身份映射，**先出方案等人批准**。
- 故障与延迟测试控件不得在生产配置默认打开。
- 写路径：生产世界不开放 MES 写；演示写继续 Mock/DES。

## 4. 不做（M3）

- 不做真实 MES 写 / WA-06 生产确认（M4）。
- 不退役 `/api/world/mock` 与 `/api/world/des`。
- 不弱化 M0～M2 已冻结断言。
- 不开放跨工厂 `durable` 声明（独立能力）。
