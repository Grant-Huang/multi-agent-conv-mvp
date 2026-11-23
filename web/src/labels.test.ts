import { describe, expect, it } from "vitest";
import { roleLabel, STATUS_LABEL, statusLabel, taskStatusLine } from "./labels";
import type { TaskStatus } from "./bindings/TaskStatus";

describe("状态与角色文案", () => {
  it("每个任务状态都有中文显示名", () => {
    const all: TaskStatus[] = ["CREATED", "QUEUED", "RUNNING", "VERIFYING", "COMPLETED", "FAILED", "NEEDS_HUMAN", "CANCELLED"];
    for (const s of all) expect(STATUS_LABEL[s]).toMatch(/[一-龥]/);
  });

  it("角色显示中文，未知角色原样返回", () => {
    expect(roleLabel("coding")).toBe("编码");
    expect(roleLabel("mystery")).toBe("mystery");
  });

  it("NEEDS_HUMAN 按 result 区分待批准 / 待验收", () => {
    expect(statusLabel({ status: "NEEDS_HUMAN", result: null })).toBe("待批准");
    expect(
      statusLabel({
        status: "NEEDS_HUMAN",
        result: { status: "succeeded", summary: "ok", artifacts: [], evidence: [], errors: [] },
      }),
    ).toBe("待验收");
  });

  it("对话区状态行：执行中带角色名，排队带位置，待验收有 result", () => {
    expect(taskStatusLine({ status: "RUNNING", role: "coding", result: null }, null)).toBe("编码执行中");
    expect(taskStatusLine({ status: "QUEUED", role: "coding", result: null }, 2)).toBe("排队中（第 2 位）");
    expect(taskStatusLine({ status: "NEEDS_HUMAN", role: "general", result: null }, null)).toBe("待批准");
    expect(
      taskStatusLine(
        {
          status: "NEEDS_HUMAN",
          role: "general",
          result: { status: "succeeded", summary: "ok", artifacts: [], evidence: [], errors: [] },
        },
        null,
      ),
    ).toBe("待验收");
    expect(taskStatusLine({ status: "COMPLETED", role: "general", result: null }, null)).toBe("已完成");
  });
});
