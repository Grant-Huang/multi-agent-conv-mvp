// 后端 API 封装。类型来自 Rust 自动生成的 bindings（cargo test -p an-core 生成）。
import type { AgentCard } from "./bindings/AgentCard";
import type { Conversation } from "./bindings/Conversation";
import type { Message } from "./bindings/Message";
import type { RouteLog } from "./bindings/RouteLog";
import type { TaskEvent } from "./bindings/TaskEvent";
import type { TaskRecord } from "./bindings/TaskRecord";
import type { TurnDecision } from "./bindings/TurnDecision";
import type { DiscoveredNode } from "./bindings/DiscoveredNode";
import type { PairedNode } from "./bindings/PairedNode";
import type { Problem } from "./bindings/Problem";

export interface NodesView {
  discovered: DiscoveredNode[];
  paired: PairedNode[];
}

/** 一个后端的健康状况（/api/health 和 /api/diagnostics 都带） */
export interface BackendStatus {
  name: string;
  kind: string;
  base_url: string;
  model: string;
  roles: string[];
  ok: boolean;
  server_models: string[];
  error: string | null;
  server_error: string | null;
  slots: { id: number; class: string; holder: string | null; held_ms: number; server_busy: boolean | null; tasks: number | null }[];
  metrics: { processing: number; queued: number; gen_per_sec: number } | null;
  scheduler: { inflight: number; max_inflight: number; queue_len: number; paused_ms: number };
  calls: { total: number; outcomes: Record<string, number>; total_ms_p50: number; total_ms_p95: number; ttft_ms_p50: number; ttft_ms_p95: number };
}

/** /api/diagnostics 的返回 */
export interface Diagnostics {
  version: string;
  time: string;
  llm: { ok: boolean; models?: string[]; code?: string; error?: string };
  backends: BackendStatus[];
  /** 角色 → 执行组 */
  groups: Record<string, { members?: string[]; enabled?: boolean; error?: string }>;
  agents: AgentCard[];
  problem_counts: Record<string, number>;
  recent_problems: Problem[];
}

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
  backends: BackendStatus[];
  agents: AgentWithLink[];
}

/** 连接状态：connected 绿（已连通且可调用）、configured 灰（已配置但未连通）、unavailable 红（不可用） */
export interface AgentLink {
  status: "connected" | "configured" | "unavailable";
  reason: string;
}

export type AgentWithLink = AgentCard & { link?: AgentLink };

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
  if (!res.ok) throw new Error(body.message ?? body.error ?? `HTTP ${res.status}`);
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
  task: (id: string, after?: string) => request<TaskDetail>(`/api/tasks/${id}${after ? `?after=${encodeURIComponent(after)}` : ""}`),
  cancel: (id: string) => request<{ cancelled: boolean }>(`/api/tasks/${id}/cancel`, { method: "POST" }),
  approve: (id: string) => request<{ approved: boolean }>(`/api/tasks/${id}/approve`, { method: "POST" }),
  feedback: (body: {
    message_id: string;
    original_action: string;
    original_role?: string;
    correct_action: string;
    correct_role?: string;
    note?: string;
  }) =>
    request<{ status: string; data: { id: string }; message: string }>("/api/feedback", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  routeLogs: () => request<RouteLog[]>("/api/route-logs?limit=50"),
  nodes: () => request<NodesView>("/api/nodes"),
  pairNode: (address: string, code: string) =>
    request<PairedNode>("/api/nodes/pair", { method: "POST", body: JSON.stringify({ address, code }) }),
  unpairNode: (id: string) => request<{ unpaired: boolean }>(`/api/nodes/${id}`, { method: "DELETE" }),
  diagnostics: () => request<Diagnostics>("/api/diagnostics"),
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
