// 界面文案的唯一来源：状态、角色的中文显示名。
// 对话区和任务面板都从这里取文字，避免同一状态在两处叫法不同。
import type { Role } from "./bindings/Role";
import type { TaskRecord } from "./bindings/TaskRecord";
import type { TaskStatus } from "./bindings/TaskStatus";

/** 任务状态的显示名（面向用户：NEEDS_HUMAN 的动作是"批准"，所以叫"待批准"） */
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
export function taskStatusLine(task: Pick<TaskRecord, "status" | "role">, rank: number | null): string {
  switch (task.status) {
    case "QUEUED":
      return rank ? `排队中（第 ${rank} 位）` : "排队中";
    case "RUNNING":
      return `${roleLabel(task.role)}执行中`;
    default:
      return STATUS_LABEL[task.status];
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
