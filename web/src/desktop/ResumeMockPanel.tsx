import { useEffect, useRef, useState } from "react";
import {
  api,
  subscribeDesktopEvents,
  type DesktopEventEnvelopeDto,
  type DesktopResumeRunDto,
} from "../api";

type Props = {
  onOpenTask?: (taskId: string) => void;
};

type RunView = {
  taskId: string;
  status: string;
  progress: string;
  version: number;
  seenEventIds: string[];
};

function mapStatus(raw: string | null | undefined, fallback: string): string {
  if (!raw) return fallback;
  const s = raw.toUpperCase();
  if (s === "RUNNING") return "running";
  if (s === "NEEDS_HUMAN") return "waiting_input";
  if (s === "QUEUED") return "queued";
  if (s === "COMPLETED") return "completed";
  if (s === "FAILED") return "failed";
  if (s === "CANCELLED") return "cancelled";
  return raw;
}

function fromSeed(runs: DesktopResumeRunDto[]): RunView[] {
  return runs.map((r) => ({
    taskId: r.task_id,
    status: r.status,
    progress: r.progress,
    version: r.version,
    seenEventIds: [],
  }));
}

export function ResumeMockPanel({ onOpenTask }: Props) {
  const [runs, setRuns] = useState<RunView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const conversationIdRef = useRef<string | null>(null);
  const unsubRef = useRef<(() => void) | null>(null);

  const stopSse = () => {
    unsubRef.current?.();
    unsubRef.current = null;
  };

  useEffect(() => () => stopSse(), []);

  const applyEnvelope = (ev: DesktopEventEnvelopeDto) => {
    const taskId = ev.task_id;
    if (!taskId) return;
    setRuns((prev) => {
      const idx = prev.findIndex((r) => r.taskId === taskId);
      if (idx < 0) return prev;
      const run = prev[idx];
      if (run.seenEventIds.includes(ev.event_id)) return prev;
      const next: RunView = {
        ...run,
        seenEventIds: [...run.seenEventIds, ev.event_id],
        status: mapStatus(ev.status, run.status),
        progress: ev.message || run.progress,
        version: run.version + 1,
      };
      const copy = [...prev];
      copy[idx] = next;
      return copy;
    });
  };

  const connectSse = (conversationId: string, after: string) => {
    stopSse();
    conversationIdRef.current = conversationId;
    unsubRef.current = subscribeDesktopEvents(conversationId, after, applyEnvelope);
  };

  return (
    <div className="resume-mock" data-testid="resume-mock-panel">
      <h3>异步 Run 恢复（B2 SSE）</h3>
      {error && (
        <p className="evidence-error" role="alert">
          {error}
        </p>
      )}
      <div className="settings-actions">
        <button
          type="button"
          className="btn primary small"
          data-testid="resume-seed"
          onClick={() => {
            void api
              .desktopResumeSeed()
              .then((r) => {
                setError(null);
                setRuns(fromSeed(r.runs));
                connectSse(r.conversation_id, r.after_cursor);
              })
              .catch((e: Error) => setError(e.message));
          }}
        >
          种子双任务
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="resume-replay-a"
          onClick={() => {
            const cid = conversationIdRef.current;
            if (!cid) {
              setError("请先种子双任务");
              return;
            }
            void api
              .desktopResumeReplayA(cid)
              .then(() => setError(null))
              .catch((e: Error) => setError(e.message));
          }}
        >
          断线重放 A（含重复）
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="resume-continue-b"
          onClick={() => {
            const cid = conversationIdRef.current;
            if (!cid) {
              setError("请先种子双任务");
              return;
            }
            void api
              .desktopResumeContinueB(cid)
              .then(() => setError(null))
              .catch((e: Error) => setError(e.message));
          }}
        >
          补充 B 并继续
        </button>
      </div>
      <ul className="resume-list" data-testid="resume-run-list">
        {runs.map((r) => (
          <li key={r.taskId} data-testid={`resume-run-${r.taskId}`} data-run-status={r.status}>
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
