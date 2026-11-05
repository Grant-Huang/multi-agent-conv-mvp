import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskEvent } from "./bindings/TaskEvent";
import { ChatView } from "./components/ChatView";
import {
  BURST_REQUEST_LIMIT,
  createDeltaBuffer,
  createEventPump,
  createMarkdownGate,
  MARKDOWN_MIN_INTERVAL_MS,
  planFor,
  REFRESH_INTERVAL_MS,
  refreshStats,
  useIncomingDetail,
} from "./refresh";

const parses = vi.hoisted(() => ({ n: 0 }));

vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => {
    parses.n += 1;
    return <div data-testid="md">{children}</div>;
  },
}));

function progress(i: number): TaskEvent {
  return {
    id: `e${i}`,
    conversation_id: "c",
    task_id: "task-1",
    kind: "progress",
    status: null,
    message: `第 ${i} 行`,
    stream_id: null,
    created_at: "2026-01-01T00:00:00Z",
  };
}

function Probe({ incoming }: { incoming: TaskEvent | null }) {
  useIncomingDetail(true, "task-1", incoming);
  return null;
}

describe("刷新节流", () => {
  const urls: string[] = [];

  beforeEach(() => {
    urls.length = 0;
    refreshStats.fired = 0;
    refreshStats.dropped = 0;
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo) => {
        urls.push(String(input));
        return {
          ok: true,
          json: async () => ({
            task: { id: "task-1" },
            events: [{ id: "seed", conversation_id: "c", task_id: "task-1", kind: "progress", status: null, message: "", stream_id: null, created_at: "2026-01-01T00:00:00Z" }],
          }),
        };
      }),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("连发 50 个进度事件，任务接口请求不超过节流上限，而且不拉任务列表", async () => {
    expect(planFor("progress").tasks).toBe(false);
    const { rerender } = render(<Probe incoming={null} />);
    await act(async () => {
      await Promise.resolve();
    });
    const baseline = urls.length;

    for (let i = 0; i < 50; i++) rerender(<Probe incoming={progress(i)} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(REFRESH_INTERVAL_MS);
    });

    const added = urls.slice(baseline).filter((u) => u.includes("/api/tasks"));
    expect(added.length).toBe(BURST_REQUEST_LIMIT);
    expect(added.some((u) => u.includes("conversation_id"))).toBe(false);
    expect(added.every((u) => u.includes("after="))).toBe(true);
  });

  it("事件泵把 50 条进度收成一次增量拉取", () => {
    const queued: Array<() => void> = [];
    let tasks = 0;
    let detail = 0;
    let live = 0;
    const pump = createEventPump({
      intervalMs: REFRESH_INTERVAL_MS,
      clock: { now: () => 0, schedule: (fn) => queued.push(fn) },
      onTasks: () => {
        tasks += 1;
      },
      onMessages: () => {},
      onPanels: () => {},
      onLive: () => {
        live += 1;
      },
      onDetail: () => {
        detail += 1;
      },
      onDelta: () => {},
    });
    for (let i = 0; i < 50; i++) pump(progress(i));
    expect(tasks).toBe(0);
    expect(live).toBe(50);
    expect(queued.length).toBeLessThanOrEqual(BURST_REQUEST_LIMIT);
    queued.forEach((fn) => fn());
    expect(detail).toBeLessThanOrEqual(BURST_REQUEST_LIMIT);
  });
});

describe("流式渲染", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = () => {};
  });

  afterEach(() => {
    parses.n = 0;
  });

  it("100 个流式片段不解析 Markdown，结束后才解析一次", () => {
    const view = render(<ChatView messages={[]} onSend={async () => {}} streaming={{ id: "s", text: "x" }} />);
    let text = "x";
    for (let i = 0; i < 100; i++) {
      text += "字";
      view.rerender(<ChatView messages={[]} onSend={async () => {}} streaming={{ id: "s", text }} />);
    }
    expect(parses.n).toBe(0);
    expect(view.container.querySelector(".stream-plain")?.textContent).toBe(text);

    view.rerender(
      <ChatView
        messages={[{ id: "s", conversation_id: "c", role: "assistant", content: text, task_id: null, created_at: "2026-01-01T00:00:00Z" }]}
        onSend={async () => {}}
        streaming={null}
      />,
    );
    expect(parses.n).toBeGreaterThan(0);
    expect(parses.n).toBeLessThanOrEqual(4);
  });

  it("同一帧的片段合成一次更新，Markdown 门闩在间隔内不重复解析", () => {
    let flushed = "";
    let raf: (() => void) | null = null;
    const push = createDeltaBuffer(
      (chunk) => {
        flushed += chunk;
      },
      (cb) => {
        raf = cb;
      },
    );
    for (let i = 0; i < 100; i++) push("a");
    expect(flushed).toBe("");
    raf!();
    expect(flushed).toBe("a".repeat(100));

    let t = 0;
    const gate = createMarkdownGate(MARKDOWN_MIN_INTERVAL_MS, () => t);
    for (let i = 0; i < 100; i++) expect(gate.take(false)).toBe(false);
    expect(gate.parses).toBe(0);
    expect(gate.take(true)).toBe(true);
    expect(gate.parses).toBe(1);
  });
});
