// 界面改进的行为测试：对话内批准、两步确认、进度条、示例点击、失败原因。
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Message } from "./bindings/Message";
import type { TaskRecord } from "./bindings/TaskRecord";
import { ChatView } from "./components/ChatView";
import { ConfirmButton } from "./components/ConfirmButton";
import { TaskTicker } from "./components/TaskTicker";

// jsdom 没有 scrollIntoView；对话区滚动到底部只是视觉行为，这里用空实现替代
beforeAll(() => {
  Element.prototype.scrollIntoView = () => {};
});

// 未开启 vitest globals，RTL 不会自动卸载组件；每个用例后手动清理，避免用例之间互相干扰
afterEach(() => cleanup());

function task(over: Partial<TaskRecord>): TaskRecord {
  return {
    id: "t1",
    conversation_id: "c1",
    goal: "写个函数",
    role: "coding",
    status: "NEEDS_HUMAN",
    route: { task_class: "L2", execution_role: "coding", required_tools: [], risk_level: "low", reason: "代码" },
    policy: { auto_execute: false, needs_review: false, notes: [] },
    result: null,
    review: null,
    created_at: "2026-11-05T00:00:00Z",
    updated_at: "2026-11-05T00:00:00Z",
    ...over,
  } as TaskRecord;
}

const reply: Message = {
  id: "m1",
  conversation_id: "c1",
  role: "assistant",
  content: "已派给后台",
  task_id: "t1",
  created_at: "2026-11-05T00:00:00Z",
} as Message;

describe("对话内批准与取消", () => {
  it("待批准的任务直接在对话里显示批准按钮，并调用动作", () => {
    const onTaskAction = vi.fn(async () => {});
    render(
      <ChatView
        messages={[reply]}
        tasks={[task({})]}
        onSend={async () => {}}
        streaming={null}
        onTaskAction={onTaskAction}
      />,
    );
    expect(screen.getByText("待批准")).toBeTruthy();
    fireEvent.click(screen.getByText("批准执行"));
    expect(onTaskAction).toHaveBeenCalledWith("approve", "t1");
  });

  it("运行中的任务可以取消，取消需要二次确认", () => {
    const onTaskAction = vi.fn(async () => {});
    render(
      <ChatView
        messages={[reply]}
        tasks={[task({ status: "RUNNING" })]}
        onSend={async () => {}}
        streaming={null}
        onTaskAction={onTaskAction}
      />,
    );
    fireEvent.click(screen.getByText("取消任务"));
    expect(onTaskAction).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("确认取消"));
    expect(onTaskAction).toHaveBeenCalledWith("cancel", "t1");
  });

  it("待批准的任务只有一个取消按钮（回归：曾经同时显示两个）", () => {
    render(
      <ChatView messages={[reply]} tasks={[task({})]} onSend={async () => {}} streaming={null} onTaskAction={async () => {}} />,
    );
    expect(screen.getAllByText(/取消/)).toHaveLength(1);
  });

  it("失败的任务显示失败原因，且不显示批准按钮", () => {
    render(
      <ChatView
        messages={[reply]}
        tasks={[task({
          status: "FAILED",
          result: { status: "failed", summary: "", artifacts: [], evidence: [], errors: ["codex 超时"] } as unknown as TaskRecord["result"],
        })]}
        onSend={async () => {}}
        streaming={null}
        onTaskAction={async () => {}}
      />,
    );
    expect(screen.getByText("codex 超时")).toBeTruthy();
    expect(screen.queryByText("批准执行")).toBeNull();
  });
});

describe("两步确认按钮", () => {
  it("第一次点击只进入确认状态，第二次才执行", () => {
    const onConfirm = vi.fn();
    render(<ConfirmButton label="解除配对" confirmLabel="确认解除" onConfirm={onConfirm} />);
    fireEvent.click(screen.getByText("解除配对"));
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("确认解除"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

describe("进度条", () => {
  const lines = ["排队中", "执行中 1/3", "写入文件"];

  it("默认只显示最新一条", () => {
    render(<TaskTicker lines={lines} running={false} />);
    expect(screen.getByText("写入文件")).toBeTruthy();
    expect(screen.queryByText("排队中")).toBeNull();
  });

  // 需求变更（用户确认）：展开内容从旧到新列出，与执行顺序一致
  it("展开后显示全部历史，从旧到新", () => {
    render(<TaskTicker lines={lines} running={false} />);
    fireEvent.click(screen.getByTitle("展开全部进度"));
    const items = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(items).toEqual(["排队中", "执行中 1/3", "写入文件"]);
  });

  it("没有进度且未运行时不渲染", () => {
    const { container } = render(<TaskTicker lines={[]} running={false} />);
    expect(container.innerHTML).toBe("");
  });
});

describe("空会话示例", () => {
  it("点击示例会填入输入框", () => {
    render(<ChatView messages={[]} onSend={async () => {}} streaming={null} />);
    fireEvent.click(screen.getByText("你好，介绍一下你能做什么"));
    expect((screen.getByLabelText("输入消息") as HTMLTextAreaElement).value).toBe("你好，介绍一下你能做什么");
  });
});
