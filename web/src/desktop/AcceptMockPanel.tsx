import { useState } from "react";
import {
  loadAccept,
  readBack,
  resetAccept,
  retrySameOp,
  setDraft,
  setMode,
  submitAccept,
  type AcceptRecord,
} from "./acceptMock";

export function AcceptMockPanel() {
  const [r, setR] = useState<AcceptRecord>(() => resetAccept());

  const apply = (next: AcceptRecord) => setR(next);

  return (
    <div className="accept-mock" data-testid="accept-mock-panel">
      <h3>受理（模拟）</h3>
      <dl className="evidence-dl">
        <dt>模式</dt>
        <dd data-testid="accept-mode">{r.mode === "reliable" ? "可靠" : "现有"}</dd>
        <dt>相位</dt>
        <dd data-testid="accept-phase">{r.phase}</dd>
        <dt>操作 ID</dt>
        <dd data-testid="accept-op-id">{r.clientOperationId}</dd>
        <dt>request_id</dt>
        <dd data-testid="accept-request-id">{r.requestId ?? "—"}</dd>
        <dt>派发次数</dt>
        <dd data-testid="accept-dispatch-count">{r.dispatchCount}</dd>
        <dt>Task</dt>
        <dd data-testid="accept-task">{r.taskId}</dd>
      </dl>
      {r.lastError && (
        <p className="evidence-error" data-testid="accept-error" role="status">
          {r.lastError}
        </p>
      )}
      {r.phase === "unknown" && (
        <p className="muted" data-testid="accept-pending-hint" role="status">
          待确认：回执丢失，请先回读或（可靠模式）用原操作 ID 重试
        </p>
      )}
      <label className="settings-field">
        正文
        <input
          data-testid="accept-text"
          value={r.text}
          onChange={(e) => apply(setDraft(e.target.value))}
        />
      </label>
      <div className="settings-actions">
        <button type="button" className="btn small" data-testid="accept-reset" onClick={() => apply(resetAccept())}>
          重置
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-mode-legacy"
          aria-pressed={r.mode === "legacy"}
          onClick={() => apply(setMode("legacy"))}
        >
          现有模式
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-mode-reliable"
          aria-pressed={r.mode === "reliable"}
          onClick={() => apply(setMode("reliable"))}
        >
          可靠模式
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="accept-submit"
          onClick={() => apply(submitAccept())}
        >
          提交受理
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-drop-receipt"
          onClick={() => apply(submitAccept({ dropReceipt: true }))}
        >
          提交并丢回执
        </button>
        <button type="button" className="btn small" data-testid="accept-readback" onClick={() => apply(readBack())}>
          回读确认
        </button>
        <button type="button" className="btn small" data-testid="accept-retry" onClick={() => apply(retrySameOp())}>
          原操作 ID 重试
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="accept-change-text"
          onClick={() => apply(setDraft(`${loadAccept().text}（已改）`))}
        >
          改正文（同操作 ID）
        </button>
      </div>
    </div>
  );
}
