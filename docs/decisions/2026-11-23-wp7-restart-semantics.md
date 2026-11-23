# WP7 重启语义：明确失败 + 待批准保持（不做断点续跑）

| 字段 | 内容 |
|---|---|
| 状态 | 有效 |
| 日期 | 2026-11-23 |
| 来源 | 用户批准（对话中批准方案 A 与交付文案；第 4 线 T-81，方案见当日对话，本文件为决策记录） |
| 关联 | G08、GS6 重启分支、E2E-RESTART、RT-RESTART-01～06；WP2（#215）、`desktop_replay.rs::rt_adreplay_04`；INTEGRATION_PLAN WP7 |

## 背景

启动时 `fail_interrupted_tasks` 把 CREATED/QUEUED/RUNNING/VERIFYING 一律置 FAILED，但只改状态：无任务事件、无失败交付，用户在时间线上看不到原因（宪章原则 3/5：出错就报错、每一步可追查）。`NEEDS_HUMAN`（待批准/待验收）不在重置列表内，且 `approve`/`human_review_verdict` 为 store 驱动，重启后可用，但没有用例锁定。

## 决策

1. **不做断点续跑**：系统没有步骤级执行状态，「续跑」只能从头重跑，会对已产生部分副作用的任务（coding 改文件、git 快照、外发检索）重复执行，违反宪章第 7 条（有后果的动作由固定代码执行并要人确认）。
2. **方案 A（用户批准）**：启动恢复时对每个中断任务 ① 置 FAILED（沿用原语义）；② 新增含「重启中断」原因的 StatusChanged 事件（同步落库，启动期无 SSE 消费者，不广播）；③ 向发起会话写绑定 `task_id` 的失败交付「任务因服务重启中断，可重新委派」（对齐 M0 问题 1「失败交付说明真实原因」）。
3. **落点**：`store.rs::recover_interrupted_tasks`（替代 `fail_interrupted_tasks`，返回受影响任务；调用方仅 `lib.rs::assemble`）+ `lib.rs::assemble` 恢复循环。`orchestrator.rs` 不改（判定/审批链已 store 驱动，由 RT-RESTART-03～05 基线锁定）。
4. **QUEUED 不自动重排**：与运行中任务一并明确失败（用户批准的方案 A）；若后续需要「未起跑任务自动重排」，另出 orchestrator 启动驱动方案卡（方案 C）。

## 用例（TDD Red→Green 证据见 PR）

- RT-RESTART-01/02：新行为（无原因事件/无交付为 Red）。
- RT-RESTART-03～06：既有跨重启出口与终态保护的基线锁定（合入前已绿）。
- E2E-RESTART：浏览器 GS6 重启分支（执行中重启 → FAILED + 失败交付 + 重连原会话；待验收重启 → 判定通过 → COMPLETED）。

## 非目标

断点续跑/步骤级恢复；QUEUED 自动重排；公开 HTTP API 变化；`acp.rs`/`util.rs`；新表新列（事件复用现有表）。

## 回滚

单 PR revert：`recover_interrupted_tasks` 还原为仅置 FAILED 的旧语义，事件与交付为追加写，无 DDL、无脏状态。
