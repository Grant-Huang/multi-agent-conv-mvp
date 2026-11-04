// 后端 API 封装。类型来自 Rust 自动生成的 bindings（cargo test -p an-core 生成）。
import type { AgentCard } from "./bindings/AgentCard";
import type { Conversation } from "./bindings/Conversation";
import type { Message } from "./bindings/Message";
import type { RouteLog } from "./bindings/RouteLog";
import type { TaskEvent } from "./bindings/TaskEvent";
import type { TaskRecord } from "./bindings/TaskRecord";
import type { TurnDecision } from "./bindings/TurnDecision";

/** 发送消息后的返回（对应 Rust 的 TurnOutcome） */
export interface TurnOutcome {
  reply: Message;
  decision: TurnDecision | null;
  task: TaskRecord | null;
}

export interface Health {
  llm: {
    ok: boolean;
    base_url: string;
    model: string;
    server_models: string[];
    error: string | null;
    structured_mode: string;
  };
  agents: AgentCard[];
}

export interface TaskDetail {
  task: TaskRecord;
  events: TaskEvent[];
}

/** 统一的请求函数：非 2xx 时抛出后端返回的错误信息 */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error ?? `HTTP ${res.status}`);
  return body as T;
}

export const api = {
  health: () => request<Health>("/api/health"),
  conversations: () => request<Conversation[]>("/api/conversations"),
  createConversation: () => request<Conversation>("/api/conversations", { method: "POST", body: "{}" }),
  messages: (cid: string) => request<Message[]>(`/api/conversations/${cid}/messages`),
  send: (cid: string, text: string) =>
    request<TurnOutcome>(`/api/conversations/${cid}/messages`, { method: "POST", body: JSON.stringify({ text }) }),
  tasks: (cid: string) => request<TaskRecord[]>(`/api/tasks?conversation_id=${cid}`),
  task: (id: string) => request<TaskDetail>(`/api/tasks/${id}`),
  cancel: (id: string) => request<{ cancelled: boolean }>(`/api/tasks/${id}/cancel`, { method: "POST" }),
  approve: (id: string) => request<{ approved: boolean }>(`/api/tasks/${id}/approve`, { method: "POST" }),
  routeLogs: () => request<RouteLog[]>("/api/route-logs?limit=50"),
};

/** 订阅某个会话的 SSE 事件流，返回取消订阅函数 */
export function subscribeEvents(cid: string, onEvent: (e: TaskEvent) => void): () => void {
  const es = new EventSource(`/api/events?conversation_id=${cid}`);
  es.addEventListener("task_event", (msg) => {
    try {
      onEvent(JSON.parse((msg as MessageEvent).data));
    } catch {
      /* 忽略无法解析的事件 */
    }
  });
  return () => es.close();
}
