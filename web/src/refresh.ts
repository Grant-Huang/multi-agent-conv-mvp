// 事件刷新策略：进度事件不拉任务列表；诊断 / 路由日志只在状态变化时刷新；
// 同一类请求尾部节流（默认 1 秒最多一次）。流式文本按帧合并，Markdown 至少隔 250ms 才重新解析。
import { useEffect, useMemo, useRef, useState } from "react";
import type { EventKind } from "./bindings/EventKind";
import type { TaskEvent } from "./bindings/TaskEvent";
import { api, type TaskDetail } from "./api";

/** 同一类接口刷新的最小间隔 */
export const REFRESH_INTERVAL_MS = 1000;
/** 流式过程中重新解析 Markdown 的最小间隔 */
export const MARKDOWN_MIN_INTERVAL_MS = 250;
/** 同一时刻连发的事件，触发的接口请求上限（测试用这个数断言） */
export const BURST_REQUEST_LIMIT = 1;

export interface RefreshPlan {
  /** 拉任务列表。进度事件不算，避免一条命令输出打一次 /api/tasks */
  tasks: boolean;
  messages: boolean;
  /** 诊断页、路由日志。只有当前页签可见时调用方才真正发请求 */
  panels: boolean;
  /** 任务详情：本地追加这条事件，并节流地按 after 增量拉取 */
  detail: boolean;
}

export function planFor(kind: EventKind): RefreshPlan {
  if (kind === "assistant_delta") return { tasks: false, messages: false, panels: false, detail: false };
  if (kind === "progress") return { tasks: false, messages: false, panels: false, detail: true };
  if (kind === "status_changed") return { tasks: true, messages: false, panels: true, detail: true };
  return { tasks: true, messages: true, panels: true, detail: true };
}

/** 开发模式下诊断页显示：实际发出的刷新，以及被节流丢掉的次数 */
export const refreshStats = { fired: 0, dropped: 0 };

export interface Clock {
  now(): number;
  schedule(fn: () => void, ms: number): void;
}

export interface TrailingThrottle {
  push(fn: () => void): void;
}

/** 尾部节流：间隔内多次 push 只执行最后一次 */
export function createTrailingThrottle(intervalMs: number, clock?: Clock): TrailingThrottle {
  let pending: (() => void) | null = null;
  let timer = false;
  let lastFire = Number.NEGATIVE_INFINITY;
  const now = () => clock?.now() ?? Date.now();
  const schedule = (fn: () => void, ms: number) => {
    if (clock) clock.schedule(fn, ms);
    else setTimeout(fn, ms);
  };
  return {
    push(fn) {
      if (timer) refreshStats.dropped += 1;
      pending = fn;
      if (timer) return;
      timer = true;
      const wait = Math.max(0, intervalMs - (now() - lastFire));
      schedule(() => {
        timer = false;
        lastFire = now();
        const run = pending;
        pending = null;
        if (run) {
          refreshStats.fired += 1;
          run();
        }
      }, wait);
    },
  };
}

export interface EventPump {
  (ev: TaskEvent): void;
}

/** 把 SSE 事件变成节流后的刷新。`onLive` 每个事件都调用（本地追加），`onDetail` 是节流后的增量拉取 */
export function createEventPump(opts: {
  intervalMs: number;
  clock?: Clock;
  onTasks: () => void;
  onMessages: () => void;
  onPanels: () => void;
  onLive: (ev: TaskEvent) => void;
  onDetail: (ev: TaskEvent) => void;
  onDelta: (piece: string, streamId: string) => void;
}): EventPump {
  const tasks = createTrailingThrottle(opts.intervalMs, opts.clock);
  const messages = createTrailingThrottle(opts.intervalMs, opts.clock);
  const panels = createTrailingThrottle(opts.intervalMs, opts.clock);
  const detail = createTrailingThrottle(opts.intervalMs, opts.clock);
  let latest: TaskEvent | null = null;
  return (ev) => {
    if (ev.kind === "assistant_delta") {
      opts.onDelta(ev.message, ev.stream_id ?? "");
      return;
    }
    const plan = planFor(ev.kind);
    if (plan.detail) {
      opts.onLive(ev);
      latest = ev;
      detail.push(() => {
        if (latest) opts.onDetail(latest);
      });
    }
    if (plan.tasks) tasks.push(opts.onTasks);
    if (plan.messages) messages.push(opts.onMessages);
    if (plan.panels) panels.push(opts.onPanels);
  };
}

/** 展开的任务：先拉一次全量，之后每条事件本地追加，增量拉取按节流合并 */
export function useIncomingDetail(open: boolean, taskId: string, incoming: TaskEvent | null): TaskDetail | null {
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const cursor = useRef<string | undefined>(undefined);
  const throttle = useMemo(() => createTrailingThrottle(REFRESH_INTERVAL_MS), []);

  useEffect(() => {
    if (!open) return;
    api.task(taskId).then((d) => {
      setDetail(d);
      cursor.current = d.events.at(-1)?.id;
    }).catch(() => {});
  }, [open, taskId]);

  useEffect(() => {
    if (!open || !incoming || incoming.task_id !== taskId) return;
    setDetail((d) => (d ? mergeDetail(d, { task: d.task, events: [incoming] }) : d));
    throttle.push(() => {
      api.task(taskId, cursor.current).then((part) => {
        setDetail((d) => {
          const merged = mergeDetail(d, part);
          cursor.current = merged.events.at(-1)?.id;
          return merged;
        });
      }).catch(() => {});
    });
  }, [open, incoming, taskId, throttle]);

  return detail;
}

/** 把增量事件并进已有详情，按 id 去重 */
export function mergeDetail(prev: TaskDetail | null, incoming: TaskDetail): TaskDetail {
  if (!prev) return incoming;
  const seen = new Set(prev.events.map((e) => e.id));
  return { task: incoming.task, events: [...prev.events, ...incoming.events.filter((e) => !seen.has(e.id))] };
}

/** 同一帧里的流式片段合成一次更新 */
export function createDeltaBuffer(flush: (chunk: string) => void, raf: (cb: () => void) => void): (piece: string) => void {
  let pending = "";
  let scheduled = false;
  return (piece) => {
    pending += piece;
    if (scheduled) return;
    scheduled = true;
    raf(() => {
      scheduled = false;
      const chunk = pending;
      pending = "";
      if (chunk) flush(chunk);
    });
  };
}

/** 流式过程中是否重新解析 Markdown。同一时刻连着调用不会重复解析；结束时（done）一定解析 */
export function createMarkdownGate(intervalMs: number, now: () => number): { take(done: boolean): boolean; parses: number } {
  let last = now();
  let parses = 0;
  return {
    get parses() {
      return parses;
    },
    take(done) {
      if (!done && now() - last < intervalMs) return false;
      last = now();
      parses += 1;
      return true;
    },
  };
}
