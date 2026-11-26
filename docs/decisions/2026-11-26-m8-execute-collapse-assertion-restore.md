# 恢复「执行区收起状态重进保持」E2E 断言（M8）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 来源 | 用户确认（本会话明确拍板「恢复断言」，2026-11-26） |
| 状态 | 有效 |
| 影响范围 | ai-desktop 工作区执行区、E2E-ADREAD/desktop.spec、CASES.md |
| 相关设计 | `docs/CASES.md:785`；评审 `docs/review/2026-11-26.md` R-20261126-M8；#349/#350 ADREAD 锚点修复 |

## 一句话结论

被 #349/#350（ADREAD 锚点修复）重构删掉的「工作区收起执行后离开再打开，执行区保持收起」E2E 断言予以恢复，放到能播种 SMT-03 卡的套件（desktop.spec）。

## 背景

ADREAD 修复重构 ops.spec 时删除了上述断言；此后「执行区收起状态重进保持」行为无任何测试覆盖。评审 2026-11-26 将「恢复还是从 CASES 删去」列为需要人决定的事第 2 条，推荐恢复。

## 问题

用户可见的界面状态行为（收起偏好跨离开/重进保持）失去回归保护，后续重构可能无声破坏。

## 决策依据

1. 用户确认（2026-11-26，二选一明确选「恢复断言」）。
2. 评审核证：该断言删除后无等价覆盖（`docs/review/2026-11-26.md` 交接清单第 3 条）。

## 定了什么

1. 恢复断言：重进后 `workspace-exec` 计数为 0、`workspace-exec-toggle` 的 aria-expanded=false。
2. 放 desktop.spec（该套件能播种 SMT-03 卡；ops 实例缺桌面任务）。
3. 派工卡 [#366](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/366)（第 1 线）随之解锁；CASES.md:785 同步。

## 没选什么

从 CASES 删去该行为断言（PR 须附「测试变更说明」）——用户未选。

## 影响与后续

第 1 线按卡修复；AR-13 复验；test:map 欠账数随用例恢复下降。

## 出问题先看

desktop.spec 新用例；`workspace-exec` / `workspace-exec-toggle` 定位器；SMT-03 播种夹具。

## 变更记录

- 2026-11-26 新建。
