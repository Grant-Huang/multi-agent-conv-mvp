import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TaskRecord } from "./bindings/TaskRecord";
import { TaskPanel, taskMessageIds } from "./components/TaskPanel";
import { api } from "./api";

vi.mock("./api", () => ({
  api: {
    task: vi.fn(async () => ({ task: null, events: [] })),
    feedback: vi.fn(async () => ({ status: "success", data: { id: "fb-1" }, message: "" })),
  },
}));

const task = {
  id: "t1",
  conversation_id: "c1",
  goal: "写个函数",
  role: "coding",
  status: "COMPLETED",
  route: { task_class: "L2", execution_role: "coding", required_tools: [], risk_level: "low", reason: "代码" },
  policy: { auto_execute: true, needs_review: false, notes: [] },
  result: null,
  review: null,
  created_at: "2026-11-05T00:00:00Z",
  updated_at: "2026-11-05T00:00:00Z",
} as TaskRecord;

describe("派错了", () => {
  it("把正确动作记到这条用户消息上", async () => {
    render(<TaskPanel tasks={[task]} incoming={null} onChanged={() => {}} messageIds={{ t1: "m1" }} />);
    fireEvent.click(screen.getByText("写个函数"));
    fireEvent.click(await screen.findByText("派错了"));
    fireEvent.click(screen.getByText("保存"));
    await screen.findByText("已记下");
    expect(api.feedback).toHaveBeenCalledWith({
      message_id: "m1",
      original_action: "delegate",
      original_role: "coding",
      correct_action: "respond",
      correct_role: "",
      note: "",
    });
  });

  it("taskMessageIds：无 task_id 时按 goal 匹配用户消息", () => {
    const ids = taskMessageIds(
      [
        { id: "m-local", role: "user", content: "写个函数", task_id: null },
        { id: "a1", role: "assistant", content: "明白", task_id: "t1" },
      ],
      [{ id: "t1", goal: "写个函数" }],
    );
    expect(ids).toEqual({ t1: "m-local" });
  });

  it("taskMessageIds：已有 task_id 时优先用它", () => {
    const ids = taskMessageIds(
      [
        { id: "m1", role: "user", content: "写个函数", task_id: "t1" },
        { id: "m2", role: "user", content: "写个函数", task_id: null },
      ],
      [{ id: "t1", goal: "写个函数" }],
    );
    expect(ids).toEqual({ t1: "m1" });
  });
});
