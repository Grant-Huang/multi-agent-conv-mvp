import { beforeEach, describe, expect, it, vi } from "vitest";
import { durableHomeAccept, waitForConversationTask } from "./homeDelegate";
import { api } from "../api";

vi.mock("../api", () => ({
  api: {
    createConversation: vi.fn(),
    desktopAcceptCreate: vi.fn(),
    desktopAcceptGet: vi.fn(),
    tasks: vi.fn(),
  },
}));

describe("durableHomeAccept", () => {
  beforeEach(() => {
    vi.mocked(api.createConversation).mockResolvedValue({
      id: "conv-1",
      title: "t",
      created_at: "",
    });
    vi.mocked(api.desktopAcceptCreate).mockResolvedValue({
      ok: true,
      status: 202,
      body: {
        request_id: "dreq-1",
        message_id: "msg-1",
        status: "accepted",
        dispatch_count: 1,
        replayed: false,
      },
    });
    vi.mocked(api.desktopAcceptGet).mockResolvedValue({
      request_id: "dreq-1",
      message_id: "msg-1",
      conversation_id: "conv-1",
      client_operation_id: "op-1",
      text: "调查",
      scope: "continue",
      task_id: null,
      status: "accepted",
      dispatch_count: 1,
    });
  });

  it("POST 后 GET 确认，同 op 幂等", async () => {
    const r = await durableHomeAccept({ text: "调查", clientOperationId: "op-1" });
    expect(r.requestId).toBe("dreq-1");
    expect(r.dispatchCount).toBe(1);
    expect(api.desktopAcceptCreate).toHaveBeenCalledTimes(2);
    expect(api.desktopAcceptGet).toHaveBeenCalledWith("dreq-1");
  });
});

describe("waitForConversationTask", () => {
  it("等到 goal 匹配的任务", async () => {
    vi.mocked(api.tasks)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        {
          id: "task-9",
          conversation_id: "c",
          goal: "人工验收验证报告",
          role: "general",
          status: "NEEDS_HUMAN",
          route: {
            task_class: "L2",
            execution_role: "general",
            required_tools: [],
            risk_level: "medium",
            reason: "t",
          },
          policy: { auto_execute: true, needs_review: true, notes: [] },
          result: null,
          review: null,
          created_at: "",
          updated_at: "",
        },
      ]);
    const id = await waitForConversationTask("c", "人工验收", 5_000);
    expect(id).toBe("task-9");
  });
});
