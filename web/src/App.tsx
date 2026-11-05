// AgentNexus MVP 前端：左侧会话列表，中间对话，右侧任务面板 / 路由日志。
import { useCallback, useEffect, useState } from "react";
import { api, subscribeEvents, type Health } from "./api";
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
  // 每次收到事件时递增，用来通知子组件刷新（例如展开中的任务详情）
  const [eventTick, setEventTick] = useState(0);
  const [error, setError] = useState<string | null>(null);

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

  // 切换会话：加载数据 + 订阅 SSE 事件
  useEffect(() => {
    if (!current) {
      setMessages([]);
      setTasks([]);
      return;
    }
    refreshConversation(current).catch((e) => setError(String(e)));
    return subscribeEvents(current, (ev) => {
      setEventTick((n) => n + 1);
      if (ev.kind === "assistant_message") {
        // 任务结果交付：刷新消息
        api.messages(current).then(setMessages).catch(() => {});
      }
      if (ev.kind === "status_changed" || ev.kind === "assistant_message") {
        api.tasks(current).then(setTasks).catch(() => {});
      }
    });
  }, [current, refreshConversation]);

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
        <ChatView messages={messages} onSend={send} />
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
          <TaskPanel tasks={tasks} eventTick={eventTick} onChanged={() => current && refreshConversation(current)} />
        )}
        {sideTab === "nodes" && <NodesPanel />}
        {sideTab === "diag" && <DiagnosticsPanel eventTick={eventTick} />}
        {sideTab === "logs" && <RouteLogPanel eventTick={eventTick} />}
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
      <div>
        <span className={`dot ${health.llm.ok ? "ok" : "bad"}`} />
        LLM {health.llm.ok ? "已连接" : "未连接"}
      </div>
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
          <div>{health.llm.base_url}</div>
          <div>模型：{health.llm.server_models.join(", ") || health.llm.model}</div>
          <div>结构化：{health.llm.structured_mode}</div>
          {health.llm.error && <div className="bad-text">{health.llm.error}</div>}
        </div>
      )}
    </div>
  );
}
