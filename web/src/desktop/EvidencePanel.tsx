import { useEffect } from "react";
import { evidenceForTask, type EvidenceRecord } from "./evidenceMock";

export type EvidenceLayer = "list" | "preview" | "source";

type Props = {
  taskId: string;
  layer: EvidenceLayer;
  evidenceId: string | null;
  /** 演示空列表：传入 empty */
  mode?: "normal" | "empty";
  onLayer: (layer: EvidenceLayer, evidenceId: string | null) => void;
  onClose: () => void;
  onDemoEmpty?: () => void;
};

export function EvidencePanel({ taskId, layer, evidenceId, mode = "normal", onLayer, onClose, onDemoEmpty }: Props) {
  const list = mode === "empty" ? ([] as EvidenceRecord[]) : evidenceForTask(taskId);
  const current = list.find((e) => e.id === evidenceId) ?? null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (layer === "source") onLayer("preview", evidenceId);
      else if (layer === "preview") onLayer("list", null);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layer, evidenceId, onClose, onLayer]);

  return (
    <div className="desktop-panel evidence-panel" data-testid="evidence-panel" role="dialog" aria-label="证据">
      <header className="desktop-panel-head">
        <button type="button" className="btn small" data-testid="evidence-back" onClick={() => {
          if (layer === "source") onLayer("preview", evidenceId);
          else if (layer === "preview") onLayer("list", null);
          else onClose();
        }}>
          {layer === "list" ? "关闭证据" : "返回上层"}
        </button>
        <h2>证据</h2>
        <span className="desktop-sim">模拟</span>
      </header>

      {layer === "list" && (
        <div data-testid="evidence-list">
          {list.length === 0 ? (
            <p className="desktop-panel-empty" data-testid="evidence-empty" role="status">
              暂无证据记录
            </p>
          ) : (
            <ul className="evidence-list">
              {list.map((e) => (
                <li key={e.id}>
                  <button
                    type="button"
                    className="evidence-item"
                    data-testid={`evidence-item-${e.id}`}
                    onClick={() => onLayer("preview", e.id)}
                  >
                    <span className="evidence-title">{e.title}</span>
                    <span className="evidence-meta">
                      {e.objectLabel} · {e.version} · {e.status}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {onDemoEmpty && mode !== "empty" && (
            <button type="button" className="btn small" data-testid="evidence-demo-empty" onClick={onDemoEmpty}>
              空列表夹具
            </button>
          )}
          {mode === "empty" && (
            <button type="button" className="btn small" data-testid="evidence-demo-restore" onClick={() => onClose()}>
              关闭空夹具
            </button>
          )}
        </div>
      )}

      {layer === "preview" && current && (
        <div data-testid="evidence-preview">
          <h3>{current.title}</h3>
          <dl className="evidence-dl">
            <dt>业务对象</dt>
            <dd data-testid="evidence-object">{current.objectLabel}</dd>
            <dt>采集时间</dt>
            <dd data-testid="evidence-time">{current.collectedAt}</dd>
            <dt>来源类型</dt>
            <dd>{current.sourceType}</dd>
            <dt>版本</dt>
            <dd data-testid="evidence-version">{current.version}</dd>
            <dt>任务</dt>
            <dd data-testid="evidence-task">{current.taskId}</dd>
          </dl>
          {current.status === "failed" && (
            <p className="evidence-error" data-testid="evidence-error" role="alert">
              {current.errorMessage}
            </p>
          )}
          {current.status === "expired" && (
            <p className="evidence-error" data-testid="evidence-expired" role="alert">
              {current.errorMessage}
            </p>
          )}
          {current.status === "ok" && <p data-testid="evidence-summary">{current.summary}</p>}
          {current.status !== "failed" && (
            <button
              type="button"
              className="btn small"
              data-testid="evidence-open-source"
              onClick={() => onLayer("source", current.id)}
            >
              查看来源原文
            </button>
          )}
        </div>
      )}

      {layer === "source" && current && (
        <div data-testid="evidence-source">
          <h3>来源原文</h3>
          <pre className="evidence-source-body">{current.sourceBody || "（无原文）"}</pre>
        </div>
      )}
    </div>
  );
}
