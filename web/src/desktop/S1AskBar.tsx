import { useState } from "react";

export type S1BgJob = {
  /** 分流后的真实任务 id；受理中可为占位 pending */
  taskId: string;
  conversationId: string;
  requestId: string | null;
  label: string;
  /** UXFIX-H（INTENT-015）：done=任务已终结（完成/需处理），受理提示同步、不再显示后台处理中 */
  status: "submitting" | "accepted" | "failed" | "done";
  error?: string | null;
};

type Props = {
  bgJob: S1BgJob | null;
  onAsk: (text: string) => void;
  onDelegate: (text: string) => void;
  onOpenBgTask: (job: S1BgJob) => void;
};

/** S1 桌面问答：不强制进工作区；委派走真实 B1 受理（首页真实委派切片）。 */
export function S1AskBar({ bgJob, onAsk, onDelegate, onOpenBgTask }: Props) {
  const [text, setText] = useState("");

  const submitAsk = () => {
    const t = text.trim();
    if (!t) return;
    onAsk(t);
    setText("");
  };

  const submitDelegate = () => {
    // UXFIX-I（INTENT-016）：不代用户编造默认任务——输入为空时按钮不可用
    const t = text.trim();
    if (!t) return;
    onDelegate(t);
    setText("");
  };

  const statusLabel =
    bgJob?.status === "submitting"
      ? "提交中…"
      : bgJob?.status === "failed"
        ? "受理失败"
        : bgJob?.status === "done"
          ? "已完成"
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
        <button
          type="button"
          className="btn primary small"
          data-testid="s1-delegate"
          title={text.trim() ? undefined : "先说说要做什么"}
          disabled={!text.trim()}
          onClick={submitDelegate}
        >
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
