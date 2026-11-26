// AgentNexus：组织级 AI 桌面壳（V1.7 A1）；会话/运营为壳内模块。
import { useCallback, useEffect, useRef, useState } from "react";
import { api, subscribeEvents, type Health } from "./api";
import type { TaskEvent } from "./bindings/TaskEvent";
import { createDeltaBuffer, createEventPump, REFRESH_INTERVAL_MS } from "./refresh";
import type { Conversation } from "./bindings/Conversation";
import type { MessageView } from "./bindings/MessageView";
import type { TaskRecord } from "./bindings/TaskRecord";
import { ChatView, type TaskActionKind } from "./components/ChatView";
import { RouteLogPanel } from "./components/RouteLogPanel";
import { TaskPanel, taskMessageIds } from "./components/TaskPanel";
import { NodesPanel } from "./components/NodesPanel";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { AgentsPanel } from "./components/AgentsPanel";
import { OpsDesk } from "./components/OpsDesk";
import { IntrospectionPanel } from "./components/IntrospectionPanel";
import { DesktopHome } from "./desktop/DesktopHome";
import { DesExperiment } from "./desktop/DesExperiment";
import { MomWorkbench } from "./desktop/MomWorkbench";
import { WorkspaceView } from "./desktop/WorkspaceView";
import { AssistantChatPanel } from "./desktop/AssistantChatPanel";
import { EvidencePanel, type EvidenceLayer } from "./desktop/EvidencePanel";
import { HistoryPanel } from "./desktop/HistoryPanel";
import { SettingsPanel, type SettingsLayer } from "./desktop/SettingsPanel";
import { fetchProfile, loadProfile, type AssistantProfile, type SettingsCategoryId } from "./desktop/profileStore";
import { AttentionToast, type AttentionItem } from "./desktop/AttentionToast";
import { AssistantReadPanel } from "./desktop/AssistantReadPanel";
import type { S1BgJob } from "./desktop/S1AskBar";
import { durableHomeAccept, waitForConversationTask } from "./desktop/homeDelegate";
import {
  SMT03_TASK_ID,
  readDesktopView,
  writeDesktopView,
  type DesktopMainView,
} from "./desktop/mockData";
import { roleLabel } from "./labels";

type MainView = DesktopMainView;

type DesktopSide =
  | null
  | { kind: "tasks" }
  | { kind: "evidence"; taskId: string; layer: EvidenceLayer; evidenceId: string | null; mode?: "normal" | "empty" }
  | { kind: "history" }
  | {
      kind: "settings";
      layer: SettingsLayer;
      categoryId: SettingsCategoryId | null;
      memoryId: string | null;
      search: string;
    };

/** 每个任务保留的进度条目数，超过后丢弃最早的 */
const MAX_PROGRESS_LINES = 30;
/** 开发者模式的本地开关键（只是界面偏好，不影响数据） */
const DEV_MODE_KEY = "agentnexus.devMode";

type SideTab = "tasks" | "agents" | "nodes" | "diag" | "logs" | "intro";

/** 追加一条进度：与上一条相同则忽略，超出上限丢弃最早的 */
function appendLine(list: string[], msg: string): string[] {
  const text = msg.trim();
  if (!text || list[list.length - 1] === text) return list;
  return [...list, text].slice(-MAX_PROGRESS_LINES);
}

/** 读取开发者模式开关；localStorage 不可用时（隐私窗口等）静默退化为关闭 */
function readDevMode(): boolean {
  try {
    return localStorage.getItem(DEV_MODE_KEY) === "1";
  } catch {
    return false;
  }
}

function writeDevMode(on: boolean) {
  try {
    localStorage.setItem(DEV_MODE_KEY, on ? "1" : "0");
  } catch {
    /* 忽略：只是偏好设置 */
  }
}

/** 记住当前会话，刷新后回到同一个会话（而不是列表第一项）；存储不可用时退化为默认行为 */
const CURRENT_CONV_KEY = "an.currentConversation";
function readCurrentConv(): string | null {
  try {
    return localStorage.getItem(CURRENT_CONV_KEY);
  } catch {
    return null;
  }
}

function writeCurrentConv(id: string | null) {
  try {
    if (id) localStorage.setItem(CURRENT_CONV_KEY, id);
  } catch {
    /* 忽略：只是便利功能 */
  }
}

/** 运营频道的固定会话 id（与后端 `OPS_CHANNEL_ID` 一致） */
const OPS_CHANNEL_ID = "ops-channel";

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  // 运营频道未读数；null 表示没有开启运营频道（[mom] chat_bridge 关闭，接口 404）
  const [opsUnread, setOpsUnread] = useState<number | null>(null);
  // 桌面主视图：默认首页；workspace 带 Mock Task（A1）
  const initialDesktop = readDesktopView();
  const [mainView, setMainViewState] = useState<MainView>(initialDesktop.view);
  const [workspaceTaskId, setWorkspaceTaskId] = useState<string | null>(initialDesktop.taskId);
  const setMainView = (view: MainView, taskId: string | null = null) => {
    setMainViewState(view);
    if (view === "workspace" && taskId) setWorkspaceTaskId(taskId);
    if (view !== "workspace") setWorkspaceTaskId(null);
    writeDesktopView(view, view === "workspace" ? taskId : null);
  };
  // 业务内省未读；null 表示接口不可用（403/失败）
  const [introUnread, setIntroUnread] = useState<number | null>(null);
  const [introMainId, setIntroMainId] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessageView[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [sideTab, setSideTab] = useState<SideTab>("tasks");
  // 开发者模式：显示诊断和路由日志页签；默认关闭，面向普通用户
  const [devMode, setDevMode] = useState<boolean>(readDevMode);
  // 窄屏下会话列表是抽屉，这里记录是否展开
  const [navOpen, setNavOpen] = useState(false);
  // 只在状态变化 / 助手消息时递增，而且做了节流；诊断页和路由日志用它刷新
  const [panelTick, setPanelTick] = useState(0);
  // 最新一条任务事件，展开的任务卡片会把它追加到时间线上
  const [incoming, setIncoming] = useState<TaskEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 正在流式生成的回答：id 等于最终消息的 id，最终消息到达后自动隐藏
  const [streaming, setStreaming] = useState<{ id: string; text: string } | null>(null);
  // 每个任务的进度历史（时间顺序）：进度条显示最后一条，展开时看全部
  const [notes, setNotes] = useState<Record<string, string[]>>({});
  const noted = useRef(new Set<string>());

  // 健康检查：启动时 + 每 10 秒（节点配对、上线、离线后左下角能及时更新）
  useEffect(() => {
    const load = () => api.health().then(setHealth).catch(() => setHealth(null));
    load();
    const t = setInterval(load, 10_000);
    return () => clearInterval(t);
  }, []);

  const loadConversations = useCallback(async () => {
    const list = await api.conversations();
    setConversations(list);
    return list;
  }, []);

  // 首次加载：优先回到上次选中的会话（刷新页面后不变）；找不到时选最近的会话
  useEffect(() => {
    loadConversations()
      .then((list) => {
        const saved = readCurrentConv();
        const pick = list.find((c) => c.id === saved)?.id ?? list[0]?.id ?? null;
        setCurrent((c) => c ?? pick);
      })
      .catch((e) => setError(String(e)));
  }, [loadConversations]);

  // 每次切换会话都记下来，供下次刷新时恢复
  useEffect(() => {
    writeCurrentConv(current);
  }, [current]);

  const sideRef = useRef(sideTab);
  sideRef.current = sideTab;
  const currentRef = useRef(current);
  currentRef.current = current;
  const streamRef = useRef("");

  const refreshConversation = useCallback(async (cid: string) => {
    const [m, t] = await Promise.all([api.messages(cid), api.tasks(cid)]);
    // 迟到响应不得覆盖当前会话视图（A2 / DESIGN §3）
    if (currentRef.current !== cid) return;
    setMessages(m);
    setTasks(t);
  }, []);

  // 切换会话：加载数据 + 订阅 SSE 事件
  useEffect(() => {
    if (!current) {
      setMessages([]);
      setTasks([]);
      return;
    }
    refreshConversation(current).catch((e) => setError(String(e)));
    const pushDelta = createDeltaBuffer((chunk) => {
      const id = streamRef.current;
      setStreaming((prev) => (prev && prev.id === id ? { id, text: prev.text + chunk } : { id, text: chunk }));
    }, (cb) => {
      requestAnimationFrame(cb);
    });
    const pump = createEventPump({
      intervalMs: REFRESH_INTERVAL_MS,
      onTasks: () => {
        const cid = currentRef.current;
        // 对话里的状态和产物也要这份列表，不能只在任务页签打开时拉
        if (cid) api.tasks(cid).then(setTasks).catch(() => {});
      },
      onMessages: () => {
        const cid = currentRef.current;
        if (cid) api.messages(cid).then(setMessages).catch(() => {});
      },
      onPanels: () => {
        if (sideRef.current === "diag" || sideRef.current === "logs") setPanelTick((n) => n + 1);
      },
      onLive: (ev) => {
        setIncoming(ev);
        if (ev.task_id && ev.message.trim() && ev.kind !== "assistant_message") {
          const id = ev.task_id;
          setNotes((n) => ({ ...n, [id]: appendLine(n[id] ?? [], ev.message) }));
        }
        if (ev.kind === "status_changed" && ev.task_id && ev.status) {
          const id = ev.task_id;
          const status = ev.status;
          setTasks((list) => list.map((t) => (t.id === id ? { ...t, status } : t)));
        }
      },
      // 增量拉取在展开的任务卡片里，按它自己的游标节流；这里不再发第二次请求
      onDetail: () => {},
      onDelta: (piece, id) => {
        streamRef.current = id;
        pushDelta(piece);
      },
    });
    return subscribeEvents(current, (ev) => {
      if (ev.kind === "assistant_message") setStreaming(null);
      pump(ev);
    });
  }, [current, refreshConversation]);

  // 切换会话时清空进度历史，之后按需重新取回
  useEffect(() => {
    noted.current = new Set();
    setNotes({});
  }, [current]);

  useEffect(() => {
    const ids = [...new Set(messages.map((m) => m.task_id).filter((id): id is string => !!id))];
    for (const id of ids) {
      if (noted.current.has(id)) continue;
      noted.current.add(id);
      api.task(id).then((d) => {
        // 已经结束的任务不会再推事件，这里一次性取回全部进度作为历史
        const lines = d.events
          .filter((e) => e.kind !== "assistant_message")
          .reduce<string[]>((list, e) => appendLine(list, e.message), []);
        if (lines.length === 0) return;
        setNotes((n) => (n[id] ? n : { ...n, [id]: lines }));
      }).catch(() => {});
    }
  }, [messages]);

  // 切回任务页签时补一次列表（对话里的状态由事件到达时刷新）
  useEffect(() => {
    if (sideTab === "tasks" && current) api.tasks(current).then(setTasks).catch(() => {});
  }, [sideTab, current]);

  // 运营频道：每 2 秒看一次未读；频道会话打开时顺带刷新消息（卡片是就地更新的，不走任务事件）
  const conversationsRef = useRef(conversations);
  conversationsRef.current = conversations;
  useEffect(() => {
    let stopped = false;
    const poll = async () => {
      try {
        const r = await api.opsUnread();
        if (stopped) return;
        setOpsUnread(r.unread);
        if (!conversationsRef.current.some((c) => c.id === OPS_CHANNEL_ID)) loadConversations().catch(() => {});
        if (currentRef.current === OPS_CHANNEL_ID) {
          api.messages(OPS_CHANNEL_ID).then((m) => !stopped && setMessages(m)).catch(() => {});
          if (opsViewed.current && r.unread > 0) {
            await api.opsRead();
            if (!stopped) setOpsUnread(0);
          }
        }
      } catch {
        // 404：没有开启运营频道，不再轮询
        if (!stopped) setOpsUnread(null);
        stopped = true;
        clearInterval(timer);
      }
    };
    const timer = setInterval(poll, 2000);
    poll();
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [loadConversations]);

  // 业务内省未读角标（挂会话列表主窗口；G-6）
  const introViewed = useRef(false);
  useEffect(() => {
    let stopped = false;
    const poll = async () => {
      try {
        const r = await api.introUnread();
        if (stopped) return;
        setIntroUnread(r.unread);
        if (r.conversation_id) {
          setIntroMainId(r.conversation_id);
          if (!conversationsRef.current.some((c) => c.id === r.conversation_id)) {
            loadConversations().catch(() => {});
          }
          if (currentRef.current === r.conversation_id) {
            api.messages(r.conversation_id).then((m) => !stopped && setMessages(m)).catch(() => {});
            if (introViewed.current && r.unread > 0) {
              await api.introRead();
              if (!stopped) setIntroUnread(0);
            }
          }
        }
      } catch {
        // 与运营频道一致：接口不存在（旧后端 / 未挂载）时停止轮询
        if (!stopped) setIntroUnread(null);
        stopped = true;
        clearInterval(timer);
      }
    };
    const timer = setInterval(poll, 2000);
    poll();
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [loadConversations]);

  const newConversation = async () => {
    const c = await api.createConversation();
    await loadConversations();
    setMainView("chat");
    setCurrent(c.id);
    setDesktopSide(null);
    setNavOpen(false);
  };

  const [desktopSide, setDesktopSide] = useState<DesktopSide>(null);
  const [assistantProfile, setAssistantProfile] = useState<AssistantProfile>(() => loadProfile());
  const [s1BgJob, setS1BgJob] = useState<S1BgJob | null>(null);
  // UXFIX-H（INTENT-015）：委派任务终结时在助理消息区提示（普通优先级，按任务去重，带“查看结果”）。
  // 无服务端存储（issue：新增存储先出方案）——本机 sessionStorage 留存，按任务去重。
  const [taskNotices, setTaskNotices] = useState<Array<{ taskId: string; title: string; status: string }>>([]);
  const [s1Reply, setS1Reply] = useState<string | null>(null);
  // UXFIX-G（INTENT-012）：「我的助理」打开可对话的个人助理面板（不再是设置表单）
  const [assistantPanelOpen, setAssistantPanelOpen] = useState(false);
  /** UXFIX-A（INTENT-001）：首页问答走真实会话；同一次驻留内沿用同一会话（DESIGN §2「下一轮沿用会话」） */
  const s1AskConvRef = useRef<string | null>(null);
  const [composingInput, setComposingInput] = useState(false);
  const [attnItems, setAttnItems] = useState<AttentionItem[]>([]);
  // UXFIX-N（INTENT-023）：注意力列表每次同步递增，驱动首页“为你关注”实时刷新
  const [attnRevision, setAttnRevision] = useState(0);
  const [attnMuted, setAttnMuted] = useState(false);
  // UXFIX-B（INTENT-003）：测试模式由服务端告知（snapshot.test_fixtures）；正常界面不渲染测试控件
  const [testFixtures, setTestFixtures] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .desktopSnapshot()
      .then((r) => {
        if (!cancelled) setTestFixtures(!!r.test_fixtures);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const apply = () => document.documentElement.dataset.reducedMotion = mq.matches ? "1" : "0";
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchProfile()
      .then((p) => {
        if (!cancelled) setAssistantProfile(p);
      })
      .catch(() => {
        /* harness 未就绪时保留默认占位 */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      api
        .desktopAttentionList()
        .then((r) => {
          if (cancelled) return;
          setAttnItems(
            r.items.map((d) => ({
              id: d.id,
              kind: d.kind,
              title: d.title,
              body: d.body,
              taskId: d.task_id ?? undefined,
            })),
          );
          setAttnMuted(r.mute_ordinary);
          setAttnRevision((v) => v + 1);
        })
        .catch(() => {
          /* harness 未就绪时忽略 */
        });
    load();
    const t = setInterval(load, 5_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, []);

  const openWorkspace = (taskId: string) => {
    setMainView("workspace", taskId);
    setNavOpen(false);
  };

  // UXFIX-H：委派任务状态轮询——终结时更新受理提示并生成助理消息通知
  useEffect(() => {
    const job = s1BgJob;
    if (!job || job.status !== "accepted" || job.taskId === "pending") return;
    let cancelled = false;
    const t = setInterval(() => {
      api
        .task(job.taskId)
        .then((r) => {
          if (cancelled) return;
          const st = r.task.status;
          if (st !== "COMPLETED" && st !== "FAILED" && st !== "NEEDS_HUMAN" && st !== "CANCELLED") return;
          setS1BgJob((prev) => (prev && prev.taskId === job.taskId ? { ...prev, status: st === "FAILED" ? "failed" : "done" } : prev));
          setTaskNotices((prev) => {
            if (prev.some((n) => n.taskId === job.taskId)) return prev;
            const next = [
              ...prev,
              {
                taskId: job.taskId,
                title: job.label,
                status: st === "COMPLETED" ? "已完成" : st === "NEEDS_HUMAN" ? "需要你处理" : st === "CANCELLED" ? "已取消" : "失败",
              },
            ];
            try {
              sessionStorage.setItem("an.desktop.tasknotice.v1", JSON.stringify(next.slice(-10)));
            } catch {
              /* 忽略 */
            }
            return next;
          });
        })
        .catch(() => {});
    }, 5_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [s1BgJob]);

  // 恢复本会话已有的任务通知（按任务去重，已在列表内不重复生成）
  useEffect(() => {
    try {
      const raw = sessionStorage.getItem("an.desktop.tasknotice.v1");
      if (raw) setTaskNotices(JSON.parse(raw) as Array<{ taskId: string; title: string; status: string }>);
    } catch {
      /* 忽略 */
    }
  }, []);

  // UXFIX-O（INTENT-024）：记录是否经场景捷径进入设置（捷径一次返回即回原工作）
  const settingsViaShortcutRef = useRef(false);
  const openSettings = (layer: SettingsLayer = "categories", categoryId: SettingsCategoryId | null = null, viaShortcut = false) => {
    settingsViaShortcutRef.current = viaShortcut;
    setDesktopSide({ kind: "settings", layer, categoryId, memoryId: null, search: "" });
  };

  const openEvidence = (taskId: string, mode: "normal" | "empty" = "normal") => {
    openWorkspace(taskId);
    setDesktopSide({ kind: "evidence", taskId, layer: "list", evidenceId: null, mode });
    setSideTab("tasks");
  };

  // 只有用户点开运营频道才算已读；页面刷新时自动回到该会话不清未读
  const opsViewed = useRef(false);
  const selectConversation = (id: string) => {
    setMainView("chat");
    setCurrent(id);
    setDesktopSide(null);
    setNavOpen(false);
    opsViewed.current = id === OPS_CHANNEL_ID;
    if (opsViewed.current && opsUnread !== null) {
      api.opsRead().then(() => setOpsUnread(0)).catch(() => {});
    }
    introViewed.current = introMainId !== null && id === introMainId;
    if (introViewed.current && introUnread !== null) {
      api.introRead().then(() => setIntroUnread(0)).catch(() => {});
    }
  };

  /** 从实验页进入运营频道时保留返回「DES 仿真实验」；从桌面进入则按运营员视图隐藏实验入口（DESISO）。 */
  const [opsFromExperiment, setOpsFromExperiment] = useState(false);
  const openOpsChannel = (_caseId: string, fromExperiment = false) => {
    setOpsFromExperiment(fromExperiment);
    // 先同步切入频道，避免 click 后立刻 reload 时仍停在实验页（DESPAUSE）
    setMainView("chat");
    setCurrent(OPS_CHANNEL_ID);
    setDesktopSide(null);
    writeCurrentConv(OPS_CHANNEL_ID);
    api.worldMock
      .ensureOpsSeed()
      .catch(() => {})
      .then(() => api.conversations())
      .then((list) => {
        setConversations(list);
        return refreshConversation(OPS_CHANNEL_ID);
      })
      .catch(() => {});
  };

  /** 发送消息：先乐观地显示用户消息，等后端返回后再整体刷新 */
  const send = async (text: string) => {
    let cid = current;
    if (!cid) {
      const c = await api.createConversation();
      cid = c.id;
      setCurrent(cid);
    }
    const optimistic: MessageView = {
      card: null,
      id: `local-${Date.now()}`,
      conversation_id: cid,
      role: "user",
      content: text,
      task_id: null,
      created_at: new Date().toISOString(),
    };
    setMessages((m) => [...m, optimistic]);
    setStreaming(null);
    try {
      await api.send(cid, text);
      setError(null);
    } catch (e) {
      setError(`发送失败：${e}`);
    } finally {
      await refreshConversation(cid);
      loadConversations().catch(() => {});
    }
  };

  /**
   * 批准 / 验收 / 取消任务：对话区和任务面板共用这一个入口。
   * 失败时显示在顶部错误栏（行内），不使用 alert；成功后刷新会话，让两处同时更新。
   */
  const taskAction = useCallback(async (kind: TaskActionKind, taskId: string) => {
    const label =
      kind === "approve" ? "批准"
      : kind === "accept_review" ? "判定通过"
      : kind === "reject_review" ? "判定不通过"
      : "取消";
    try {
      if (kind === "approve") await api.approve(taskId);
      else if (kind === "accept_review") await api.reviewVerdict(taskId, "accept");
      else if (kind === "reject_review") await api.reviewVerdict(taskId, "reject");
      else await api.cancel(taskId);
      setError(null);
    } catch (e) {
      setError(`${label}失败：${e}`);
    }
    const cid = currentRef.current;
    if (cid) refreshConversation(cid).catch(() => {});
  }, [refreshConversation]);

  // 开发者模式关闭时，若停留在诊断 / 路由日志页签，显示任务页签（不改变已保存的选择）
  const activeTab: SideTab = devMode || (sideTab !== "diag" && sideTab !== "logs") ? sideTab : "tasks";
  const toggleDevMode = () => {
    const next = !devMode;
    setDevMode(next);
    writeDevMode(next);
    if (!next) setSideTab("tasks");
  };
  const currentTitle = conversations.find((c) => c.id === current)?.title ?? "新会话";

  return (
    <div className={`layout ${navOpen ? "nav-open" : ""}`}>
      {navOpen && <div className="nav-backdrop" onClick={() => setNavOpen(false)} aria-hidden="true" />}

      <aside className="sidebar" aria-label="会话列表">
        <button type="button" className="brand brand-btn" onClick={() => setMainView("home")}>
          AgentNexus <span>Desktop</span>
        </button>
        <button
          type="button"
          className="btn assistant-chip"
          data-testid="assistant-chip"
          onClick={() => setAssistantPanelOpen(true)}
        >
          {assistantProfile.avatar} {assistantProfile.name}
        </button>
        <button className="btn primary" onClick={newConversation}>＋ 新会话</button>
        <button
          type="button"
          className={`btn ${mainView === "home" ? "active" : ""}`}
          onClick={() => setMainView("home")}
        >
          我的桌面
        </button>
        <button
          type="button"
          className={`btn ${desktopSide?.kind === "tasks" ? "active" : ""}`}
          data-testid="open-tasks"
          onClick={() => setDesktopSide({ kind: "tasks" })}
        >
          任务
        </button>
        <button
          type="button"
          className={`btn ${desktopSide?.kind === "history" ? "active" : ""}`}
          data-testid="open-history"
          onClick={() => setDesktopSide({ kind: "history" })}
        >
          历史
        </button>
        <p className="sidebar-label">运营</p>
        <button
          type="button"
          className={`btn ${mainView === "mom-workbench" ? "active" : ""}`}
          data-testid="ops-entry-workbench"
          disabled={opsUnread === null}
          title={opsUnread === null ? "运营模块未启用" : undefined}
          onClick={() => setMainView("mom-workbench")}
        >
          运营工作台
        </button>
        {opsUnread !== null && (
          <button
            type="button"
            className={`btn ${mainView === "desk" ? "active" : ""}`}
            data-testid="ops-entry-desk"
            onClick={() => setMainView("desk")}
          >
            运营桌面
            {opsUnread > 0 && <span className="badge unread" data-testid="ops-unread">{opsUnread}</span>}
          </button>
        )}
        <button
          type="button"
          className={`btn ${mainView === "des-experiment" ? "active" : ""}`}
          data-testid="ops-entry-des"
          onClick={() => setMainView("des-experiment")}
        >
          仿真实验
        </button>
        <button
          type="button"
          className={`btn ${desktopSide?.kind === "settings" ? "active" : ""}`}
          data-testid="open-settings"
          onClick={() => openSettings()}
        >
          设置与管理
        </button>
        <p className="sidebar-label">最近</p>
        <nav className="conv-list" aria-label="最近工作">
          <button
            type="button"
            className={`conv-item ${mainView === "workspace" && workspaceTaskId === SMT03_TASK_ID ? "active" : ""}`}
            onClick={() => openWorkspace(SMT03_TASK_ID)}
          >
            SMT-03 · {SMT03_TASK_ID}
          </button>
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conv-item ${mainView === "chat" && c.id === current ? "active" : ""}`}
              onClick={() => selectConversation(c.id)}
              title={c.title}
            >
              {c.title}
              {c.id === OPS_CHANNEL_ID && opsUnread !== null && opsUnread > 0 && (
                <span className="badge unread" data-testid="ops-unread">{opsUnread}</span>
              )}
              {introMainId && c.id === introMainId && introUnread !== null && introUnread > 0 && (
                <span className="badge unread" data-testid="intro-unread">{introUnread}</span>
              )}
            </button>
          ))}
        </nav>
        <HealthBox health={health} />
      </aside>

      <main className="main">
        {/* 窄屏下左栏变成抽屉，这里提供入口；宽屏下隐藏 */}
        <header className="mobile-bar">
          <button type="button" className="btn small" aria-expanded={navOpen} onClick={() => setNavOpen((v) => !v)}>
            ☰ 会话
          </button>
          <span className="mobile-title">{currentTitle}</span>
        </header>
        {error && (
          <div className="error-bar" role="alert" onClick={() => setError(null)}>
            {error}（点击关闭）
          </div>
        )}
        {mainView === "home" && (
          <DesktopHome
            onOpenTask={(id) => openWorkspace(id)}
            onOpenEvidence={(id) => openEvidence(id)}
            assistantName={assistantProfile.name}
            attentionRevision={attnRevision}
            s1BgJob={s1BgJob}
            s1Reply={s1Reply}
            onS1Ask={(text) => {
              void (async () => {
                setS1Reply("正在思考…");
                try {
                  let cid = s1AskConvRef.current;
                  if (!cid) {
                    const c = await api.createConversation();
                    cid = c.id;
                    s1AskConvRef.current = cid;
                  }
                  const outcome = await api.send(cid, text);
                  const reply = outcome.reply?.content?.trim();
                  setS1Reply(reply || "（模型没有返回内容）");
                  // 问答会话进「最近」，浮层收起后可从历史恢复（DESIGN §2）
                  loadConversations().catch(() => {});
                } catch {
                  // CH-P3：出错就报错，不静默回退到模拟文本
                  setS1Reply("问答暂不可用：无法连接会话服务，请稍后重试。");
                }
              })();
            }}
            onS1Delegate={(text) => {
              setS1Reply(null);
              setS1BgJob({
                taskId: "pending",
                conversationId: "",
                requestId: null,
                label: text,
                status: "submitting",
                error: null,
              });
              void (async () => {
                try {
                  const acc = await durableHomeAccept({ text });
                  writeCurrentConv(acc.conversationId);
                  setS1BgJob({
                    taskId: "pending",
                    conversationId: acc.conversationId,
                    requestId: acc.requestId,
                    label: text,
                    status: "submitting",
                    error: null,
                  });
                  const taskId = await waitForConversationTask(acc.conversationId, text);
                  setS1BgJob({
                    taskId,
                    conversationId: acc.conversationId,
                    requestId: acc.requestId,
                    label: text,
                    status: "accepted",
                    error: null,
                  });
                  await loadConversations().catch(() => {});
                } catch (e) {
                  setS1BgJob((prev) =>
                    prev
                      ? {
                          ...prev,
                          status: "failed",
                          error: e instanceof Error ? e.message : String(e),
                        }
                      : prev,
                  );
                }
              })();
            }}
            onOpenBgTask={(job) => {
              if (!job.conversationId) return;
              selectConversation(job.conversationId);
            }}
          />
        )}
        {(mainView === "home" || mainView === "chat") && (
          <AttentionToast
            items={attnItems}
            muted={attnMuted}
            composing={composingInput}
            onOpenTask={(id) => openWorkspace(id)}
            onChange={({ items, muted }) => {
              setAttnItems(items);
              setAttnMuted(muted);
              setAttnRevision((v) => v + 1);
            }}
          />
        )}
        {assistantPanelOpen && (
          <AssistantChatPanel
            name={assistantProfile.name}
            avatar={assistantProfile.avatar}
            reply={s1Reply}
            onAsk={(text) => {
              // 与首页 S1 问答同一条链路（#304 接真后此处同步为真实回答）
              setS1Reply(`问答（模拟，留在桌面）：${text.slice(0, 80)}`);
            }}
            onOpenSettings={() => {
              setAssistantPanelOpen(false);
              openSettings("assistant", "assistant");
            }}
            onClose={() => setAssistantPanelOpen(false)}
          />
        )}
        {mainView === "mom-workbench" && (
          <MomWorkbench
            onBack={() => setMainView("home")}
            onOpenOpsChannel={() => openOpsChannel(OPS_CHANNEL_ID, false)}
            onOpenDesExperiment={() => setMainView("des-experiment")}
          />
        )}
        {mainView === "workspace" && workspaceTaskId && (
          <WorkspaceView
            taskId={workspaceTaskId}
            onBack={() => {
              setDesktopSide(null);
              setMainView("home");
            }}
            onOpenEvidence={() => openEvidence(workspaceTaskId)}
            onOpenSettings={() => openSettings("assistant", "assistant", true)}
            onOpenTask={(id) => openWorkspace(id)}
            testFixtures={testFixtures}
          />
        )}
        {mainView === "des-experiment" && (
          <DesExperiment
            onBack={() => setMainView("home")}
            onOpenOpsChannel={() => openOpsChannel(OPS_CHANNEL_ID, true)}
          />
        )}
        {mainView === "desk" && opsUnread !== null && (
          <OpsDesk onOpenChannel={(id) => openOpsChannel(id, false)} />
        )}
        {mainView === "chat" && opsFromExperiment && current === OPS_CHANNEL_ID && (
          <div className="des-ops-return">
            <a
              href="#des-experiment"
              onClick={(e) => {
                e.preventDefault();
                setMainView("des-experiment");
              }}
            >
              DES 仿真实验
            </a>
          </div>
        )}
        {mainView === "chat" && (
          <ChatView
            conversationId={current}
            messages={messages}
            onRefresh={() => current && refreshConversation(current).catch(() => {})}
            tasks={tasks}
            notes={notes}
            onSend={send}
            onTaskAction={taskAction}
            streaming={streaming}
            onComposerFocusChange={setComposingInput}
          />
        )}
      </main>

      <aside className="side" aria-label="任务与状态">
        {desktopSide?.kind === "evidence" ? (
          <EvidencePanel
            taskId={desktopSide.taskId}
            layer={desktopSide.layer}
            evidenceId={desktopSide.evidenceId}
            mode={desktopSide.mode}
            onLayer={(layer, evidenceId) =>
              setDesktopSide({ ...desktopSide, layer, evidenceId })
            }
            onClose={() => setDesktopSide(null)}
            testFixtures={testFixtures}
            onDemoEmpty={() =>
              setDesktopSide({
                kind: "evidence",
                taskId: desktopSide.taskId,
                layer: "list",
                evidenceId: null,
                mode: "empty",
              })
            }
          />
        ) : desktopSide?.kind === "history" ? (
          <HistoryPanel
            onOpenTask={(id) => {
              openWorkspace(id);
              setDesktopSide(null);
            }}
            onClose={() => setDesktopSide(null)}
          />
        ) : desktopSide?.kind === "settings" ? (
          <SettingsPanel
            layer={desktopSide.layer}
            categoryId={desktopSide.categoryId}
            memoryId={desktopSide.memoryId}
            search={desktopSide.search}
            closeToSource={settingsViaShortcutRef.current}
            testFixtures={testFixtures}
            onNavigate={(next) =>
              setDesktopSide({
                kind: "settings",
                layer: next.layer,
                categoryId: next.categoryId,
                memoryId: next.memoryId,
                search: next.search ?? desktopSide.search,
              })
            }
            onClose={() => setDesktopSide(null)}
            onProfileSaved={setAssistantProfile}
          />
        ) : desktopSide?.kind === "tasks" ? (
          <TasksSidePanel
            onOpenTask={(id) => {
              openWorkspace(id);
              setDesktopSide(null);
            }}
            onClose={() => setDesktopSide(null)}
          />
        ) : mainView !== "chat" ? (
          <AssistantReadPanel
            taskNotices={taskNotices}
            onOpenNotice={(taskId) => {
              openWorkspace(taskId);
              setDesktopSide(null);
            }}
          />
        ) : (
          <>
            <div className="tabs" role="tablist">
              <TabButton id="tasks" label="任务" active={activeTab} onSelect={setSideTab}>
                {tasks.length > 0 && <span className="count">{tasks.length}</span>}
              </TabButton>
              <TabButton id="intro" label="内省" active={activeTab} onSelect={setSideTab}>
                {introUnread !== null && introUnread > 0 && <span className="count" data-testid="intro-tab-unread">{introUnread}</span>}
              </TabButton>
              <TabButton id="agents" label="助手" active={activeTab} onSelect={setSideTab} />
              <TabButton id="nodes" label="节点" active={activeTab} onSelect={setSideTab} />
              {devMode && <TabButton id="diag" label="诊断" active={activeTab} onSelect={setSideTab} />}
              {devMode && <TabButton id="logs" label="路由日志" active={activeTab} onSelect={setSideTab} />}
            </div>
            {activeTab === "tasks" && (
              <TaskPanel
                tasks={tasks}
                incoming={incoming}
                onChanged={() => current && refreshConversation(current)}
                onTaskAction={taskAction}
                messageIds={taskMessageIds(messages, tasks)}
              />
            )}
            {activeTab === "intro" && <IntrospectionPanel />}
            {activeTab === "agents" && <AgentsPanel />}
            {activeTab === "nodes" && <NodesPanel />}
            {activeTab === "diag" && <DiagnosticsPanel refreshTick={panelTick} />}
            {activeTab === "logs" && <RouteLogPanel refreshTick={panelTick} />}
            <div className="side-footer">
              <button type="button" className="dev-toggle" aria-pressed={devMode} onClick={toggleDevMode}>
                开发者模式：{devMode ? "开" : "关"}
              </button>
            </div>
          </>
        )}
      </aside>
    </div>
  );
}

/** 接入方式的显示名 */
const KIND_LABEL: Record<string, string> = { builtin: "内置", cli: "CLI", acp: "ACP", node: "节点" };

/** 侧栏「任务」：全部活跃任务的轻量列表（点击进工作区）。T-88 首页整治。 */
function TasksSidePanel(props: { onOpenTask: (id: string) => void; onClose: () => void }) {
  const [tasks, setTasks] = useState<TaskRecord[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    api
      .tasksAll()
      .then(setTasks)
      .catch((e) => setErr(String(e)));
  }, []);

  const active = (tasks ?? []).filter((t) => t.status !== "COMPLETED" && t.status !== "CANCELLED");
  const done = (tasks ?? []).length - active.length;

  return (
    <div data-testid="tasks-side">
      <p className="settings-crumb">
        任务
        <button type="button" className="btn small" aria-label="关闭任务面板" onClick={props.onClose}>
          关闭
        </button>
      </p>
      {err && <p className="bad-text" role="alert">{err}</p>}
      {tasks === null && <p className="muted">加载中…</p>}
      {tasks !== null && (
        <>
          <p className="muted">进行中 {active.length} · 已完成折叠 {done}</p>
          <ul className="conv-list" aria-label="活跃任务">
            {active.map((t) => (
              <li key={t.id}>
                <button
                  type="button"
                  className="conv-item"
                  onClick={() => props.onOpenTask(t.id)}
                  title={t.id}
                >
                  {t.goal}
                  <span className="agent-meta">{t.status}</span>
                </button>
              </li>
            ))}
          </ul>
          {active.length === 0 && <p className="muted">暂无进行中的任务</p>}
        </>
      )}
    </div>
  );
}

/** 左下角：连接状态摘要（一行）；助手与后端详情默认折叠到「连接详情」（T-88，设计原则 §13 诊断层折叠） */
export function HealthBox({ health }: { health: Health | null }) {
  const [open, setOpen] = useState(false);
  if (!health) return <div className="health"><span className="dot bad" /> 后端未连接</div>;
  const availableAgents = health.agents.filter((a) => (a.link?.status ?? (a.enabled ? "connected" : "unavailable")) === "connected").length;
  return (
    <div className="health">
      <div>
        <span className={`dot ${health.llm.ok ? "ok" : "bad"}`} />
        连接正常 · {availableAgents}/{health.agents.length} 个助手可用
      </div>
      {open && (
        <>
          {(health.backends ?? []).map((b) => (
            <div key={b.name}>
              <span className={`dot ${b.ok ? "ok" : "bad"}`} />
              {b.name} {b.ok ? "已连接" : "未连接"}
              {b.slots.some((s) => s.server_busy) ? ` · ${b.slots.filter((s) => s.server_busy).length} 个槽位生成中` : ""}
            </div>
          ))}
          {/* 每个 agent 一行：名称 + 角色 + 接入方式。绿=已连通可调用，灰=已配置未连通，红=不可用；灰红的原因在悬停提示里 */}
          {health.agents.map((a) => {
            const status = a.link?.status ?? (a.enabled ? "connected" : "unavailable");
            const dot = status === "connected" ? "ok" : status === "configured" ? "idle" : "bad";
            return (
              <div key={a.id} className="agent-row" title={a.link?.reason}>
                <span className={`dot ${dot}`} />
                {a.name}
                <span className="agent-meta">{roleLabel(a.role)} · {KIND_LABEL[a.kind] ?? a.kind}</span>
                {!a.enabled && a.disabled_reason && <div className="bad-text agent-reason">{a.disabled_reason}</div>}
              </div>
            );
          })}
          <div className="health-detail">
            {(health.backends ?? []).map((b) => (
              <div key={b.name}>
                {b.base_url} · {b.server_models.join(", ") || b.model}
                {b.error && <div className="bad-text">{b.error}</div>}
              </div>
            ))}
            <div>结构化：{health.llm.structured_mode}</div>
          </div>
        </>
      )}
      <button type="button" className="health-toggle" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        {open ? "收起详情 ▴" : "连接详情 ▾"}
      </button>
    </div>
  );
}

/** 右侧页签按钮：统一的 tab 语义，供屏幕阅读器和键盘使用 */
function TabButton(props: {
  id: SideTab;
  label: string;
  active: SideTab;
  onSelect: (id: SideTab) => void;
  children?: React.ReactNode;
}) {
  const selected = props.active === props.id;
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      className={selected ? "active" : ""}
      onClick={() => props.onSelect(props.id)}
    >
      {props.label} {props.children}
    </button>
  );
}
