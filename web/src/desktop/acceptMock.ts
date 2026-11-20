/** 受理 Mock（A5.4 / AD-ACCEPT / M1）；模拟丢回执、回读与操作 ID 幂等。 */

export type AcceptMode = "legacy" | "reliable";
export type AcceptPhase = "idle" | "submitting" | "accepted" | "unknown" | "conflict" | "failed";

export type AcceptRecord = {
  conversationId: string;
  taskId: string;
  clientOperationId: string;
  text: string;
  /** 首次成功/丢回执时钉住的正文，供同 ID 冲突检测 */
  committedText: string | null;
  requestId: string | null;
  messageId: string | null;
  phase: AcceptPhase;
  dispatchCount: number;
  mode: AcceptMode;
  lastError: string | null;
};

const KEY = "an.desktop.acceptMock.v1";

export function loadAccept(): AcceptRecord {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw) as AcceptRecord;
  } catch {
    /* 忽略 */
  }
  return empty();
}

function empty(): AcceptRecord {
  return {
    conversationId: "conv-smt-03",
    taskId: "TASK-2026-018",
    clientOperationId: "op-accept-1",
    text: "深入调查 SMT-03",
    committedText: null,
    requestId: null,
    messageId: null,
    phase: "idle",
    dispatchCount: 0,
    mode: "reliable",
    lastError: null,
  };
}

function save(r: AcceptRecord): AcceptRecord {
  try {
    localStorage.setItem(KEY, JSON.stringify(r));
  } catch {
    /* 忽略 */
  }
  return r;
}

export function resetAccept(): AcceptRecord {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
  return empty();
}

export function setMode(mode: AcceptMode): AcceptRecord {
  return save({ ...loadAccept(), mode, lastError: null });
}

export function setDraft(text: string, clientOperationId?: string): AcceptRecord {
  const r = loadAccept();
  return save({
    ...r,
    text,
    clientOperationId: clientOperationId ?? r.clientOperationId,
    lastError: null,
  });
}

/**
 * 提交受理。`dropReceipt`：服务端已记一笔，但客户端收不到 202 → unknown。
 * 同操作 ID + 同正文 → 不增加派发；同 ID 不同正文 → 409 conflict。
 */
export function submitAccept(opts: { dropReceipt?: boolean } = {}): AcceptRecord {
  const r = loadAccept();
  if (!r.text.trim()) {
    return save({ ...r, phase: "failed", lastError: "正文为空" });
  }

  if (r.requestId && r.committedText !== null) {
    if (r.text === r.committedText) {
      return save({ ...r, phase: "accepted", lastError: null });
    }
    return save({
      ...r,
      phase: "conflict",
      lastError: "409 操作 ID 已用于不同正文",
    });
  }

  const requestId = `req-${Date.now()}`;
  const messageId = `msg-${Date.now()}`;
  const base = {
    ...r,
    requestId,
    messageId,
    committedText: r.text,
    dispatchCount: r.dispatchCount + 1,
  };

  if (opts.dropReceipt) {
    return save({
      ...base,
      phase: "unknown",
      lastError: "受理回执丢失，请回读确认",
    });
  }

  return save({ ...base, phase: "accepted", lastError: null });
}

/** 现有模式：先回读 GET /requests/{id}。 */
export function readBack(): AcceptRecord {
  const r = loadAccept();
  if (!r.requestId) {
    return save({ ...r, phase: "failed", lastError: "无可回读的 request_id" });
  }
  return save({ ...r, phase: "accepted", lastError: null });
}

/** 可靠模式可用原操作 ID 重试；legacy + unknown 必须先回读。 */
export function retrySameOp(): AcceptRecord {
  const r = loadAccept();
  if (r.mode === "legacy" && r.phase === "unknown") {
    return save({ ...r, lastError: "现有模式请先回读确认" });
  }
  if (r.phase === "conflict") return r;
  return submitAccept({ dropReceipt: false });
}
