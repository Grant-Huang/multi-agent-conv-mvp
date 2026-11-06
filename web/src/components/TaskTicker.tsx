// 任务进度条：单行、上下滚动显示最新一条进度，点击展开全部历史。
// 用等宽小字体，与对话正文区分开，避免和回复内容混在一起。
import { useState } from "react";

export function TaskTicker(props: { lines: string[]; failed?: boolean }) {
  const { lines, failed } = props;
  const [expanded, setExpanded] = useState(false);
  if (lines.length === 0) return null;
  const last = lines[lines.length - 1];

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
        <span className="ticker-caret" aria-hidden="true">{expanded ? "▴" : "▾"}</span>
      </button>
      {expanded && (
        <ol className="ticker-history">
          {/* 最新的在上面，方便先看到结果 */}
          {[...lines].reverse().map((l, i) => (
            <li key={`${lines.length - i}`}>{l}</li>
          ))}
        </ol>
      )}
    </div>
  );
}
