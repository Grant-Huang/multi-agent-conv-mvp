/** 主动消息 / 注意力策略 Mock（A5.1 / DESIGN §7）；首版本地偏好。 */

export type AttentionKind = "ordinary" | "digest" | "urgent";

export type AttentionItem = {
  id: string;
  kind: AttentionKind;
  title: string;
  body: string;
  taskId?: string;
  at: number;
};

const MUTE_KEY = "an.desktop.attn.mute.v1";
const QUEUE_KEY = "an.desktop.attn.queue.v1";

export function loadMuteOrdinary(): boolean {
  try {
    return localStorage.getItem(MUTE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveMuteOrdinary(mute: boolean): void {
  try {
    localStorage.setItem(MUTE_KEY, mute ? "1" : "0");
  } catch {
    /* 忽略 */
  }
}

export function loadAttentionQueue(): AttentionItem[] {
  try {
    const raw = localStorage.getItem(QUEUE_KEY);
    return raw ? (JSON.parse(raw) as AttentionItem[]) : [];
  } catch {
    return [];
  }
}

function writeQueue(items: AttentionItem[]) {
  try {
    localStorage.setItem(QUEUE_KEY, JSON.stringify(items.slice(-20)));
  } catch {
    /* 忽略 */
  }
}

/** 投递：静音时丢弃 ordinary/digest；urgent 始终入队。返回是否实际展示。 */
export function enqueueAttention(item: Omit<AttentionItem, "id" | "at"> & { id?: string }): { delivered: boolean; item: AttentionItem | null } {
  const mute = loadMuteOrdinary();
  if (mute && item.kind !== "urgent") {
    return { delivered: false, item: null };
  }
  const full: AttentionItem = {
    id: item.id ?? `attn-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
    kind: item.kind,
    title: item.title,
    body: item.body,
    taskId: item.taskId,
    at: Date.now(),
  };
  const q = loadAttentionQueue();
  // 同标题 ordinary 去重：只保留一条
  const next =
    full.kind === "ordinary"
      ? [...q.filter((x) => !(x.kind === "ordinary" && x.title === full.title)), full]
      : [...q, full];
  writeQueue(next);
  return { delivered: true, item: full };
}

export function dismissAttention(id: string): void {
  writeQueue(loadAttentionQueue().filter((x) => x.id !== id));
}

export function clearAttentionQueue(): void {
  writeQueue([]);
}
