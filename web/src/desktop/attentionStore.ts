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

// ===== UXFIX-N（#322，INTENT-021）：“稍后”——只收起浮层，不动服务端，“为你关注”保留 =====

const SNOOZE_KEY = "an.desktop.attn.snooze.v1";

function loadSnooze(): Record<string, number> {
  try {
    const raw = sessionStorage.getItem(SNOOZE_KEY);
    return raw ? (JSON.parse(raw) as Record<string, number>) : {};
  } catch {
    return {};
  }
}

/** 稍后：到点前不再在浮层出现；不删除服务端记录，“为你关注”不受影响 */
export function snoozeAttention(id: string, minutes: number): void {
  try {
    const all = loadSnooze();
    all[id] = Date.now() + minutes * 60_000;
    sessionStorage.setItem(SNOOZE_KEY, JSON.stringify(all));
  } catch {
    /* 忽略 */
  }
}

/** 过滤掉仍在“稍后”中的提醒（过期的顺手清理） */
export function filterSnoozed<T extends { id: string }>(items: T[]): T[] {
  const all = loadSnooze();
  const now = Date.now();
  let changed = false;
  const out = items.filter((it) => {
    const until = all[it.id];
    if (until === undefined) return true;
    if (until <= now) {
      delete all[it.id];
      changed = true;
      return true;
    }
    return false;
  });
  if (changed) {
    try {
      sessionStorage.setItem(SNOOZE_KEY, JSON.stringify(all));
    } catch {
      /* 忽略 */
    }
  }
  return out;
}
