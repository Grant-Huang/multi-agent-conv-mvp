# WP2：审查需人工的验收出口方案

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-22 |
| 来源 | 用户授权 T-65 出方案；#210 合入 = ★方案批准；T-69 实现 |
| 状态 | **有效** |
| 影响范围 | 执行引擎任务状态与人工验收；`orchestrator.rs` / `store.rs`；前端任务文案 |
| 相关设计 | [WP2_IMPLEMENTATION_PLAN.md](../fencing/WP2_IMPLEMENTATION_PLAN.md)；[REVIEW_VERDICT_DESIGN.md](../fencing/REVIEW_VERDICT_DESIGN.md)；[INTEGRATION_PLAN.md](../INTEGRATION_PLAN.md) WP2 / GS3；契约 T1（[state-and-cancellation](../contracts/state-and-cancellation.md)）；决定 [2026-11-22-c2-m2-minimal-contracts](2026-11-22-c2-m2-minimal-contracts.md) D7 |

**一句话结论**：采纳「`NEEDS_HUMAN` + result 有无」区分待批准 / 待验收，新增 `review-verdict` API 与 UI 出口。

## 背景

G04：审查判需人工后任务带 result 停在 `NEEDS_HUMAN`，界面显示「待批准」却无验收按钮，违反「完成≠验收」与契约 S7。CASES 已冻结 RT-REV-01～04 / E2E-REV；`REVIEW_VERDICT_DESIGN` 有 API 摘要但未批准实施。

## 问题

若不先定状态拆分方式与负责人，实现会话可能直接改枚举或并行改 orchestrator，扩大回滚面并与 Desktop 映射冲突。

## 决策依据

1. 用户 T-65：只出方案 PR，禁止改业务代码；触 orchestrator/store 须方案批准（对话 2026-11-22；INTEGRATION_PLAN WP2）。
2. 契约 T1 已确认：待批准 / 待验收分状态**或等价显式等待原因**（决定 2026-11-22-c2-m2-minimal-contracts D7）。
3. 代码事实：`approve_task` 已要求 `NEEDS_HUMAN AND result_json IS NULL`；审查 needs_human 路径写入 result（`orchestrator.rs`）；UI 仅在无 result 时显示批准（`ChatView`/`TaskPanel`）。
4. 既有设计与冻结用例：[REVIEW_VERDICT_DESIGN.md](../fencing/REVIEW_VERDICT_DESIGN.md)；`docs/testing/CASES.md` RT-REV-*。

## 定了什么

- ★采用等待原因：`awaiting_approval` = NEEDS_HUMAN 无 result；`awaiting_acceptance` = NEEDS_HUMAN 有 result。
- API：`POST /api/tasks/{id}/review-verdict`；accept → COMPLETED；reject → 返工或 FAILED；终态 409。
- 唯一实施负责人：主线编排/语义 worker；实现卡独占 orchestrator/store 相关改动。
- 默认不加 DB 新列；回滚 = revert 实现 PR。

## 简单设计

见 [WP2_IMPLEMENTATION_PLAN.md](../fencing/WP2_IMPLEMENTATION_PLAN.md) §2～§6。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 方案卡改 orchestrator/store | T-65 禁止；实现另卡（T-69） |
| 新增 TaskStatus 枚举值 | 改动面大；T1 允许等价等待原因 |
| 验收并进 `/approve` | 与执行前批准混淆 |

## 影响与后续

- T-69 实现：Red RT-REV → Green → E2E-REV → DESIGN §15 #4 已解决。
- WP4 可在 WP2 实现合并后开工。

## 出问题先看

- `docs/fencing/WP2_IMPLEMENTATION_PLAN.md`
- `Store::approve_task` / `accept_human_review` / `reject_human_review_*`
- RT-REV-01～04

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-22 | 新建（待确认） | T-65 |
| 2026-11-22 | #210 合入 = ★批准；状态改为有效；T-69 实施 | T-69 |
