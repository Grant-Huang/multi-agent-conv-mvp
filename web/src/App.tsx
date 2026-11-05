// AgentNexus MVP 前端：左侧会话列表，中间对话，右侧任务面板 / 路由日志。
import { useCallback, useEffect, useRef, useState } from "react";
import { api, subscribeEvents, type Health } from "./api";
import type { TaskEvent } from "./bindings/TaskEvent";
import { createDeltaBuffer, createEventPump, REFRESH_INTERVAL_MS } from "./refresh";
import type { Conversation } from "./bindings/Conversation";
import type { Message } from "./bindings/Message";
import type { TaskRecord } from "./bindings/TaskRecord";
import { ChatView } from "./components/ChatView";
import { RouteLogPanel } from "./components/RouteLogPanel";
import { TaskPanel } from "./components/TaskPanel";
import { NodesPanel } from "./components/NodesPanel";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";

export default function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [sideTab, setSideTab] = useState<"tasks" | "nodes" | "diag" | "logs">("tasks");
  // 只在状态变化 / 助手消息时递增，而且做了节流；诊断页和路由日志用它刷新
  const [panelTick, setPanelTick] = useState(0);
  // 最新一条任务事件，展开的任务卡片会把它追加到时间线上
  const [incoming, setIncoming] = useState<TaskEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 正在流式生成的回答：id 等于最终消息的 id，最终消息到达后自动隐藏
  const [streaming, setStreaming] = useState<{ id: string; text: string } | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>({});
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

  // 首次加载：选中最近的会话
  useEffect(() => {
    loadConversations()
      .then((list) => setCurrent((c) => c ?? list[0]?.id ?? null))
      .catch((e) => setError(String(e)));
  }, [loadConversations]);

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
          setNotes((n) => ({ ...n, [id]: ev.message }));
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

  // 已经结束的任务不会再推事件：打开会话时用最后一条事件当作走马灯
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
        const last = [...d.events].reverse().find((e) => e.kind !== "assistant_message" && e.message.trim());
        if (!last) return;
        setNotes((n) => (n[id] ? n : { ...n, [id]: last.message }));
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

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">AgentNexus <span>MVP</span></div>
        <button className="btn primary" onClick={newConversation}>＋ 新会话</button>
        <nav className="conv-list">
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conv-item ${c.id === current ? "active" : ""}`}
              onClick={() => setCurrent(c.id)}
              title={c.title}
            >
              {c.title}
            </button>
          ))}
        </nav>
        <HealthBox health={health} />
      </aside>

      <main className="main">
        {error && <div className="error-bar" onClick={() => setError(null)}>{error}（点击关闭）</div>}
        <ChatView messages={messages} tasks={tasks} notes={notes} onSend={send} streaming={streaming} />
      </main>

      <aside className="side">
        <div className="tabs">
          <button className={sideTab === "tasks" ? "active" : ""} onClick={() => setSideTab("tasks")}>
            任务 {tasks.length > 0 && <span className="count">{tasks.length}</span>}
          </button>
          <button className={sideTab === "nodes" ? "active" : ""} onClick={() => setSideTab("nodes")}>节点</button>
          <button className={sideTab === "diag" ? "active" : ""} onClick={() => setSideTab("diag")}>诊断</button>
          <button className={sideTab === "logs" ? "active" : ""} onClick={() => setSideTab("logs")}>路由日志</button>
        </div>
        {sideTab === "tasks" && (
          <TaskPanel tasks={tasks} incoming={incoming} onChanged={() => current && refreshConversation(current)} />
        )}
        {sideTab === "nodes" && <NodesPanel />}
        {sideTab === "diag" && <DiagnosticsPanel refreshTick={panelTick} />}
        {sideTab === "logs" && <RouteLogPanel refreshTick={panelTick} />}
      </aside>
    </div>
  );
}

/** 接入方式的显示名 */
const KIND_LABEL: Record<string, string> = { builtin: "内置", cli: "CLI", acp: "ACP", node: "节点" };

/** 左下角：LLM 连接状态和 Agent 可用状态 */
function HealthBox({ health }: { health: Health | null }) {
  const [open, setOpen] = useState(false);
  if (!health) return <div className="health"><span className="dot bad" /> 后端未连接</div>;
  return (
    <div className="health" onClick={() => setOpen(!open)} title="点击展开">
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
      {/* 每个 agent 一行：名称 + 角色 + 接入方式；不可用时悬停可看原因 */}
      {health.agents.map((a) => (
        <div key={a.id} title={a.disabled_reason ?? a.description}>
          <span className={`dot ${a.enabled ? "ok" : "bad"}`} />
          {a.name}
          <span className="agent-meta">{a.role} · {KIND_LABEL[a.kind] ?? a.kind}</span>
        </div>
      ))}
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
    </div>
  );
}
