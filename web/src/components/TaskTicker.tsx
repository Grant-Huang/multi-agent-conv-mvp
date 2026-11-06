// 走马灯：挂在任务的简短回复下面。收起时只显示最新一行；展开时从旧到新列出全部进度。
// 运行中在行尾显示动画 SVG；结束后行尾是展开箭头。背景与正文一致，字体比正文小、颜色更浅。
import { useState } from "react";

export function TaskTicker(props: { lines: string[]; running: boolean; failed?: boolean }) {
  const { lines, running, failed } = props;
  const [expanded, setExpanded] = useState(false);
  // 还没有进度时，运行中显示占位文字，避免行看起来是空的
  const last = lines.length > 0 ? lines[lines.length - 1] : running ? "准备中…" : "";
  if (!last) return null;

  return (
    <div className={`ticker ${failed ? "is-failed" : ""}`}>
      <button
        type="button"
        className="ticker-line"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        title={expanded ? "收起历史" : "展开全部进度"}
      >
        {/* key 随条数变化而变，新进度出现时触发一次"从下往上滑入"动画 */}
        <span key={lines.length} className="ticker-text">{last}</span>
        {running ? (
          // 运行指示：行尾的旋转圆弧，任务结束后消失
          <svg className="ticker-spin" viewBox="0 0 16 16" width="12" height="12" role="img" aria-label="执行中">
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="2"
              strokeDasharray="28 10" strokeLinecap="round" />
          </svg>
        ) : (
          <span className="ticker-caret" aria-hidden="true">{expanded ? "▴" : "▾"}</span>
        )}
      </button>
      {expanded && (
        <ol className="ticker-history">
          {/* 从旧到新：与执行顺序一致，最新的一行在最下面 */}
          {lines.map((l, i) => (
            <li key={i}>{l}</li>
          ))}
        </ol>
      )}
    </div>
  );
}
