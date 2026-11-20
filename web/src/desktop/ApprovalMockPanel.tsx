import { useState } from "react";
import {
  acknowledgeUnknown,
  bumpParams,
  decide,
  executeAction,
  expire,
  resetApproval,
  type ApprovalRecord,
} from "./approvalMock";

type Props = {
  taskId: string;
};

export function ApprovalMockPanel({ taskId }: Props) {
  const [rec, setRec] = useState<ApprovalRecord>(() => resetApproval(taskId));
  const [error, setError] = useState<string | null>(null);
  const [paramsDraft, setParamsDraft] = useState(rec.params);

  const refresh = (next: ApprovalRecord, err?: string | null) => {
    setRec(next);
    setParamsDraft(next.params);
    setError(err ?? null);
  };

  return (
    <div className="appr-mock" data-testid="approval-mock-panel">
      <h3>动作审批（模拟）</h3>
      <dl className="evidence-dl">
        <dt>方案版本</dt>
        <dd data-testid="appr-revision">{rec.planRevision}</dd>
        <dt>状态</dt>
        <dd data-testid="appr-status">{rec.status}</dd>
        <dt>批准人</dt>
        <dd data-testid="appr-decider">{rec.decidedBy ?? "—"}</dd>
        <dt>执行次数</dt>
        <dd data-testid="appr-exec-count">{rec.executeCount}</dd>
        <dt>回执</dt>
        <dd data-testid="appr-receipt">{rec.lastReceipt}</dd>
      </dl>
      <label className="settings-field">
        参数
        <input data-testid="appr-params" value={paramsDraft} onChange={(e) => setParamsDraft(e.target.value)} />
      </label>
      <div className="settings-actions">
        <button
          type="button"
          className="btn small"
          data-testid="appr-bump"
          onClick={() => refresh(bumpParams(taskId, paramsDraft || rec.params + "（已改）"))}
        >
          改参数并升版
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="appr-approve"
          onClick={() => {
            const r = decide(taskId, "approve", rec.planRevision);
            refresh(r.ok ? r.record : r.record, r.ok ? null : r.reason);
          }}
        >
          批准
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-reject"
          onClick={() => {
            const r = decide(taskId, "reject", rec.planRevision);
            refresh(r.ok ? r.record : r.record, r.ok ? null : r.reason);
          }}
        >
          拒绝
        </button>
        <button type="button" className="btn small" data-testid="appr-expire" onClick={() => refresh(expire(taskId))}>
          注入超时
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-execute"
          onClick={() => {
            const r = executeAction(taskId, rec.planRevision);
            refresh(r.ok ? r.record : r.record, r.ok ? null : r.reason);
          }}
        >
          执行
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-execute-unknown"
          onClick={() => {
            const r = executeAction(taskId, rec.planRevision, { forceUnknown: true });
            refresh(r.ok ? r.record : r.record, r.ok ? null : r.reason);
          }}
        >
          执行（未知回执）
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-ack-unknown"
          onClick={() => refresh(acknowledgeUnknown(taskId))}
        >
          查证未知回执
        </button>
        <button type="button" className="btn small" data-testid="appr-reset" onClick={() => refresh(resetApproval(taskId))}>
          重置
        </button>
      </div>
      {error && (
        <p className="evidence-error" data-testid="appr-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
