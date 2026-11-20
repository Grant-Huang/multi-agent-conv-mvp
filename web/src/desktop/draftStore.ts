/** 草稿按 Conversation / Task 隔离（A2）；首版本地命名空间，见 DESIGN §3。 */
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
