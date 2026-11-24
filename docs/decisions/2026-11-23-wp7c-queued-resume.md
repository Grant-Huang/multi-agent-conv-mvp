# WP7 方案 C：重启后重排未起跑的单任务（QUEUED/CREATED）

| 字段 | 内容 |
|---|---|
| 状态 | 有效 |
| 日期 | 2026-11-23 |
| 来源 | 用户在 T-85 设计卡对话中以「继续下一个任务」推进；三项设计决定（默认开关=true、v1 排除模板家族、卡号 T-85）按助手推荐项执行（用户未逐项表态，如实标注） |
| 关联 | [2026-11-23-wp7-restart-semantics](2026-11-23-wp7-restart-semantics.md)（方案 A/明确失败）；G08、GS6；INTEGRATION_PLAN WP7；RT-RESTART-07～11 |

## 背景

T-81（方案 A）把 CREATED/QUEUED/RUNNING/VERIFYING 一律失败。其中 CREATED/QUEUED 任务**从未起跑**（零副作用），失败会丢掉本可继续的工作；但模板家族（root/branch/join）重驱有家族一致性风险（join 兄弟未终态、root 失败后孤儿 branch）。

## 决策

1. **重排范围（v1）**：仅单任务（`fanout_kind IS NULL`）且状态 CREATED/QUEUED——代码确认该状态已携带完整 route/policy，重驱走既有 `spawn` 路径（与 approve-after-restart 同构，RT-RESTART-05 基线已证）。
2. **仍失败**：RUNNING/VERIFYING（可能有部分副作用，重跑=重复执行，宪章 7）；模板家族 CREATED/QUEUED（重驱排除后统一走 T-81 失败恢复：原因事件 + 失败交付）。
3. **开关**：`[orchestrator] resume_queued_on_restart`，默认 **true**；设 false 即恢复 T-81 全失败语义（免 revert 回滚）。
4. **实现落点**：`store.list_resumable_tasks()`（重排候选）+ `store.fail_unstarted_tasks(exclude_ids)`（未重排者失败恢复）+ `orchestrator.resume_queued_on_restart()`（重驱，返回已重排 id）+ `lib.rs::assemble`（orchestrator 构建后、serve 前：先重排取 id，再失败剩余者）。
5. **不做**：断点续跑（维持方案 A 决策）；模板家族级恢复（如需另卡，需先定义家族重建语义）；浏览器新增用例（GS6 重启分支已有 E2E-RESTART，QUEUED 窗口时序脆弱，合同层覆盖）。

## 用例（TDD Red→Green 见 PR）

RT-RESTART-07（QUEUED 重排）、08（CREATED 重排）为新行为；09（副作用态失败/排队态存活）、10（开关回滚）、11（模板家族不重驱）为守卫；01 断言随语义收窄改为仅 RUNNING/VERIFYING（需求变化，非改断言迎合实现）。

## 回滚

`resume_queued_on_restart=false` 配置即回 T-81 语义；或单 PR revert（无 DDL，事件追加写）。
