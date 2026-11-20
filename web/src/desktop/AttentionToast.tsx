import {
  dismissAttention,
  enqueueAttention,
  loadAttentionQueue,
  loadMuteOrdinary,
  saveMuteOrdinary,
  type AttentionItem,
} from "./attentionStore";

type Props = {
  items: AttentionItem[];
  muted: boolean;
  composing: boolean;
  onChange: (next: { items: AttentionItem[]; muted: boolean }) => void;
};

/** 主动消息浮层：不抢焦点；静音后 ordinary/digest 不再投递。 */
export function AttentionToast({ items, muted, composing, onChange }: Props) {
  const refresh = () => onChange({ items: loadAttentionQueue(), muted: loadMuteOrdinary() });

  const inject = (kind: AttentionItem["kind"]) => {
    enqueueAttention({
      kind,
      title: kind === "urgent" ? "紧急：产线联锁" : kind === "digest" ? "摘要：多条关注更新" : "建议：复核 SMT-03 证据",
      body: kind === "urgent" ? "安全联锁触发，请立即处理" : "可在稍后打开；不打断当前输入",
      taskId: "TASK-2026-018",
    });
    refresh();
  };

  return (
    <div className="attn-root" data-testid="attn-root" data-composing={composing ? "1" : "0"}>
      <div className="attn-controls">
        <button
          type="button"
          className="btn small"
          data-testid="attn-inject-ordinary"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => inject("ordinary")}
        >
          注入普通建议
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="attn-inject-urgent"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => inject("urgent")}
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
            saveMuteOrdinary(!muted);
            refresh();
          }}
        >
          {muted ? "取消静音普通建议" : "静音普通建议"}
        </button>
      </div>
      <ul className="attn-list" data-testid="attn-list" aria-live="polite">
        {items.map((it) => (
          <li key={it.id} className={`attn-item attn-${it.kind}`} data-testid={`attn-item-${it.kind}`} data-attn-id={it.id}>
            <strong>{it.title}</strong>
            <span className="muted">{it.body}</span>
            <button type="button" className="btn small" data-testid="attn-dismiss" onClick={() => { dismissAttention(it.id); refresh(); }}>
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
