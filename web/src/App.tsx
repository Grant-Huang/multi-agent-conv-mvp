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
import { TaskPanel } from "./components/TaskPanel";
import { NodesPanel } from "./components/NodesPanel";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { AgentsPanel } from "./components/AgentsPanel";
import { DiscoveryCard } from "./components/AgentCards";
import { OpsDesk } from "./components/OpsDesk";
import { IntrospectionPanel } from "./components/IntrospectionPanel";
import { DesktopHome } from "./desktop/DesktopHome";
import { DesExperiment } from "./desktop/DesExperiment";
import { MomWorkbench } from "./desktop/MomWorkbench";
import { WorkspaceView } from "./desktop/WorkspaceView";
import { EvidencePanel, type EvidenceLayer } from "./desktop/EvidencePanel";
import { HistoryPanel } from "./desktop/HistoryPanel";
import { SettingsPanel, type SettingsLayer } from "./desktop/SettingsPanel";
import { loadProfile, type AssistantProfile, type SettingsCategoryId } from "./desktop/profileStore";
import { AttentionToast, type AttentionItem } from "./desktop/AttentionToast";
import type { S1BgJob } from "./desktop/S1AskBar";
import {
  SMT03_TASK_ID,
  readDesktopView,
  writeDesktopView,
  type DesktopMainView,
} from "./desktop/mockData";
import type { CatalogItem } from "./bindings/CatalogItem";
import { roleLabel } from "./labels";

type MainView = DesktopMainView;

type DesktopSide =
  | null
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
/** 首次打开时的"发现卡"是否已经看过（按浏览器记录，AG.3 用户确认） */
const AGENTS_INTRO_KEY = "an.agentsIntroSeen";

function readIntroSeen(): boolean {
  try {
    return localStorage.getItem(AGENTS_INTRO_KEY) === "1";
  } catch {
    // 读不到本地存储（隐私窗口等）时不打扰用户
    return true;
  }
}

function writeIntroSeen() {
  try {
    localStorage.setItem(AGENTS_INTRO_KEY, "1");
  } catch {
    /* 忽略：只是界面偏好 */
  }
}

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
  // 首次打开时的发现卡：没看过才去读目录
  const [introCatalog, setIntroCatalog] = useState<CatalogItem[] | null>(null);
  useEffect(() => {
    if (readIntroSeen()) return;
    api
      .agentCatalog()
      .then(setIntroCatalog)
      .catch(() => {});
  }, []);
  const dismissIntro = () => {
    writeIntroSeen();
    setIntroCatalog(null);
  };
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
    setNavOpen(false);
  };

  const [desktopSide, setDesktopSide] = useState<DesktopSide>(null);
  const [assistantProfile, setAssistantProfile] = useState<AssistantProfile>(() => loadProfile());
  const [s1BgJob, setS1BgJob] = useState<S1BgJob | null>(null);
  const [s1Reply, setS1Reply] = useState<string | null>(null);
  const [composingInput, setComposingInput] = useState(false);
  const [attnItems, setAttnItems] = useState<AttentionItem[]>([]);
  const [attnMuted, setAttnMuted] = useState(false);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const apply = () => document.documentElement.dataset.reducedMotion = mq.matches ? "1" : "0";
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, []);

  useEffect(() => {
    let cancelled = false;
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
      })
      .catch(() => {
        /* harness 未就绪时忽略 */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const openWorkspace = (taskId: string) => {
    setMainView("workspace", taskId);
    setNavOpen(false);
  };

  const openSettings = (layer: SettingsLayer = "categories", categoryId: SettingsCategoryId | null = null) => {
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
   * 批准 / 取消任务：对话区和任务面板共用这一个入口。
   * 失败时显示在顶部错误栏（行内），不使用 alert；成功后刷新会话，让两处同时更新。
   */
  const taskAction = useCallback(async (kind: TaskActionKind, taskId: string) => {
    try {
      if (kind === "approve") await api.approve(taskId);
      else await api.cancel(taskId);
      setError(null);
    } catch (e) {
      setError(`${kind === "approve" ? "批准" : "取消"}失败：${e}`);
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
          onClick={() => openSettings("assistant", "assistant")}
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
          className={`btn ${desktopSide?.kind === "history" ? "active" : ""}`}
          data-testid="open-history"
          onClick={() => setDesktopSide({ kind: "history" })}
        >
          历史
        </button>
        <button
          type="button"
          className={`btn ${desktopSide?.kind === "settings" ? "active" : ""}`}
          data-testid="open-settings"
          onClick={() => openSettings()}
        >
          设置与管理
        </button>
        <a className="btn" href="/mom/">MOM 运营沙盘</a>
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
        <div className="main-tabs" role="tablist" aria-label="主视图">
          <button
            type="button"
            role="tab"
            aria-selected={mainView === "home" || mainView === "workspace"}
            className={mainView === "home" || mainView === "workspace" ? "active" : ""}
            onClick={() => setMainView("home")}
          >
            我的桌面
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mainView === "chat"}
            className={mainView === "chat" ? "active" : ""}
            onClick={() => setMainView("chat")}
          >
            会话
          </button>
          {opsUnread !== null && (
            <button
              type="button"
              role="tab"
              aria-selected={mainView === "desk"}
              className={mainView === "desk" ? "active" : ""}
              onClick={() => setMainView("desk")}
            >
              运营桌面
            </button>
          )}
        </div>
        {introCatalog && (mainView === "home" || mainView === "chat") && (
          <DiscoveryCard
            catalog={introCatalog}
            onDismiss={dismissIntro}
            onManage={() => {
              dismissIntro();
              setSideTab("agents");
            }}
          />
        )}
        {mainView === "home" && (
          <DesktopHome
            onOpenTask={(id) => openWorkspace(id)}
            onOpenEvidence={(id) => openEvidence(id)}
            onOpenDesExperiment={() => setMainView("des-experiment")}
            onOpenOpsChannel={() => openOpsChannel(OPS_CHANNEL_ID, false)}
            onOpenMomWorkbench={() => setMainView("mom-workbench")}
            opsEnabled={opsUnread !== null}
            assistantName={assistantProfile.name}
            s1BgJob={s1BgJob}
            s1Reply={s1Reply}
            onS1Ask={(text) => {
              setS1Reply(`问答（模拟，留在桌面）：${text.slice(0, 80)}`);
            }}
            onS1Delegate={(text) => {
              setS1Reply(null);
              setS1BgJob({ taskId: SMT03_TASK_ID, label: text, status: "submitting" });
              window.setTimeout(() => {
                setS1BgJob({ taskId: SMT03_TASK_ID, label: text, status: "accepted" });
              }, 400);
            }}
            onOpenBgTask={(id) => openWorkspace(id)}
          />
        )}
        {(mainView === "home" || mainView === "chat") && (
          <AttentionToast
            items={attnItems}
            muted={attnMuted}
            composing={composingInput}
            onChange={({ items, muted }) => {
              setAttnItems(items);
              setAttnMuted(muted);
            }}
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
            onOpenSettings={() => openSettings("assistant", "assistant")}
            onOpenTask={(id) => openWorkspace(id)}
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
                messageIds={Object.fromEntries(messages.filter((m) => m.role === "user" && m.task_id).map((m) => [m.task_id as string, m.id]))}
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

/** 左下角：LLM 连接状态和 Agent 可用状态。不可用的原因直接显示，不依赖悬停提示 */
export function HealthBox({ health }: { health: Health | null }) {
  const [open, setOpen] = useState(false);
  if (!health) return <div className="health"><span className="dot bad" /> 后端未连接</div>;
  return (
    <div className="health">
      {(health.backends ?? []).length === 0 ? (
        <div>
          <span className={`dot ${health.llm.ok ? "ok" : "bad"}`} />
          LLM {health.llm.ok ? "已连接" : "未连接"}
        </div>
      ) : (
        health.backends.map((b) => (
          <div key={b.name}>
            <span className={`dot ${b.ok ? "ok" : "bad"}`} />
            {b.name} {b.ok ? "已连接" : "未连接"}
            {b.slots.some((s) => s.server_busy) ? ` · ${b.slots.filter((s) => s.server_busy).length} 个槽位生成中` : ""}
          </div>
        ))
      )}
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
      {open && (
        <div className="health-detail">
          {(health.backends ?? []).map((b) => (
            <div key={b.name}>
              {b.base_url} · {b.server_models.join(", ") || b.model}
              {b.error && <div className="bad-text">{b.error}</div>}
            </div>
          ))}
          <div>结构化：{health.llm.structured_mode}</div>
        </div>
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
