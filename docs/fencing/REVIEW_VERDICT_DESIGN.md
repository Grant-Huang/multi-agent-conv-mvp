# 审查进待人工：人工判定通过 / 不通过（设计摘要）

对应 `docs/DESIGN.md` 已知限制 #4；缺口 G04 / 黄金场景 GS3。

**完整实施方按（迁移、回滚、拒绝路径、唯一负责人）见 [WP2_IMPLEMENTATION_PLAN.md](WP2_IMPLEMENTATION_PLAN.md)。**  
决策：[2026-11-22-wp2-review-acceptance-plan](../decisions/2026-11-22-wp2-review-acceptance-plan.md)（有效；T-69 实现）。

## 问题

任务因 Reviewer 结论「需要人工」或审查失败进入 `NEEDS_HUMAN` 且**已有** `result` 时，界面只有取消，没有「人工判定通过 / 不通过」。与执行前「待批准」（无 result）共用状态字面量，文案易混。

## 定了什么

| 项 | 内容 |
|---|---|
| 等待原因（T1） | `awaiting_approval` = `NEEDS_HUMAN` 且无 result；`awaiting_acceptance` = `NEEDS_HUMAN` 且有 result（不新增枚举） |
| API | `POST /api/tasks/{id}/review-verdict` body：`{"verdict":"accept"\|"reject","note":"..."}` |
| accept | 任务 → `COMPLETED`；记录人工验收事件；Composer 按完成交付 |
| reject | 若仍有返工额度 → 带 note 返工；否则 → `FAILED` |
| 鉴权 | 本机 MVP 仍无登录；与现有 approve 同级信任边界 |
| 幂等 | 终态或不满足待验收条件 → 409 |
| UI | 待验收显示「判定通过」「判定不通过」；文案「待验收」vs「待批准」 |

## 非目标

- 不改自动质检通过/失败路径
- 不引入多用户权限
- 不把 Reviewer Agent 的意见当成最终状态
- 不在本方案卡改业务代码

## 验收用例（已写入 CASES）

| ID | Given | When | Then |
|---|---|---|---|
| RT-REV-01 | 审查进入 NEEDS_HUMAN 且有 result | accept | COMPLETED；事件含人工 note |
| RT-REV-02 | 同上，仍有返工轮次 | reject | 回到执行/VERIFYING，带问题清单 |
| RT-REV-03 | 返工用尽 | reject | FAILED |
| RT-REV-04 | 已是 COMPLETED | accept/reject | 409，状态不变 |
| E2E-REV | 浏览器任务卡 | 点判定通过 | 状态与交付文案正确 |

## 出问题先看

实现 PR 必须先附本设计对应决策的**用户确认**与 Red 测试，再改业务代码。细节以 [WP2_IMPLEMENTATION_PLAN.md](WP2_IMPLEMENTATION_PLAN.md) 为准。
