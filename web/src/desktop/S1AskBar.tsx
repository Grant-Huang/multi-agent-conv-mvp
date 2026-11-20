import { useState } from "react";
import { SMT03_TASK_ID } from "./mockData";

export type S1BgJob = {
  taskId: string;
  label: string;
  status: "submitting" | "accepted";
};

type Props = {
  bgJob: S1BgJob | null;
  onAsk: (text: string) => void;
  onDelegate: (text: string) => void;
  onOpenBgTask: (taskId: string) => void;
};

/** S1 桌面问答：不强制进工作区；委派后仅显示后台芯片（A5.1 / E2E-ADASK）。 */
export function S1AskBar({ bgJob, onAsk, onDelegate, onOpenBgTask }: Props) {
  const [text, setText] = useState("");

  const submitAsk = () => {
    const t = text.trim();
    if (!t) return;
    onAsk(t);
    setText("");
  };

  const submitDelegate = () => {
    const t = text.trim() || "继续调查 SMT-03 质量根因";
    onDelegate(t);
    setText("");
  };

  return (
    <div className="s1-ask" data-testid="s1-ask-bar">
      <label className="s1-ask-label" htmlFor="s1-ask-input">
        S1 快速问答 / 委派
      </label>
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
          <span data-testid="s1-bg-status">
            {bgJob.status === "submitting" ? "提交中…" : "后台处理中（已受理）"}
          </span>
          <span className="muted" data-testid="s1-bg-task">
            {bgJob.taskId}
          </span>
          <button type="button" className="btn small" data-testid="s1-bg-open" onClick={() => onOpenBgTask(bgJob.taskId)}>
            打开任务
          </button>
        </div>
      )}
      <p className="muted s1-ask-hint">委派默认关联 {SMT03_TASK_ID}；不强制离开桌面。</p>
    </div>
  );
}
