import { api, type DesktopAttentionDto } from "../api";

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
  onChange: (next: { items: AttentionItem[]; muted: boolean }) => void;
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

/** 主动消息浮层：真实 Attention API；不抢焦点；静音后 ordinary/digest 不再投递。 */
export function AttentionToast({ items, muted, composing, onChange }: Props) {
  const refresh = async () => {
    const r = await api.desktopAttentionList();
    onChange({
      items: r.items.map(mapItem),
      muted: r.mute_ordinary,
    });
  };

  const inject = async (kind: "ordinary" | "urgent") => {
    await api.desktopAttentionPost({
      kind,
      title: kind === "urgent" ? "紧急：产线联锁" : "建议：复核 SMT-03 证据",
      body: kind === "urgent" ? "安全联锁触发，请立即处理" : "可在稍后打开；不打断当前输入",
      task_id: "TASK-2026-018",
    });
    await refresh();
  };

  return (
    <div className="attn-root" data-testid="attn-root" data-composing={composing ? "1" : "0"}>
      <div className="attn-controls">
        <button
          type="button"
          className="btn small"
          data-testid="attn-inject-ordinary"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void inject("ordinary")}
        >
          注入普通建议
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="attn-inject-urgent"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void inject("urgent")}
        >
          注入紧急
        </button>
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
      </div>
      <ul className="attn-list" data-testid="attn-list" aria-live="polite">
        {items.map((it) => (
          <li
            key={it.id}
            className={`attn-item attn-${it.kind}`}
            data-testid={`attn-item-${it.kind}`}
            data-attn-id={it.id}
          >
            <strong>{it.title}</strong>
            <span className="muted">{it.body}</span>
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
          </li>
        ))}
      </ul>
      {composing && items.some((i) => i.kind === "ordinary") && (
        <p className="muted" data-testid="attn-no-steal">
          输入中：普通建议未抢焦点
        </p>
      )}
    </div>
  );
}
