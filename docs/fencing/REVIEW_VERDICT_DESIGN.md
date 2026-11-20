# 审查进待人工：人工判定通过 / 不通过（设计）

对应 `docs/DESIGN.md` 已知限制 #4。  
**本文件只定方案与验收；不修改 `orchestrator.rs` / `store.rs`（AGENTS.md 高风险，须你批准后再实现）。**

## 问题

任务因 Reviewer 结论「需要人工」或审查失败进入 `NEEDS_HUMAN` 且**已有** `result` 时，界面只有取消，没有「人工判定通过 / 不通过」。

## 定了什么（待批准后实现）

| 项 | 内容 |
|---|---|
| API | `POST /api/tasks/{id}/review-verdict` body：`{"verdict":"accept"\|"reject","note":"..."}` |
| accept | 任务 → `COMPLETED`；记录人工验收事件；Composer 按完成交付 |
| reject | 若仍有返工额度 → 带 note 返工；否则 → `FAILED` |
| 鉴权 | 本机 MVP 仍无登录；与现有 approve 同级信任边界 |
| 幂等 | 终态任务返回 409 |
| UI | 任务卡在「有 result 的 NEEDS_HUMAN」显示「判定通过」「判定不通过」 |

## 非目标

- 不改自动质检通过/失败路径
- 不引入多用户权限
- 不把 Reviewer Agent 的意见当成最终状态

## 验收用例（实现前写入 CASES）

| ID | Given | When | Then |
|---|---|---|---|
| RT-REV-01 | 审查进入 NEEDS_HUMAN 且有 result | accept | COMPLETED；事件含人工 note |
| RT-REV-02 | 同上，仍有返工轮次 | reject | 回到执行/VERIFYING，带问题清单 |
| RT-REV-03 | 返工用尽 | reject | FAILED |
| RT-REV-04 | 已是 COMPLETED | accept/reject | 409，状态不变 |
| E2E-REV-01 | 浏览器任务卡 | 点判定通过 | 状态与交付文案正确 |

## 出问题先看

实现 PR 必须先附本设计批准记录（DECISIONS）与 Red 测试，再改业务代码。
