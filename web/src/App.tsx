// AgentNexus MVP 前端：左侧会话列表，中间对话，右侧任务面板 / 路由日志。
import { useCallback, useEffect, useRef, useState } from "react";
import { api, subscribeEvents, type Health } from "./api";
import type { TaskEvent } from "./bindings/TaskEvent";
import { createDeltaBuffer, createEventPump, REFRESH_INTERVAL_MS } from "./refresh";
import type { Conversation } from "./bindings/Conversation";
import type { Message } from "./bindings/Message";
import type { TaskRecord } from "./bindings/TaskRecord";
import { ChatView, type TaskActionKind } from "./components/ChatView";
import { RouteLogPanel } from "./components/RouteLogPanel";
import { TaskPanel } from "./components/TaskPanel";
import { NodesPanel } from "./components/NodesPanel";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { roleLabel } from "./labels";

/** 每个任务保留的进度条目数，超过后丢弃最早的 */
const MAX_PROGRESS_LINES = 30;
/** 开发者模式的本地开关键（只是界面偏好，不影响数据） */
const DEV_MODE_KEY = "agentnexus.devMode";

type SideTab = "tasks" | "nodes" | "diag" | "logs";

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

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
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

  const refreshConversation = useCallback(async (cid: string) => {
    const [m, t] = await Promise.all([api.messages(cid), api.tasks(cid)]);
    setMessages(m);
    setTasks(t);
  }, []);

  const sideRef = useRef(sideTab);
  sideRef.current = sideTab;
  const currentRef = useRef(current);
  currentRef.current = current;
  const streamRef = useRef("");

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

  const newConversation = async () => {
    const c = await api.createConversation();
    await loadConversations();
    setCurrent(c.id);
    setNavOpen(false);
  };

  const selectConversation = (id: string) => {
    setCurrent(id);
    setNavOpen(false);
  };

  /** 发送消息：先乐观地显示用户消息，等后端返回后再整体刷新 */
  const send = async (text: string) => {
    let cid = current;
    if (!cid) {
      const c = await api.createConversation();
      cid = c.id;
      setCurrent(cid);
    }
    const optimistic: Message = {
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
        <div className="brand">AgentNexus <span>MVP</span></div>
        <button className="btn primary" onClick={newConversation}>＋ 新会话</button>
        <a className="btn" href="/mom/">MOM 运营沙盘</a>
        <nav className="conv-list">
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conv-item ${c.id === current ? "active" : ""}`}
              onClick={() => selectConversation(c.id)}
              title={c.title}
            >
              {c.title}
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
        <ChatView
          messages={messages}
          tasks={tasks}
          notes={notes}
          onSend={send}
          onTaskAction={taskAction}
          streaming={streaming}
        />
      </main>

      <aside className="side" aria-label="任务与状态">
        <div className="tabs" role="tablist">
          <TabButton id="tasks" label="任务" active={activeTab} onSelect={setSideTab}>
            {tasks.length > 0 && <span className="count">{tasks.length}</span>}
          </TabButton>
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
        {activeTab === "nodes" && <NodesPanel />}
        {activeTab === "diag" && <DiagnosticsPanel refreshTick={panelTick} />}
        {activeTab === "logs" && <RouteLogPanel refreshTick={panelTick} />}
        <div className="side-footer">
          <button type="button" className="dev-toggle" aria-pressed={devMode} onClick={toggleDevMode}>
            开发者模式：{devMode ? "开" : "关"}
          </button>
        </div>
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
