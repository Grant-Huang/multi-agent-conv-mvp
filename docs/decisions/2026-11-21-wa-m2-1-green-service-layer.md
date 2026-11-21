# World Adapter M2-1 Green：服务层接线（待批）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 助手建议 |
| 状态 | 待用户确认 |
| 影响范围 | World Adapter（M2 写路径）；`mom_world_des`；相关 CASES / Playwright |
| 相关设计 | [WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §4.3～4.5、§6.2～6.3；[WA_M2_TASKS.md](../mom/WA_M2_TASKS.md)；[WA_M2_1_PLAN.md](../mom/WA_M2_1_PLAN.md)；共享契约草案 [state-and-cancellation.md](../contracts/state-and-cancellation.md) S3/S6、[identity-and-approval.md](../contracts/identity-and-approval.md) A1～A3 |

**一句话结论**：M2 Green 默认只在服务层挂 `/api/world/des` 的 Propose/Execute/Confirm；不改 engine / DesConnector 内部 / orchestrator，除非升级方案再批。M2-1 本卡只到 Red。

## 背景

M1 + T-50 已把远程 DES **读**路径接到 `/api/world/des`（bind/observe/subscribe/trial/official/step）。写路径仍只在 `/api/world/mock`。M2-0（#194）冻结了 `E2E-WAEXEC`/`WARECV`/`WADYN`/`WALOOP` 与 `RT-WADES-09`～`14` 草案。M2-1 要把草案落成正式表格与 Red 空壳；Green 可能触及高风险模块，须先方案。

## 问题

若不先限定 Green 改动面，实现会话可能直接改 `engine.rs` 提交窗口或 `DesConnector` 派发/回执语义，扩大 L2/L3 面且难回滚。

## 决策依据

1. 用户任务卡授权：T-53「World Adapter M2-1 Red 空壳」要求只到 Red、高风险路径先方案（对话 2026-11-21；任务卡 [WA_M2_TASKS.md](../mom/WA_M2_TASKS.md) M2-1）。
2. 代码事实：`mom_world_des.rs` 路由表仅有读与实验控制，无 propose/execute/confirm（本地 Red：对写路径返回 404，见 [wa-m2-1-red.md](../testing/evidence/wa-m2-1-red.md)）。
3. 先例：M1-1 Green 方案同为「服务层优先、默认不改 engine/DesConnector」（[WA_M1_1_GREEN_PLAN.md](../mom/WA_M1_1_GREEN_PLAN.md)；#178）。
4. 契约对齐（草案，引用不重新定义）：取消后未发不发、迟到回执不改终态（S3）；无法判定时 `unknown`（S6）；批准绑定版本与幂等（A1～A3）。
5. **推断**：`DesConnector::dispatch_writes` 公共 API 足以支撑首版写前门；若不足再升级方案（标推断，待 Green 验证）。

## 定了什么

- Green 默认只在 `mom_world_des`（及必要的 `main` merge / 测试控件）接线；调用既有 MOM 审批/执行与 `DesConnector::dispatch_writes` 公共 API。
- **不改** `engine.rs` / `orchestrator.rs` / `registry.rs` / `DesConnector` 内部，除非升级方案再批。
- M2-1 本卡只到 Red（CASES 表格 + Playwright 空壳 + 本地 Rust Red 证据）；Rust 失败测试不入库以免 CI 红。

## 简单设计

见 [WA_M2_1_PLAN.md](../mom/WA_M2_1_PLAN.md)：在已有 `/api/world/des` 上增 propose/execute/confirm 与 lose/restore/factory-restart 测试控件；字段对齐 Mock `RT-WA-04/06`，`provenance=des`。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 本卡直接改引擎提交窗口或 DesConnector 超时/锁 | 高风险，须单独批准 |
| 本卡写 Green | T-53 范围只到 Red |
| 往 `DECISIONS.md` 追加编号条目 | 新决策一条一个文件（决定 2026-11-21-decision-format） |

## 影响与后续

- 用户批准本条后，开 Green 卡按 `WA_M2_1_PLAN.md` 实现。
- 不在本卡创建 `docs/features/world-adapter/` 特性档案（WA 试点迁移须用户统一安排）。

## 出问题先看

- [`WA_M2_1_PLAN.md`](../mom/WA_M2_1_PLAN.md)
- [`wa-m2-1-red.md`](../testing/evidence/wa-m2-1-red.md)
- `crates/server/src/mom_world_des.rs` 路由表

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-21 | 新建（按决策模板补齐依据） | T-53 / M2-1 Red |
