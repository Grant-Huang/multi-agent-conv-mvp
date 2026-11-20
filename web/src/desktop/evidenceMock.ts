import { SMT03_TASK_ID } from "./mockData";

export type EvidenceStatus = "ok" | "empty" | "failed" | "expired";

export type EvidenceRecord = {
  id: string;
  taskId: string;
  title: string;
  objectLabel: string;
  collectedAt: string;
  sourceType: string;
  version: string;
  status: EvidenceStatus;
  summary: string;
  sourceBody: string;
  errorMessage?: string;
};

/** SMT-03 质量运营可读模拟记录（DESIGN §6） */
export const SMT03_EVIDENCE: EvidenceRecord[] = [
  {
    id: "ev-smt-qc-1",
    taskId: SMT03_TASK_ID,
    title: "首件检验记录",
    objectLabel: "质量批次 SMT-03",
    collectedAt: "2026-11-20T09:12:00-08:00",
    sourceType: "质检系统（模拟）",
    version: "v3",
    status: "ok",
    summary: "事实：尺寸抽检 12/12 合格。推断：工艺窗口稳定。待核实：供应商批次一致性。",
    sourceBody: "【模拟原文】LOT=SMT-03-Q · Cpk=1.42 · inspector=fixture-qa",
  },
  {
    id: "ev-smt-fail",
    taskId: SMT03_TASK_ID,
    title: "影像比对报告",
    objectLabel: "质量批次 SMT-03",
    collectedAt: "2026-11-20T08:01:00-08:00",
    sourceType: "视觉检测（模拟）",
    version: "v1",
    status: "failed",
    summary: "加载失败夹具",
    sourceBody: "",
    errorMessage: "证据服务暂时不可用（模拟失败）",
  },
  {
    id: "ev-smt-exp",
    taskId: SMT03_TASK_ID,
    title: "过期工艺参数快照",
    objectLabel: "质量批次 SMT-03",
    collectedAt: "2026-10-01T10:00:00-08:00",
    sourceType: "MES 快照（模拟）",
    version: "v0",
    status: "expired",
    summary: "该快照已过期，不可作为核验依据",
    sourceBody: "【过期】请重新采集",
    errorMessage: "证据已过期，请刷新采集",
  },
];

export function evidenceForTask(taskId: string, includeEmptyDemo = false): EvidenceRecord[] {
  const list = SMT03_EVIDENCE.filter((e) => e.taskId === taskId);
  if (includeEmptyDemo && list.length === 0) return [];
  return list;
}

export type HistoryItem = {
  id: string;
  kind: "task" | "conversation";
  title: string;
  objectLabel: string;
  updatedAt: string;
  taskId?: string;
  reason?: string;
};

export const HISTORY_SEED: HistoryItem[] = [
  {
    id: "hist-smt",
    kind: "task",
    title: "SMT-03 质量调查",
    objectLabel: "质量批次 SMT-03",
    updatedAt: "2026-11-20T09:30:00-08:00",
    taskId: SMT03_TASK_ID,
    reason: "与你负责的质量目标相关",
  },
  {
    id: "hist-prod",
    kind: "task",
    title: "产品改进跟踪",
    objectLabel: "产品路线图",
    updatedAt: "2026-11-19T16:00:00-08:00",
    taskId: "TASK-2026-021",
    reason: "Agent 正在汇总中",
  },
  {
    id: "hist-chat",
    kind: "conversation",
    title: "独立轻量会话",
    objectLabel: "个人",
    updatedAt: "2026-11-18T11:00:00-08:00",
  },
];

const PREF_KEY = "an.desktop.historyPrefs.v1";

export type HistoryPrefs = {
  pinned: string[];
  hidden: string[];
  watched: string[];
  filter: string;
};

export function loadHistoryPrefs(): HistoryPrefs {
  try {
    const raw = localStorage.getItem(PREF_KEY);
    if (!raw) return { pinned: [], hidden: [], watched: [], filter: "" };
    const p = JSON.parse(raw) as Partial<HistoryPrefs>;
    return {
      pinned: p.pinned ?? [],
      hidden: p.hidden ?? [],
      watched: p.watched ?? [],
      filter: p.filter ?? "",
    };
  } catch {
    return { pinned: [], hidden: [], watched: [], filter: "" };
  }
}

export function saveHistoryPrefs(prefs: HistoryPrefs) {
  try {
    localStorage.setItem(PREF_KEY, JSON.stringify(prefs));
  } catch {
    /* 忽略 */
  }
}
