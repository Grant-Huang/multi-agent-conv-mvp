/** 动作审批 Mock（A5.2 / AD-APPROVE）；版本绑定 + 副作用恰好一次。 */

export type ApprovalStatus = "pending" | "approved" | "rejected" | "expired" | "unknown";

export type ApprovalRecord = {
  id: string;
  taskId: string;
  planRevision: number;
  params: string;
  status: ApprovalStatus;
  decidedBy: string | null;
  executeCount: number;
  lastReceipt: "none" | "applied" | "unknown" | "failed";
};

const KEY = "an.desktop.approvalMock.v1:";

export function approvalKey(taskId: string): string {
  return KEY + taskId;
}

export function loadApproval(taskId: string): ApprovalRecord {
  try {
    const raw = localStorage.getItem(approvalKey(taskId));
    if (raw) return JSON.parse(raw) as ApprovalRecord;
  } catch {
    /* 忽略 */
  }
  return {
    id: `appr-${taskId}`,
    taskId,
    planRevision: 1,
    params: "停机检修 · 备用机切换",
    status: "pending",
    decidedBy: null,
    executeCount: 0,
    lastReceipt: "none",
  };
}

function save(rec: ApprovalRecord): ApprovalRecord {
  try {
    localStorage.setItem(approvalKey(rec.taskId), JSON.stringify(rec));
  } catch {
    /* 忽略 */
  }
  return rec;
}

export function bumpParams(taskId: string, nextParams: string): ApprovalRecord {
  const cur = loadApproval(taskId);
  return save({
    ...cur,
    params: nextParams,
    planRevision: cur.planRevision + 1,
    status: "pending",
    decidedBy: null,
    lastReceipt: "none",
  });
}

export function decide(
  taskId: string,
  decision: "approve" | "reject",
  expectedRevision: number,
): { ok: true; record: ApprovalRecord } | { ok: false; record: ApprovalRecord; reason: string } {
  const cur = loadApproval(taskId);
  if (cur.planRevision !== expectedRevision) {
    return { ok: false, record: cur, reason: "方案版本已变更，原审批失效，请重新确认" };
  }
  if (decision === "reject") {
    return { ok: true, record: save({ ...cur, status: "rejected", decidedBy: "supervisor" }) };
  }
  return { ok: true, record: save({ ...cur, status: "approved", decidedBy: "supervisor" }) };
}

export function expire(taskId: string): ApprovalRecord {
  const cur = loadApproval(taskId);
  return save({ ...cur, status: "expired", decidedBy: cur.decidedBy });
}

/** 执行：仅 approved 且版本匹配；幂等键使副作用只计一次；可注入 unknown 回执。 */
export function executeAction(
  taskId: string,
  expectedRevision: number,
  opts?: { forceUnknown?: boolean },
): { ok: true; record: ApprovalRecord } | { ok: false; record: ApprovalRecord; reason: string } {
  const cur = loadApproval(taskId);
  if (cur.status === "rejected" || cur.status === "expired") {
    return { ok: false, record: cur, reason: "审批已拒绝或过期，不能执行" };
  }
  if (cur.status !== "approved") {
    return { ok: false, record: cur, reason: "尚未批准，不能执行" };
  }
  if (cur.planRevision !== expectedRevision) {
    return { ok: false, record: cur, reason: "方案版本已变更，原审批失效，请重新确认" };
  }
  if (opts?.forceUnknown) {
    return { ok: true, record: save({ ...cur, lastReceipt: "unknown" }) };
  }
  if (cur.executeCount > 0 && cur.lastReceipt === "applied") {
    return { ok: true, record: cur };
  }
  return {
    ok: true,
    record: save({ ...cur, executeCount: cur.executeCount + 1, lastReceipt: "applied" }),
  };
}

export function acknowledgeUnknown(taskId: string): ApprovalRecord {
  const cur = loadApproval(taskId);
  if (cur.lastReceipt !== "unknown") return cur;
  return save({ ...cur, lastReceipt: "applied", executeCount: Math.max(1, cur.executeCount) });
}

export function resetApproval(taskId: string): ApprovalRecord {
  try {
    localStorage.removeItem(approvalKey(taskId));
  } catch {
    /* 忽略 */
  }
  return loadApproval(taskId);
}
