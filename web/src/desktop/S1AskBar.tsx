import { useState } from "react";

export type S1BgJob = {
  /** 分流后的真实任务 id；受理中可为占位 pending */
  taskId: string;
  conversationId: string;
  requestId: string | null;
  label: string;
  status: "submitting" | "accepted" | "failed";
  error?: string | null;
};

type Props = {
  bgJob: S1BgJob | null;
  onAsk: (text: string) => void;
  onDelegate: (text: string) => void;
  onOpenBgTask: (job: S1BgJob) => void;
  /** UXFIX-J（INTENT-017）：受理提示「展开工作区」——按 Task ID 打开工作区 */
  onOpenTaskWorkspace?: (taskId: string) => void;
};

/** S1 桌面问答：不强制进工作区；委派走真实 B1 受理（首页真实委派切片）。 */
export function S1AskBar({ bgJob, onAsk, onDelegate, onOpenBgTask, onOpenTaskWorkspace }: Props) {
  const [text, setText] = useState("");

  const submitAsk = () => {
    const t = text.trim();
    if (!t) return;
    onAsk(t);
    setText("");
  };

  const submitDelegate = () => {
    const t = text.trim() || "继续调查并整理证据链";
    onDelegate(t);
    setText("");
  };

  const statusLabel =
    bgJob?.status === "submitting"
      ? "提交中…"
      : bgJob?.status === "failed"
        ? "受理失败"
        : "后台处理中（已受理）";

  return (
    <div className="s1-ask" data-testid="s1-ask-bar">
      <div className="s1-ask-row">
        <input
          id="s1-ask-input"
          data-testid="s1-ask-input"
          aria-label="S1 输入"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="先问答，或交代复杂任务…"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submitAsk();
            }
          }}
        />
        <button type="button" className="btn small" data-testid="s1-ask-send" onClick={submitAsk}>
          问答
        </button>
        <button type="button" className="btn primary small" data-testid="s1-delegate" onClick={submitDelegate}>
          委派任务
        </button>
      </div>
      {bgJob && (
        <div className="s1-bg-chip" data-testid="s1-bg-chip" role="status">
          <span data-testid="s1-bg-status">{statusLabel}</span>
          <span className="muted" data-testid="s1-bg-task">
            {bgJob.taskId}
          </span>
          {bgJob.conversationId && (
            <span className="muted" data-testid="s1-bg-conv">
              {bgJob.conversationId}
            </span>
          )}
          <button
            type="button"
            className="btn small"
            data-testid="s1-bg-open"
            disabled={bgJob.status !== "accepted"}
            onClick={() => onOpenBgTask(bgJob)}
          >
            打开任务
          </button>
          {onOpenTaskWorkspace && bgJob.status === "accepted" && bgJob.taskId !== "pending" && (
            <button
              type="button"
              className="btn small"
              data-testid="s1-bg-workspace"
              onClick={() => onOpenTaskWorkspace(bgJob.taskId)}
            >
              展开工作区
            </button>
          )}
          {bgJob.error && (
            <p className="bad-text" data-testid="s1-bg-error" role="alert">
              {bgJob.error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
