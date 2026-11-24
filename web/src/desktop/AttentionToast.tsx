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

/** 主动消息浮层（T-88 整治）：只展示与忽略/静音；测试注入走 API（E2E 直接 POST）。
 *  列表为空时不渲染，不占首页空间。静音是正式用户控制（DESIGN §7）。 */
export function AttentionToast({ items, muted, composing, onChange }: Props) {
  const refresh = async () => {
    const r = await api.desktopAttentionList();
    onChange({
      items: r.items.map(mapItem),
      muted: r.mute_ordinary,
    });
  };

  if (items.length === 0) return null;

  return (
    <div className="attn-root" data-testid="attn-root" data-composing={composing ? "1" : "0"}>
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
      {composing && items.some((i) => i.kind === "ordinary") && (
        <p className="muted" data-testid="attn-no-steal">
          输入中：普通建议未抢焦点
        </p>
      )}
    </div>
  );
}
