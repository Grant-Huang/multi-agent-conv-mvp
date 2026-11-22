import { useEffect, useState } from "react";
import { api, type DesktopApprovalDto } from "../api";

type Props = {
  taskId: string;
};

function toView(a: DesktopApprovalDto) {
  return a;
}

export function ApprovalMockPanel({ taskId }: Props) {
  const [rec, setRec] = useState<DesktopApprovalDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [paramsDraft, setParamsDraft] = useState("");

  const refresh = (next: DesktopApprovalDto, err?: string | null) => {
    setRec(next);
    setParamsDraft(next.params);
    setError(err ?? null);
  };

  useEffect(() => {
    let cancelled = false;
    api
      .desktopApprovalReset(taskId)
      .then((r) => {
        if (!cancelled) refresh(toView(r.approval));
      })
      .catch((e: Error) => {
        if (!cancelled) setError(e.message);
      });
    return () => {
      cancelled = true;
    };
  }, [taskId]);

  if (!rec) {
    return (
      <div className="appr-mock" data-testid="approval-mock-panel">
        <p className="muted">加载审批…</p>
        {error && (
          <p className="evidence-error" data-testid="appr-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="appr-mock" data-testid="approval-mock-panel">
      <h3>动作审批（真实网关）</h3>
      <dl className="evidence-dl">
        <dt>方案版本</dt>
        <dd data-testid="appr-revision">{rec.plan_revision}</dd>
        <dt>状态</dt>
        <dd data-testid="appr-status">{rec.status}</dd>
        <dt>批准人</dt>
        <dd data-testid="appr-decider">{rec.decided_by ?? "—"}</dd>
        <dt>执行次数</dt>
        <dd data-testid="appr-exec-count">{rec.execute_count}</dd>
        <dt>回执</dt>
        <dd data-testid="appr-receipt">{rec.last_receipt}</dd>
      </dl>
      <label className="settings-field">
        参数
        <input
          data-testid="appr-params"
          value={paramsDraft}
          onChange={(e) => setParamsDraft(e.target.value)}
        />
      </label>
      <div className="settings-actions">
        <button
          type="button"
          className="btn small"
          data-testid="appr-bump"
          onClick={() => {
            void api
              .desktopApprovalBump(rec.id, paramsDraft || `${rec.params}（已改）`)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
          改参数并升版
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="appr-approve"
          onClick={() => {
            void api
              .desktopApprovalDecide(rec.id, "approve", rec.plan_revision)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
          批准
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-reject"
          onClick={() => {
            void api
              .desktopApprovalDecide(rec.id, "reject", rec.plan_revision)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
          拒绝
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-expire"
          onClick={() => {
            void api
              .desktopApprovalExpire(rec.id)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
          注入超时
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-execute"
          onClick={() => {
            void api.desktopApprovalExecute(rec.id, rec.plan_revision).then((r) => {
              if (r.ok) refresh(r.approval);
              else if (r.approval) refresh(r.approval, r.message);
              else setError(r.message);
            });
          }}
        >
          执行
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-execute-unknown"
          onClick={() => {
            void api.desktopApprovalExecute(rec.id, rec.plan_revision, true).then((r) => {
              if (r.ok) refresh(r.approval);
              else if (r.approval) refresh(r.approval, r.message);
              else setError(r.message);
            });
          }}
        >
          执行（未知回执）
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-ack-unknown"
          onClick={() => {
            void api
              .desktopApprovalAckUnknown(rec.id)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
          查证未知回执
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="appr-reset"
          onClick={() => {
            void api
              .desktopApprovalReset(taskId)
              .then((r) => refresh(r.approval))
              .catch((e: Error) => setError(e.message));
          }}
        >
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
