# World Adapter M3-1 Green：生产只读服务层接线

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-22 |
| 来源 | 助手建议（待用户确认；T-67 / M3-1 要求高风险先方案） |
| 状态 | 待用户确认（本卡仍只到 Red；Green 默认路径建议） |
| 影响范围 | World Adapter（M3 生产只读）；拟 `mom_world_prod`；相关 CASES / Playwright |
| 相关设计 | [WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §3、§4.1～4.2、§7；[WA_M3_TASKS.md](../mom/WA_M3_TASKS.md)；[WA_M3_1_PLAN.md](../mom/WA_M3_1_PLAN.md) |

**一句话结论**：M3 Green 默认只在服务层挂 `/api/world/prod` 的 Bind/Observe/Subscribe（生产 stub 先于真站点）；不改 engine / store 表结构 / orchestrator / 对外凭据与 egress，除非升级方案再批。M3-1 本卡只到 Red。

## 背景

M0～M2 已准出 Mock 与远程 DES 读/写。M3-0（#209）冻结了 `E2E-WAPROD`/`WAPISO`/`WAPREAD`/`WAPDELAY` 与 `RT-WAPROD-01`～`06` 草案。M3-1 要把草案落成正式表格与 Red 空壳；Green 可能触及高风险模块，须先方案。

## 问题

若不先限定 Green 改动面，实现会话可能直接改 `store` 表结构、接真实 MES 凭据/egress，或改 `engine` 绑定语义，扩大 L2/L3 面且难回滚。

## 决策依据

1. 用户任务卡授权：T-67「World Adapter M3-1 生产只读落地」要求按 [WA_M3_TASKS.md](../mom/WA_M3_TASKS.md) M3-1 卡执行、遵守 AGENTS、高风险路径先方案（对话 2026-11-22）。
2. 任务卡原文：优先服务层 `/api/world/prod`（字段对齐 `/api/world/des`）；生产 stub 先于真站点；故障控件不得在生产配置默认打开。
3. 代码事实：仓库无 `/api/world/prod` 路由；工作台种子无生产世界（见 `momWorkbenchData.ts`）。
4. 先例：M1-1 / M2-1 Green 均为「服务层优先、默认不改 engine/DesConnector」（[WA_M2_1_PLAN.md](../mom/WA_M2_1_PLAN.md)；决定 2026-11-21-wa-m2-1-green-service-layer）。
5. **推断**：内存 stub + 配置登记足以支撑首版生产只读合同；真站点接入另开升级方案（标推断，待 Green 验证）。

## 定了什么

- Green 默认只在服务层新增 `/api/world/prod`（及必要的 `main` merge / 测试控件 / 工作台种子）；`provenance=production`。
- **不改** `engine.rs` / `orchestrator.rs` / `registry.rs` / `store` 表结构 / DesConnector 内部，除非升级方案再批。
- **不接** 真实 MES 凭据与 egress；生产写属 M4。
- M3-1 本卡只到 Red（CASES 表格 + Playwright 空壳 + 本地 Rust Red 证据）；Rust 失败测试不入库以免 CI 红。

## 简单设计

见 [WA_M3_1_PLAN.md](../mom/WA_M3_1_PLAN.md)。

## 没选什么

- 扩展 `/api/world/des` 承载生产读：混淆 provenance 与适配器边界。
- 首卡即接真实站点：需凭据/egress/身份映射，超出 M3-1/默认 Green 风险面。

## 影响与后续

- 本 PR：表格、空壳、方案、证据；`runtime_cases` 等 M3-2+ 入库。
- 用户确认本方案后，M3-2 起按服务层路径 Green。

## 出问题先看

- Red 证据：`docs/testing/evidence/wa-m3-1-red.md`
- 任务卡：`docs/mom/WA_M3_TASKS.md` M3-1
- 路由是否误挂在 des/mock 前缀

## 变更记录

| 日期 | 说明 |
|---|---|
| 2026-11-22 | 新建（助手建议；待用户确认） |
