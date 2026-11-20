import { useState } from "react";
import {
  clearRuns,
  loadRuns,
  replayAfterDisconnect,
  resumeWaiting,
  seedParallelRuns,
  type MockRun,
} from "./runResumeMock";

const TASK_A = "TASK-2026-018";
const TASK_B = "TASK-2026-021";

type Props = {
  onOpenTask?: (taskId: string) => void;
};

export function ResumeMockPanel({ onOpenTask }: Props) {
  const [runs, setRuns] = useState<MockRun[]>(() => loadRuns());

  const refresh = () => setRuns(loadRuns());

  return (
    <div className="resume-mock" data-testid="resume-mock-panel">
      <h3>异步 Run 恢复（模拟）</h3>
      <div className="settings-actions">
        <button
          type="button"
          className="btn primary small"
          data-testid="resume-seed"
          onClick={() => {
            clearRuns();
            setRuns(seedParallelRuns(TASK_A, TASK_B));
          }}
        >
          种子双任务
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="resume-replay-a"
          onClick={() => {
            const run = runs.find((r) => r.taskId === TASK_A);
            if (!run) return;
            replayAfterDisconnect(run.id, [
              { id: "ev-a-1", status: "running", progress: "采集 40%" },
              { id: "ev-a-1", status: "running", progress: "采集 40%（重复）" },
              { id: "ev-a-2", status: "running", progress: "采集 80%" },
            ]);
            refresh();
          }}
        >
          断线重放 A（含重复）
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="resume-continue-b"
          onClick={() => {
            const run = runs.find((r) => r.taskId === TASK_B);
            if (!run) return;
            resumeWaiting(run.id, "限定 Line-B 批次");
            refresh();
          }}
        >
          补充 B 并继续
        </button>
      </div>
      <ul className="resume-list" data-testid="resume-run-list">
        {runs.map((r) => (
          <li key={r.id} data-testid={`resume-run-${r.taskId}`} data-run-status={r.status}>
            <strong>{r.taskId}</strong>
            <span data-testid={`resume-status-${r.taskId}`}>{r.status}</span>
            <span className="muted" data-testid={`resume-progress-${r.taskId}`}>
              {r.progress}
            </span>
            <span className="muted" data-testid={`resume-version-${r.taskId}`}>
              v{r.version}
            </span>
            <span className="muted" data-testid={`resume-events-${r.taskId}`}>
              事件 {r.seenEventIds.length}
            </span>
            {onOpenTask && (
              <button type="button" className="btn small" onClick={() => onOpenTask(r.taskId)}>
                打开
              </button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
