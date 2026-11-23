# 首页委派终态结果可靠回传

| 字段 | 内容 |
|---|---|
| 状态 | 有效 |
| 日期 | 2026-11-23 |
| 来源 | 用户授权下一卡（#235 后 Line 4 近端余下；T-94） |
| 关联 | E2E-HOMEREV、E2E-HOMEREJ；contracts T1/S7；WP2 #215；#235 |

## 背景

#235 已把 S1 接到 B1 受理与 WP2「判定通过」。HOMEREV 仅浅断言助理气泡；拒绝/返工与「交付+证据 durable 回发起会话、重连可查」未作为日常入口验收。

## 决策

1. **不改** `orchestrator.rs` / `store.rs` / `registry.rs`：终态仍走现有 `human_review_verdict` → `Composer::compose` → `add_message(..., Some(task_id))`。
2. **通过路径**：加强 `E2E-HOMEREV`——断言 messages 中带 `task_id` 的助理交付、任务 `result` 产物/证据、刷新后再进同会话仍可查。
3. **拒绝路径**：新增 `E2E-HOMEREJ`——首次 reject→返工（QUEUED，同 conversation）；二次 reject（`max_review_rounds=1`）→FAILED + durable 失败交付；刷新可断言。
4. **返工中间态**：不新增 Composer 消息（与现网 WP2 一致，仅 progress/上下文）；若产品要求「退回当下也写交付气泡」，另开 orch 方案卡。

## 非目标

WP5 / WP7 / WP6 / WP1·WA / WP8。

## 回滚

还原 HOMEREV 浅断言；删除 HOMEREJ 与本决策。
