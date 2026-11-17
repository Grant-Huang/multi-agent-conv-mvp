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
import type { CatalogItem } from "./bindings/CatalogItem";
import type { ProbeOutcome } from "./bindings/ProbeOutcome";
import type { MessageView } from "./bindings/MessageView";

// ───── Agent 配置（AG.2 / AG.3）。这些结构来自 an-runtime（不在 an-core 的 bindings 里），在这里手写 ─────

/** 托管 / config.toml 里的一个 ACP agent（对应 Rust 的 AcpConfig，只列界面用到的字段） */
export interface AcpAgentConfig {
  id: string;
  name?: string | null;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  role: "general" | "coding" | "reviewer";
  agent_type?: string | null;
}

/** GET /api/agents/config：托管配置概况 */
export interface AgentConfigView {
  load_error: string | null;
  can_undo: boolean;
  config_toml_ids: string[];
  config_toml_roles: string[];
  assign: Record<string, string[]>;
  managed: {
    version: number;
    acp: { disabled?: string[]; agents?: AcpAgentConfig[] };
    agents: { assign?: Record<string, string[]> };
  };
}

/** 修改请求（POST /api/agents/proposals 的请求体） */
export type ChangeRequest =
  | { kind: "add_preset"; id: string }
  | { kind: "add_custom"; agent: AcpAgentConfig }
  | { kind: "set_default"; role: string; ids: string[] }
  | { kind: "disable"; id: string }
  | { kind: "enable"; id: string }
  | { kind: "remove"; id: string }
  | { kind: "revert" };

/** 提议：只校验、不写入；token 只给界面，用来确认 */
export interface Proposal {
  id: string;
  kind: string;
  summary: string;
  warnings: string[];
  before: string;
  after: string;
  token: string;
  expires_in_secs: number;
}

/** 确认后的结果 */
export interface Applied {
  kind: string;
  summary: string;
  probes: ProbeOutcome[];
}

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
  /** 消息列表：每条消息多一个 card（AG.4 对话里的卡片） */
  messages: (cid: string) => request<MessageView[]>(`/api/conversations/${cid}/messages`),
  /** 在会话里生成确认卡（选择卡点选、结果卡撤销时用） */
  proposeInConversation: (cid: string, change: ChangeRequest) =>
    request<MessageView>(`/api/conversations/${cid}/agent-proposals`, { method: "POST", body: JSON.stringify(change) }),
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
  agents: () => request<AgentCard[]>("/api/agents"),
  agentCatalog: () => request<CatalogItem[]>("/api/agents/catalog"),
  agentConfig: () => request<AgentConfigView>("/api/agents/config"),
  probeAgent: (id: string) =>
    request<ProbeOutcome>("/api/agents/probe", { method: "POST", body: JSON.stringify({ id }) }),
  proposeAgentChange: (change: ChangeRequest) =>
    request<Proposal>("/api/agents/proposals", { method: "POST", body: JSON.stringify(change) }),
  applyAgentProposal: (id: string, token: string) =>
    request<Applied>(`/api/agents/proposals/${encodeURIComponent(id)}/apply`, {
      method: "POST",
      body: JSON.stringify({ token }),
    }),
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
