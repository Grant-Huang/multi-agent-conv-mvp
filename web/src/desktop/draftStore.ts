/** 草稿按 Conversation / Task 隔离（A2）；T-84 起服务端权威 + 本地缓存镜像，见 DESIGN §3。 */
import { api } from "../api";

export type DraftScope = "auto" | "continue" | "independent";

export type DraftRecord = {
  text: string;
  /** 每次编辑递增；发送时钉住该版本，确认后只清该版本 */
  version: number;
  scope: DraftScope;
  /** 继续当前任务时固定的 Task ID */
  continueTaskId: string | null;
};

const PREFIX = "an.desktop.draft.v1:";

export function draftStorageKey(kind: "task" | "conv", id: string): string {
  return `${PREFIX}${kind}:${id}`;
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
  const next: DraftRecord = {
    text: patch.text !== undefined ? patch.text : prev.text,
    version: patch.text !== undefined && patch.text !== prev.text ? prev.version + 1 : (patch.version ?? prev.version),
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

const LINK_KEY = "an.desktop.taskLinks.v1";

export type TaskLinkAudit = {
  id: string;
  taskId: string;
  source: string;
  action: "link" | "revoke";
  at: number;
};

export function listTaskLinkAudits(): TaskLinkAudit[] {
  try {
    const raw = localStorage.getItem(LINK_KEY);
    return raw ? (JSON.parse(raw) as TaskLinkAudit[]) : [];
  } catch {
    return [];
  }
}

export function appendTaskLinkAudit(entry: Omit<TaskLinkAudit, "id" | "at">): TaskLinkAudit {
  const full: TaskLinkAudit = { ...entry, id: `link-${Date.now()}`, at: Date.now() };
  const all = [...listTaskLinkAudits(), full];
  try {
    localStorage.setItem(LINK_KEY, JSON.stringify(all.slice(-50)));
  } catch {
    /* 忽略 */
  }
  return full;
}

// ===== T-84 草稿上云：服务端权威 + 本地缓存镜像 =====

/** 每个草稿键最近一次确认的服务端版本（expected_version 防丢失更新） */
const serverVersion = new Map<string, number>();
const syncTimers = new Map<string, ReturnType<typeof setTimeout>>();

function parseDraftKey(key: string): { kind: "task" | "conv"; id: string } | null {
  const m = key.match(/^an\.desktop\.draft\.v1:(task|conv):(.+)$/);
  if (!m) return null;
  return { kind: m[1] as "task" | "conv", id: m[2] };
}

/** 打开工作区时：以服务端为准刷新本地缓存。合并口径（版本号跨会话不可比）：
 *  - 本会话从未同步过该键且本地已有输入 → 本地是更新的意图，立即回推服务端；
 *  - 本地为空 → 采纳服务端草稿（跨设备/清存储恢复）；
 *  - 本会话已同步过 → 版本新者胜。
 *  服务端不可达时保留本地——那是用户自己的草稿数据，不是 Mock 回退（宪章原则 8）。 */
export async function loadDraftRemote(key: string): Promise<DraftRecord> {
  const k = parseDraftKey(key);
  if (!k) return loadDraft(key);
  try {
    const r = await api.desktopDraftGet(k.kind, k.id);
    const everSynced = serverVersion.has(key);
    const local = loadDraft(key);
    const localTyped = local.text.length > 0 && local.version > 0;
    if (!everSynced) {
      if (localTyped) {
        serverVersion.set(key, r.draft.version);
        scheduleDraftSync(key, 0);
        return loadDraft(key);
      }
      if (r.draft.version > 0 || r.draft.text) {
        saveDraft(key, {
          text: r.draft.text,
          version: r.draft.version,
          scope: (r.draft.scope as DraftScope) || "auto",
          continueTaskId: r.draft.continue_task_id,
        });
      }
      serverVersion.set(key, r.draft.version);
      return loadDraft(key);
    }
    if (r.draft.version >= local.version) {
      saveDraft(key, {
        text: r.draft.text,
        version: r.draft.version,
        scope: (r.draft.scope as DraftScope) || "auto",
        continueTaskId: r.draft.continue_task_id,
      });
    } else {
      scheduleDraftSync(key, 0);
    }
    serverVersion.set(key, r.draft.version);
  } catch {
    /* 服务端不可达：保留本地缓存 */
  }
  return loadDraft(key);
}

/** 编辑后防抖推送到服务端（expected_version 防丢失更新）；
 *  失败/冲突时回读服务端，服务端版本较新则覆盖本地。 */
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
  const expected = serverVersion.get(key);
  try {
    const r = await api.desktopDraftSave(k.kind, k.id, {
      text: rec.text,
      scope: rec.scope,
      continue_task_id: rec.continueTaskId,
      version: rec.version,
      expected_version: expected,
    });
    serverVersion.set(key, r.draft.version);
  } catch {
    try {
      const g = await api.desktopDraftGet(k.kind, k.id);
      serverVersion.set(key, g.draft.version);
      const local = loadDraft(key);
      if (g.draft.version > local.version) {
        saveDraft(key, {
          text: g.draft.text,
          version: g.draft.version,
          scope: (g.draft.scope as DraftScope) || "auto",
          continueTaskId: g.draft.continue_task_id,
        });
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
  const rec = loadDraft(key);
  void api
    .desktopDraftClearAfterSend(k.kind, k.id, sentVersion)
    .then((r) => {
      serverVersion.set(key, r.draft.version);
      if (rec.text) scheduleDraftSync(key);
    })
    .catch(() => {
      /* 离线：本地清空已生效，恢复联网后由下次编辑推送 */
    });
}
