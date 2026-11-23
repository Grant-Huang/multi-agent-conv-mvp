// 界面文案的唯一来源：状态、角色的中文显示名。
// 对话区和任务面板都从这里取文字，避免同一状态在两处叫法不同。
import type { Role } from "./bindings/Role";
import type { TaskRecord } from "./bindings/TaskRecord";
import type { TaskStatus } from "./bindings/TaskStatus";

/** 任务状态的显示名（NEEDS_HUMAN 默认按「待批准」；有 result 时用 statusLabel） */
export const STATUS_LABEL: Record<TaskStatus, string> = {
  CREATED: "已创建",
  QUEUED: "排队中",
  RUNNING: "执行中",
  VERIFYING: "审查中",
  COMPLETED: "已完成",
  FAILED: "失败",
  NEEDS_HUMAN: "待批准",
  CANCELLED: "已取消",
};

/**
 * 按等待原因区分 NEEDS_HUMAN：无 result → 待批准；有 result → 待验收（WP2 / T1）。
 */
export function statusLabel(task: Pick<TaskRecord, "status" | "result">): string {
  if (task.status === "NEEDS_HUMAN") {
    return task.result ? "待验收" : "待批准";
  }
  return STATUS_LABEL[task.status];
}

/** 角色的显示名。内部值（general / coding / reviewer）只用于 API 和日志 */
export const ROLE_LABEL: Record<Role, string> = {
  general: "通用",
  coding: "编码",
  reviewer: "审查",
};

/** 角色显示名；遇到未知值时原样返回，避免界面空白 */
export function roleLabel(role: string): string {
  return (ROLE_LABEL as Record<string, string>)[role] ?? role;
}

/**
 * 对话区任务状态行：带上角色名和排队位置。
 * rank 为排队位置（从 1 开始），非排队状态或未知时传 null。
 */
export function taskStatusLine(
  task: Pick<TaskRecord, "status" | "role" | "result" | "fanout_kind" | "template_id" | "branch_id">,
  rank: number | null,
): string {
  switch (task.status) {
    case "QUEUED":
      return rank ? `排队中（第 ${rank} 位）` : "排队中";
    case "RUNNING":
      if (task.fanout_kind === "root") {
        const tid = task.template_id?.trim();
        return tid ? `模板 ${tid} 并行执行中` : "模板并行执行中";
      }
      if (task.fanout_kind === "branch" && task.branch_id) {
        return `分支 ${task.branch_id} 执行中`;
      }
      if (task.fanout_kind === "join") {
        return "汇合中";
      }
      return `${roleLabel(task.role)}执行中`;
    default:
      return statusLabel(task);
  }
}

/** 任务卡标题旁的模板/分支角标文案；非模板任务返回空 */
export function fanoutLabel(
  task: Pick<TaskRecord, "fanout_kind" | "template_id" | "branch_id">,
): string {
  switch (task.fanout_kind) {
    case "root":
      return task.template_id ? `模板·${task.template_id}` : "模板";
    case "branch":
      return task.branch_id ? `分支·${task.branch_id}` : "分支";
    case "join":
      return "汇合";
    default:
      return "";
  }
}

/**
 * 执行者在执行组中的位置："主力" / "备份 N"；组内只有一个成员时注明没有备份。
 * member_index 从 0 开始，0 是主力。
 */
export function memberLabel(memberIndex: number, groupSize: number): string {
  if (memberIndex === 0) return groupSize > 1 ? `主力（共 ${groupSize} 个）` : "主力（无备份）";
  return `备份 ${memberIndex}（共 ${groupSize} 个）`;
}

/** 排队时长：不足 0.1 秒视为没有排队，返回空串 */
export function waitedLabel(ms: number): string {
  return ms < 100 ? "" : `排队 ${(ms / 1000).toFixed(1)}s`;
}
