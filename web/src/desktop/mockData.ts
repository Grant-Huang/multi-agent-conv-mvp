/** A 阶段桌面 Mock 种子（M1）；SMT-03 允许纯 Mock，见决定 61 / MOCK_REGISTRY。 */
export const SMT03_TASK_ID = "TASK-2026-018";

/** 夹具任务集合（UXFIX-E）：只有这些任务在工作区显示模拟内容并标“模拟”；
 *  其余任务一律按 Task ID 读真实任务（CH-P8）。 */
export const FIXTURE_TASK_IDS: ReadonlySet<string> = new Set([SMT03_TASK_ID, "TASK-2026-021"]);

export type DesktopCard = {
  id: string;
  zone: "mine" | "agent" | "watch";
  title: string;
  subtitle: string;
  entityId: string;
  badge?: string;
  simulated: boolean;
};

export const DESKTOP_CARDS: DesktopCard[] = [
  {
    id: "card-smt-03",
    zone: "mine",
    title: "SMT-03 质量调查",
    subtitle: "待核验证据 · 查看工作区",
    entityId: SMT03_TASK_ID,
    badge: "我的工作",
    simulated: true,
  },
  {
    id: "card-product",
    zone: "agent",
    title: "产品改进跟踪",
    subtitle: "Agent 汇总中",
    entityId: "TASK-2026-021",
    badge: "Agent 工作",
    simulated: true,
  },
  {
    id: "card-watch",
    zone: "watch",
    title: "产线风险提示",
    subtitle: "为你关注 · 模拟",
    entityId: "WATCH-001",
    badge: "为你关注",
    simulated: true,
  },
];

const LAYOUT_KEY = "an.desktop.workspaceLayout";
const VIEW_KEY = "an.desktop.mainView";
const TASK_KEY = "an.desktop.workspaceTaskId";

export type WorkspaceLayout = "s2" | "s3";
export type DesktopMainView =
  | "home"
  | "chat"
  | "desk"
  | "workspace"
  | "des-experiment"
  | "mom-workbench";

export function readWorkspaceLayout(): WorkspaceLayout {
  try {
    const v = localStorage.getItem(LAYOUT_KEY);
    return v === "s3" ? "s3" : "s2";
  } catch {
    return "s2";
  }
}

export function writeWorkspaceLayout(layout: WorkspaceLayout) {
  try {
    localStorage.setItem(LAYOUT_KEY, layout);
  } catch {
    /* 偏好 */
  }
}

export function readDesktopView(): { view: DesktopMainView; taskId: string | null } {
  try {
    const view = (localStorage.getItem(VIEW_KEY) as DesktopMainView | null) ?? "home";
    const taskId = localStorage.getItem(TASK_KEY);
    if (view === "workspace" && taskId) return { view, taskId };
    if (
      view === "chat" ||
      view === "desk" ||
      view === "home" ||
      view === "des-experiment" ||
      view === "mom-workbench"
    ) {
      return { view, taskId: null };
    }
    return { view: "home", taskId: null };
  } catch {
    return { view: "home", taskId: null };
  }
}

export function writeDesktopView(view: DesktopMainView, taskId: string | null = null) {
  try {
    localStorage.setItem(VIEW_KEY, view);
    if (view === "workspace" && taskId) localStorage.setItem(TASK_KEY, taskId);
    else localStorage.removeItem(TASK_KEY);
  } catch {
    /* 偏好 */
  }
}
