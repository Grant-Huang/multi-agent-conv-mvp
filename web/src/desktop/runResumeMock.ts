/** Run 恢复 Mock（A5.2 / AD-RESUME）；并行任务状态隔离与重放去重。 */

export type RunStatus = "queued" | "running" | "waiting_input" | "completed" | "failed" | "cancelled";

export type MockRun = {
  id: string;
  taskId: string;
  status: RunStatus;
  attempt: number;
  progress: string;
  version: number;
  seenEventIds: string[];
};

const KEY = "an.desktop.runResume.v1";

export function loadRuns(): MockRun[] {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as MockRun[]) : [];
  } catch {
    return [];
  }
}

function save(runs: MockRun[]): MockRun[] {
  try {
    localStorage.setItem(KEY, JSON.stringify(runs));
  } catch {
    /* 忽略 */
  }
  return runs;
}

export function seedParallelRuns(taskA: string, taskB: string): MockRun[] {
  const runs: MockRun[] = [
    {
      id: `run-${taskA}-1`,
      taskId: taskA,
      status: "running",
      attempt: 1,
      progress: "采集中",
      version: 1,
      seenEventIds: [],
    },
    {
      id: `run-${taskB}-1`,
      taskId: taskB,
      status: "waiting_input",
      attempt: 1,
      progress: "等待补充范围",
      version: 1,
      seenEventIds: [],
    },
  ];
  return save(runs);
}

export function applyEvent(
  runId: string,
  event: { id: string; status?: RunStatus; progress?: string },
): MockRun | null {
  const runs = loadRuns();
  const idx = runs.findIndex((r) => r.id === runId);
  if (idx < 0) return null;
  const run = runs[idx];
  if (run.seenEventIds.includes(event.id)) {
    return run;
  }
  const next: MockRun = {
    ...run,
    seenEventIds: [...run.seenEventIds, event.id],
    status: event.status ?? run.status,
    progress: event.progress ?? run.progress,
    version: run.version + 1,
  };
  runs[idx] = next;
  save(runs);
  return next;
}

/** 模拟断线后重放：含重复事件；不得回退 version / 串写他任务。 */
export function replayAfterDisconnect(runId: string, events: { id: string; status?: RunStatus; progress?: string }[]): MockRun | null {
  let last: MockRun | null = null;
  for (const ev of events) {
    last = applyEvent(runId, ev);
  }
  return last;
}

export function resumeWaiting(runId: string, input: string): MockRun | null {
  const runs = loadRuns();
  const idx = runs.findIndex((r) => r.id === runId);
  if (idx < 0) return null;
  const run = runs[idx];
  if (run.status !== "waiting_input") return run;
  const next: MockRun = {
    ...run,
    status: "running",
    progress: `已补充：${input.slice(0, 40)}`,
    version: run.version + 1,
  };
  runs[idx] = next;
  save(runs);
  return next;
}

export function clearRuns(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
}
