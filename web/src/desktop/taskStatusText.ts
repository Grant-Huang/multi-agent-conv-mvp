import type { TaskStatus } from "../bindings/TaskStatus";

/** 任务状态词映射表（UXFIX-E / INTENT-2026-009：同一任务在卡片、工作区、受理提示状态一致）。
 *  权威表见 docs/ai-desktop/DESIGN.md 第 2 节；本表与
 *  `crates/server/src/desktop_api.rs` 的 `snapshot_status_subtitle`（首页卡片副标题）
 *  逐字一致，两侧任一侧改动须同步并更新设计文档。 */
export function taskStatusSubtitle(status: TaskStatus): string {
  switch (status) {
    case "CREATED":
      return "已创建";
    case "QUEUED":
      return "排队中 · Agent 工作";
    case "RUNNING":
      return "Agent 执行中";
    case "VERIFYING":
      return "待核验证据 · 查看工作区";
    case "NEEDS_HUMAN":
      return "待人工处理";
    case "COMPLETED":
      return "已完成";
    case "FAILED":
      return "失败 · 可重试";
    case "CANCELLED":
      return "已取消";
  }
}
