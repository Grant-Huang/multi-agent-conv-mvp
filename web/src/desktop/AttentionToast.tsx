import { useState } from "react";
import { api, type DesktopAttentionDto } from "../api";
import { filterSnoozed, snoozeAttention } from "./attentionStore";

export type AttentionItem = {
  id: string;
  kind: "ordinary" | "digest" | "urgent" | string;
  title: string;
  body: string;
  taskId?: string;
  at?: number;
};

type Props = {
  items: AttentionItem[];
  muted: boolean;
  composing: boolean;
  /** UXFIX-N（INTENT-021）：浮层“打开”直达关联任务 */
  onOpenTask?: (taskId: string) => void;
  onChange: (next: { items: AttentionItem[]; muted: boolean }) => void;
};

const KIND_LABEL: Record<string, string> = {
  ordinary: "普通建议",
  digest: "摘要",
  urgent: "紧急",
};

function mapItem(d: DesktopAttentionDto): AttentionItem {
  return {
    id: d.id,
    kind: d.kind,
    title: d.title,
    body: d.body,
    taskId: d.task_id ?? undefined,
  };
}

/** 主动消息浮层（T-88 整治）：展示原因与来源，可 打开 / 稍后 / 忽略 / 静音（DESIGN §7）。
 *  “稍后”只收起浮层（本机留存，到点恢复），不动服务端，“为你关注”保留；
 *  “忽略”删除服务端记录，浮层与“为你关注”一致移除。测试注入走 API（E2E 直接 POST）。 */
export function AttentionToast({ items, muted, composing, onOpenTask, onChange }: Props) {
  const [snoozeMinutes, setSnoozeMinutes] = useState(5);
  const refresh = async () => {
    const r = await api.desktopAttentionList();
    onChange({
      items: r.items.map(mapItem),
      muted: r.mute_ordinary,
    });
  };

  const visible = filterSnoozed(items);
  if (visible.length === 0) return null;

  return (
    <div className="attn-root" data-testid="attn-root" data-composing={composing ? "1" : "0"}>
      <ul className="attn-list" data-testid="attn-list" aria-live="polite">
        {visible.map((it) => (
          <li
            key={it.id}
            className={`attn-item attn-${it.kind}`}
            data-testid={`attn-item-${it.kind}`}
            data-attn-id={it.id}
          >
            <strong>{it.title}</strong>
            <span className="muted">{it.body}</span>
            {/* 原因与来源（DESIGN §7）：为什么推给我、来自哪里 */}
            <span className="muted attn-source" data-testid="attn-source">
              {KIND_LABEL[it.kind] ?? it.kind}
              {it.taskId ? ` · 关联任务 ${it.taskId}` : " · 系统提醒"}
            </span>
            <span className="attn-actions">
              {it.taskId && onOpenTask && (
                <button
                  type="button"
                  className="btn small"
                  data-testid="attn-open"
                  onClick={() => onOpenTask(it.taskId!)}
                >
                  打开
                </button>
              )}
              <select
                className="attn-snooze-select"
                data-testid="attn-snooze-select"
                aria-label="稍后提醒时间"
                value={snoozeMinutes}
                onChange={(e) => setSnoozeMinutes(Number(e.target.value))}
              >
                <option value={5}>5 分钟</option>
                <option value={60}>1 小时</option>
              </select>
              <button
                type="button"
                className="btn small"
                data-testid="attn-snooze"
                title="收起浮层，提醒保留在「为你关注」，到点后浮层重新出现"
                onClick={() => {
                  snoozeAttention(it.id, snoozeMinutes);
                  onChange({ items: visible.filter((x) => x.id !== it.id), muted });
                }}
              >
                稍后
              </button>
              <button
                type="button"
                className="btn small"
                data-testid="attn-dismiss"
                onClick={() => {
                  void api.desktopAttentionDismiss(it.id).then(() => refresh());
                }}
              >
                忽略
              </button>
            </span>
            <span className="muted attn-dismiss-hint" data-testid="attn-dismiss-hint">
              忽略后会从「为你关注」移除
            </span>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="btn small"
        data-testid="attn-mute-toggle"
        aria-pressed={muted}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          void api.desktopAttentionMute(!muted).then(() => refresh());
        }}
      >
        {muted ? "取消静音普通建议" : "静音普通建议"}
      </button>
      {composing && visible.some((i) => i.kind === "ordinary") && (
        <p className="muted" data-testid="attn-no-steal">
          输入中：普通建议未抢焦点
        </p>
      )}
    </div>
  );
}
