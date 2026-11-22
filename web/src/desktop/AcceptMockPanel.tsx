import { useEffect, useState } from "react";
import { api } from "../api";

type AcceptMode = "legacy" | "reliable";
type AcceptPhase = "idle" | "submitting" | "accepted" | "unknown" | "conflict" | "failed";

type Props = {
  taskId: string;
};

type View = {
  conversationId: string;
  clientOperationId: string;
  text: string;
  requestId: string | null;
  messageId: string | null;
  phase: AcceptPhase;
  dispatchCount: number;
  mode: AcceptMode;
  lastError: string | null;
};

function newOpId() {
  return `op-accept-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function AcceptMockPanel({ taskId }: Props) {
  const [v, setV] = useState<View | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api
      .createConversation()
      .then((conv) => {
        if (cancelled) return;
        setV({
          conversationId: conv.id,
          clientOperationId: newOpId(),
          text: "深入调查 SMT-03",
          requestId: null,
          messageId: null,
          phase: "idle",
          dispatchCount: 0,
          mode: "reliable",
          lastError: null,
        });
        setBootError(null);
      })
      .catch((e: Error) => {
        if (!cancelled) setBootError(e.message);
      });
    return () => {
      cancelled = true;
    };
  }, [taskId]);

  if (!v) {
    return (
      <div className="accept-mock" data-testid="accept-mock-panel">
        <p className="muted">加载受理…</p>
        {bootError && (
          <p className="evidence-error" data-testid="accept-error" role="alert">
            {bootError}
          </p>
        )}
      </div>
    );
  }

  const postAccept = async (opts: { dropReceipt?: boolean } = {}) => {
    const snap = v;
    if (!snap.text.trim()) {
      setV({ ...snap, phase: "failed", lastError: "正文为空" });
      return;
    }
    setV({ ...snap, phase: "submitting", lastError: null });
    const r = await api.desktopAcceptCreate(snap.conversationId, {
      client_operation_id: snap.clientOperationId,
      text: snap.text,
      task_id: taskId,
    });
    if (!r.ok) {
      if (r.status === 409) {
        setV({
          ...snap,
          phase: "conflict",
          lastError: "409 操作 ID 已用于不同正文",
          requestId: r.request_id ?? snap.requestId,
          messageId: r.message_id ?? snap.messageId,
        });
        return;
      }
      setV({ ...snap, phase: "failed", lastError: r.message });
      return;
    }
    setV({
      ...snap,
      requestId: r.body.request_id,
      messageId: r.body.message_id,
      dispatchCount: r.body.dispatch_count,
      lastError: opts.dropReceipt ? "受理回执丢失，请回读确认" : null,
      phase: opts.dropReceipt ? "unknown" : "accepted",
    });
  };

  return (
    <div className="accept-mock" data-testid="accept-mock-panel">
      <h3>受理（真实 B1）</h3>
      <dl className="evidence-dl">
        <dt>模式</dt>
        <dd data-testid="accept-mode">{v.mode === "reliable" ? "可靠" : "现有"}</dd>
        <dt>相位</dt>
        <dd data-testid="accept-phase">{v.phase}</dd>
        <dt>操作 ID</dt>
        <dd data-testid="accept-op-id">{v.clientOperationId}</dd>
        <dt>request_id</dt>
        <dd data-testid="accept-request-id">{v.requestId ?? "—"}</dd>
        <dt>派发次数</dt>
        <dd data-testid="accept-dispatch-count">{v.dispatchCount}</dd>
        <dt>Task</dt>
        <dd data-testid="accept-task">{taskId}</dd>
      </dl>
      {v.lastError && (
        <p className="evidence-error" data-testid="accept-error" role="status">
          {v.lastError}
        </p>
      )}
      {v.phase === "unknown" && (
        <p className="muted" data-testid="accept-pending-hint" role="status">
          待确认：回执丢失，请先回读或（可靠模式）用原操作 ID 重试
        </p>
      )}
      <label className="settings-field">
        正文
        <input
          data-testid="accept-text"
          value={v.text}
          onChange={(e) => setV({ ...v, text: e.target.value, lastError: null })}
        />
      </label>
      <div className="settings-actions">
        <button
          type="button"
          className="btn small"
          data-testid="accept-reset"
          onClick={() =>
            setV({
              ...v,
              clientOperationId: newOpId(),
              text: "深入调查 SMT-03",
              requestId: null,
              messageId: null,
              phase: "idle",
              dispatchCount: 0,
              lastError: null,
            })
          }
        >
          重置
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-mode-legacy"
          aria-pressed={v.mode === "legacy"}
          onClick={() => setV({ ...v, mode: "legacy", lastError: null })}
        >
          现有模式
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-mode-reliable"
          aria-pressed={v.mode === "reliable"}
          onClick={() => setV({ ...v, mode: "reliable", lastError: null })}
        >
          可靠模式
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="accept-submit"
          onClick={() => void postAccept()}
        >
          提交受理
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-drop-receipt"
          onClick={() => void postAccept({ dropReceipt: true })}
        >
          提交并丢回执
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-readback"
          onClick={() => {
            if (!v.requestId) {
              setV({ ...v, phase: "failed", lastError: "无可回读的 request_id" });
              return;
            }
            const rid = v.requestId;
            void api
              .desktopAcceptGet(rid)
              .then((rec) => {
                setV((prev) =>
                  prev
                    ? {
                        ...prev,
                        phase: "accepted",
                        dispatchCount: rec.dispatch_count,
                        messageId: rec.message_id,
                        lastError: null,
                      }
                    : prev,
                );
              })
              .catch((e: Error) =>
                setV((prev) => (prev ? { ...prev, phase: "failed", lastError: e.message } : prev)),
              );
          }}
        >
          回读确认
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-retry"
          onClick={() => {
            if (v.mode === "legacy" && v.phase === "unknown") {
              setV({ ...v, lastError: "现有模式请先回读确认" });
              return;
            }
            if (v.phase === "conflict") return;
            void postAccept({ dropReceipt: false });
          }}
        >
          原操作 ID 重试
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-change-text"
          onClick={() => setV({ ...v, text: `${v.text}（已改）`, lastError: null })}
        >
          改正文（同操作 ID）
        </button>
      </div>
    </div>
  );
}
