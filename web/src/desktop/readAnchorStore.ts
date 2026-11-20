/** 阅读锚点与执行卡展开（A2.1 / DESIGN §3）；按 Conversation/Task + 本机。 */

export type ReadAnchor = {
  messageId: string;
  /** 消息顶部相对滚动容器顶部的像素偏移 */
  offsetPx: number;
};

const ANCHOR_PREFIX = "an.desktop.readAnchor.v1:";
const EXEC_PREFIX = "an.desktop.execOpen.v1:";

export function readAnchorKey(conversationId: string): string {
  return ANCHOR_PREFIX + conversationId;
}

export function loadReadAnchor(conversationId: string): ReadAnchor | null {
  try {
    const raw = localStorage.getItem(readAnchorKey(conversationId));
    if (!raw) return null;
    const p = JSON.parse(raw) as Partial<ReadAnchor>;
    if (typeof p.messageId !== "string" || !p.messageId) return null;
    return {
      messageId: p.messageId,
      offsetPx: typeof p.offsetPx === "number" ? p.offsetPx : 0,
    };
  } catch {
    return null;
  }
}

export function saveReadAnchor(conversationId: string, anchor: ReadAnchor): void {
  try {
    localStorage.setItem(readAnchorKey(conversationId), JSON.stringify(anchor));
  } catch {
    /* 忽略 */
  }
}

export function loadExecOpen(taskId: string): boolean {
  try {
    const raw = localStorage.getItem(EXEC_PREFIX + taskId);
    if (raw === null) return true;
    return raw === "1";
  } catch {
    return true;
  }
}

export function saveExecOpen(taskId: string, open: boolean): void {
  try {
    localStorage.setItem(EXEC_PREFIX + taskId, open ? "1" : "0");
  } catch {
    /* 忽略 */
  }
}
