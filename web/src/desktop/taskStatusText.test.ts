// UXFIX-E（#303，INTENT-2026-009）：状态词映射表锁定——与首页卡片副标题（服务端
// snapshot_status_subtitle）逐字一致；改词必须先改设计文档并同步两侧。
import { describe, expect, it } from "vitest";
import { taskStatusSubtitle } from "./taskStatusText";

describe("taskStatusSubtitle", () => {
  it("与卡片副标题映射表逐字一致", () => {
    const expected: Array<[string, string]> = [
      ["CREATED", "已创建"],
      ["QUEUED", "排队中 · Agent 工作"],
      ["RUNNING", "Agent 执行中"],
      ["VERIFYING", "待核验证据 · 查看工作区"],
      ["NEEDS_HUMAN", "待人工处理"],
      ["COMPLETED", "已完成"],
      ["FAILED", "失败 · 可重试"],
      ["CANCELLED", "已取消"],
    ];
    for (const [status, word] of expected) {
      expect(taskStatusSubtitle(status as never)).toBe(word);
    }
  });
});
