/** 草稿按 Conversation / Task 隔离（A2）；T-84 起服务端权威 + 本地缓存镜像，见 DESIGN §3。
 *  UXFIX-D（#302）：作用域分存（任务 / 独立话题）、跨会话冲突保留本地并提示、
 *  推送必带 expected_version（INTENT-2026-008/011/013，宪章 CH-U2）。 */
import { api } from "../api";

export type DraftScope = "auto" | "continue" | "independent";

/** 作用域变体：auto/continue 共用任务主草稿；independent 独立分存（INTENT-013） */
export type DraftVariant = "base" | "independent";

export type DraftRecord = {
  text: string;
  /** 每次编辑递增；发送时钉住该版本，确认后只清该版本 */
  version: number;
  scope: DraftScope;
  /** 继续当前任务时固定的 Task ID */
  continueTaskId: string | null;
};

/** 跨会话草稿冲突（任何一方的文字都不得无提示丢失，CH-U2） */
export type DraftConflict = {
  /** 另一方（服务端快照）的正文 */
  serverText: string;
  serverVersion: number;
  /** kept_local：本机未同步输入优先，服务端草稿可一键改用；adopted_remote：已按版本采纳服务端草稿 */
  kind: "kept_local" | "adopted_remote";
};

const PREFIX = "an.desktop.draft.v1:";
const INDEP_SUFFIX = "@independent";

/** 独立话题作用域在服务端的 scope_id 后缀（URL 安全字符；客户端写路径时编码） */
export function draftServerId(id: string, variant: DraftVariant): string {
  return variant === "independent" ? `${id}${INDEP_SUFFIX}` : id;
}

export function draftStorageKey(kind: "task" | "conv", id: string, variant: DraftVariant = "base"): string {
  return `${PREFIX}${kind}:${draftServerId(id, variant)}`;
}

export function loadDraft(key: string): DraftRecord {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return emptyDraft();
    const parsed = JSON.parse(raw) as Partial<DraftRecord>;
    return {
      text: typeof parsed.text === "string" ? parsed.text : "",
      version: typeof parsed.version === "number" ? parsed.version : 0,
      scope: parsed.scope === "continue" || parsed.scope === "independent" ? parsed.scope : "auto",
      continueTaskId: typeof parsed.continueTaskId === "string" ? parsed.continueTaskId : null,
    };
  } catch {
    return emptyDraft();
  }
}

export function saveDraft(key: string, patch: Partial<DraftRecord> & { text?: string }): DraftRecord {
  const prev = loadDraft(key);
  const textChanged = patch.text !== undefined && patch.text !== prev.text;
  if (textChanged) markLocalEdit(key);
  const next: DraftRecord = {
    text: patch.text !== undefined ? patch.text : prev.text,
    version: textChanged ? prev.version + 1 : (patch.version ?? prev.version),
    scope: patch.scope ?? prev.scope,
    continueTaskId: patch.continueTaskId !== undefined ? patch.continueTaskId : prev.continueTaskId,
  };
  try {
    localStorage.setItem(key, JSON.stringify(next));
  } catch {
    /* 忽略 */
  }
  return next;
}

/** 仅当仍是发送时钉住的版本才清空正文（等待期间追加的输入保留） */
export function clearDraftAfterSend(key: string, sentVersion: number): DraftRecord {
  const prev = loadDraft(key);
  if (prev.version !== sentVersion) return prev;
  return saveDraft(key, { text: "", version: sentVersion + 1 });
}

function emptyDraft(): DraftRecord {
  return { text: "", version: 0, scope: "auto", continueTaskId: null };
}

export type TaskLinkAudit = {
  id: string;
  taskId: string;
  source: string;
  action: "link" | "revoke";
  at: string;
};

// ===== T-99 关联修正审计（服务端权威；按 task 过滤，created_at 升序） =====

/** 列表某任务的审计。失败时抛错，由调用方展示（原则 3：审计不静默丢弃）。 */
export async function listTaskLinkAudits(taskId: string): Promise<TaskLinkAudit[]> {
  const r = await api.desktopLinkAuditsList(taskId);
  return r.items.map((a) => ({
    id: a.id,
    taskId: a.task_id,
    source: a.source,
    action: a.action as TaskLinkAudit["action"],
    at: a.created_at,
  }));
}

/** 追加一条审计（服务端生成 id 与时间）。失败时抛错，由调用方展示。 */
export async function appendTaskLinkAudit(
  taskId: string,
  entry: { source: string; action: TaskLinkAudit["action"] },
): Promise<void> {
  await api.desktopLinkAuditCreate(taskId, entry);
}

// ===== T-84 草稿上云：服务端权威 + 本地缓存镜像 =====
// UXFIX-D 并发口径：
//  - R1 服务端版本单调：已知最新服务端版本之后的过期快照直接丢弃（版本号跨会话不可比，
//    只用于判“同一数据的新旧”，不用于判“本会话输入与服务端谁新”）。
//  - R2 本会话有未确认的本地编辑时， differing 的远端快照不覆盖本地，保留本地并提示冲突，
//    同时立即推送本地（带 expected_version）；“任何一方文字不得无提示丢失”。
//  - R3 推送必带 expected_version；未取得版本先 GET。409 冲突保留本地并提示，不静默改写。

/** 每个草稿键已知的最新服务端版本（单调不回退） */
const serverVersion = new Map<string, number>();
const syncTimers = new Map<string, ReturnType<typeof setTimeout>>();
/** 本会话每次本地文字编辑递增；确认（推送成功 / 采纳远端 / 发送清理）后记录已确认位置 */
const editSeq = new Map<string, number>();
const confirmedSeq = new Map<string, number>();
const conflictListeners = new Map<string, Set<(c: DraftConflict) => void>>();

function markLocalEdit(key: string): void {
  editSeq.set(key, (editSeq.get(key) ?? 0) + 1);
}

function isUnconfirmed(key: string): boolean {
  return (editSeq.get(key) ?? 0) > (confirmedSeq.get(key) ?? 0);
}

function markConfirmed(key: string): void {
  confirmedSeq.set(key, editSeq.get(key) ?? 0);
}

/** 订阅某草稿键的冲突提示（用于界面提示“保留本机内容 / 可改用其他设备版本”） */
export function onDraftConflict(key: string, cb: (c: DraftConflict) => void): () => void {
  let set = conflictListeners.get(key);
  if (!set) {
    set = new Set();
    conflictListeners.set(key, set);
  }
  set.add(cb);
  return () => {
    set?.delete(cb);
  };
}

function notifyConflict(key: string, c: DraftConflict): void {
  conflictListeners.get(key)?.forEach((cb) => cb(c));
}

function parseDraftKey(key: string): { kind: "task" | "conv"; id: string; variant: DraftVariant } | null {
  const m = key.match(/^an\.desktop\.draft\.v1:(task|conv):(.+)$/);
  if (!m) return null;
  const id = m[2];
  if (id.endsWith(INDEP_SUFFIX)) {
    return { kind: m[1] as "task" | "conv", id: id.slice(0, -INDEP_SUFFIX.length), variant: "independent" };
  }
  return { kind: m[1] as "task" | "conv", id, variant: "base" };
}

function applyRemote(key: string, text: string, version: number, scope: string, continueTaskId: string | null): void {
  saveDraft(key, {
    text,
    version,
    scope: (scope as DraftScope) || "auto",
    continueTaskId,
  });
}

/** 打开工作区时：以服务端为准刷新本地缓存。返回最终草稿与冲突信息（无冲突为 null）。
 *  合并口径见文件头 R1～R3；服务端不可达时保留本地——那是用户自己的草稿数据，
 *  不是 Mock 回退（宪章原则 8）。 */
export async function loadDraftRemote(key: string): Promise<{ draft: DraftRecord; conflict: DraftConflict | null }> {
  const k = parseDraftKey(key);
  if (!k) return { draft: loadDraft(key), conflict: null };
  try {
    const r = await api.desktopDraftGet(k.kind, draftServerId(k.id, k.variant));
    const known = serverVersion.get(key);
    // R1：过期快照（版本落后于已知服务端版本）直接丢弃
    if (known !== undefined && r.draft.version < known) {
      return { draft: loadDraft(key), conflict: null };
    }
    serverVersion.set(key, r.draft.version);
    const local = loadDraft(key);
    const localTyped = local.text.length > 0;
    const differs = localTyped && local.text !== r.draft.text;
    if (differs && isUnconfirmed(key)) {
      // R2：快照可能不含本会话最新输入——保留本地，提示冲突，并立即推送
      const conflict: DraftConflict = {
        serverText: r.draft.text,
        serverVersion: r.draft.version,
        kind: "kept_local",
      };
      notifyConflict(key, conflict);
      scheduleDraftSync(key, 0);
      return { draft: local, conflict };
    }
    if (r.draft.version > 0 || r.draft.text) {
      if (!differs && !localTyped) {
        // 本地为空：采纳服务端草稿（跨设备 / 清存储恢复）
        applyRemote(key, r.draft.text, r.draft.version, r.draft.scope, r.draft.continue_task_id);
        markConfirmed(key);
        return { draft: loadDraft(key), conflict: null };
      }
      if (differs) {
        // 本会话已确认同步后远端更新：按版本采纳，同时提示被替换的本机内容
        const displaced = local.text;
        applyRemote(key, r.draft.text, r.draft.version, r.draft.scope, r.draft.continue_task_id);
        markConfirmed(key);
        const conflict: DraftConflict = {
          serverText: displaced,
          serverVersion: r.draft.version,
          kind: "adopted_remote",
        };
        notifyConflict(key, conflict);
        return { draft: loadDraft(key), conflict };
      }
    }
    markConfirmed(key);
  } catch {
    /* 服务端不可达：保留本地缓存 */
  }
  return { draft: loadDraft(key), conflict: null };
}

/** 编辑后防抖推送到服务端（expected_version 防丢失更新） */
export function scheduleDraftSync(key: string, delayMs = 500): void {
  if (!parseDraftKey(key)) return;
  const prev = syncTimers.get(key);
  if (prev) clearTimeout(prev);
  syncTimers.set(
    key,
    setTimeout(() => {
      syncTimers.delete(key);
      void pushDraft(key);
    }, delayMs),
  );
}

async function pushDraft(key: string): Promise<void> {
  const k = parseDraftKey(key);
  if (!k) return;
  const rec = loadDraft(key);
  let expected = serverVersion.get(key);
  if (expected === undefined) {
    // R3：未取得服务端版本先读取，不得无前置条件覆盖其他会话的草稿
    try {
      const g = await api.desktopDraftGet(k.kind, draftServerId(k.id, k.variant));
      const known = serverVersion.get(key);
      if (known !== undefined && g.draft.version < known) return; // 过期快照
      serverVersion.set(key, g.draft.version);
      expected = g.draft.version;
    } catch {
      return; // 服务端不可达：保留本地，等待下次编辑再推
    }
  }
  try {
    const r = await api.desktopDraftSave(k.kind, draftServerId(k.id, k.variant), {
      text: rec.text,
      scope: rec.scope,
      continue_task_id: rec.continueTaskId,
      version: rec.version,
      expected_version: expected,
    });
    serverVersion.set(key, r.draft.version);
    markConfirmed(key);
  } catch {
    // 409 / 失败：回读服务端版本；保留本地文字并提示，不静默改写（R3）
    try {
      const g = await api.desktopDraftGet(k.kind, draftServerId(k.id, k.variant));
      const known = serverVersion.get(key);
      if (known !== undefined && g.draft.version < known) return;
      serverVersion.set(key, g.draft.version);
      const local = loadDraft(key);
      if (local.text && g.draft.text !== local.text) {
        const conflict: DraftConflict = {
          serverText: g.draft.text,
          serverVersion: g.draft.version,
          kind: "kept_local",
        };
        notifyConflict(key, conflict);
      } else if (!local.text || g.draft.text === local.text) {
        // 内容一致（或本地为空）：只对齐版本，视为已确认
        if (g.draft.text === local.text && local.text) markConfirmed(key);
      }
    } catch {
      /* 服务端不可达：保留本地 */
    }
  }
}

/** 发送后：按 sent_version 本地规则清空 + 同步服务端（语义一致，幂等）。 */
export function clearDraftAfterSendRemote(key: string, sentVersion: number): void {
  const k = parseDraftKey(key);
  clearDraftAfterSend(key, sentVersion);
  if (!k) return;
  markConfirmed(key);
  const rec = loadDraft(key);
  void api
    .desktopDraftClearAfterSend(k.kind, draftServerId(k.id, k.variant), sentVersion)
    .then((r) => {
      serverVersion.set(key, r.draft.version);
      if (rec.text) scheduleDraftSync(key);
    })
    .catch(() => {
      /* 离线：本地清空已生效，恢复联网后由下次编辑推送 */
    });
}

/** 仅测试：清空模块级同步状态（版本表、防抖计时器、编辑序号与订阅） */
export function __resetDraftStoreForTest(): void {
  serverVersion.clear();
  editSeq.clear();
  confirmedSeq.clear();
  conflictListeners.clear();
  syncTimers.forEach((t) => clearTimeout(t));
  syncTimers.clear();
}
