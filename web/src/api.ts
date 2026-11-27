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
import type { DiagnoseReport } from "./bindings/DiagnoseReport";
import type { DiagnosePreview } from "./bindings/DiagnosePreview";
import type { DiagnoseSendResult } from "./bindings/DiagnoseSendResult";

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

/** 业务内省结论（HTTP 列表项） */
export interface IntroFinding {
  id: string;
  job_id: string;
  topic: string;
  json: unknown;
  verification: string;
  gate_score: number | null;
  gate_reason: string;
  pushed: boolean;
  verdict: string | null;
  verdict_note: string | null;
  created_at: string;
  updated_at: string;
}

export interface IntroLabel {
  id: string;
  finding_id: string;
  kind: string;
  note: string;
  created_at: string;
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

/** 运营桌面读模型（与后端 `ops_view::DeskView` 对齐，手写，不进 bindings） */
export interface OpsDeskStrip {
  pending: number;
  blocked: number;
  overdue: number;
  risk: number;
  completed: number;
}
export interface OpsDeskRow {
  case_id: string;
  title: string;
  stage: string;
  stage_label: string;
  group: string;
  owner: string;
  deadline_minutes_left: number | null;
  stale: boolean;
  provenance: string;
}
export interface OpsDeskView {
  mode: string;
  now: number;
  state: string;
  strip: OpsDeskStrip;
  rows: OpsDeskRow[];
}
/** 抽屉详情：复用 human_view 的 JSON，字段按需读取 */
export type OpsDeskCaseDetail = Record<string, unknown>;

/** World Adapter M0 Mock（`/api/world/mock`，[mom] world_mock） */
export interface WorldMockConfigResult {
  protocol_version: number;
  config_version: string;
  request_id?: string | null;
}
export interface WorldMockTrialResult {
  protocol_version: number;
  trial_run_id: string;
  world_id: string;
  mode: string;
  delivery_count: number;
}
export interface WorldMockOfficialResult {
  protocol_version: number;
  official_run_id: string;
  world_id: string;
  mode: string;
  delivery_count: number;
  case_id: string | null;
  already?: boolean;
}
export interface WorldMockRun {
  protocol_version: number;
  run_id: string;
  world_id: string;
  mode: string;
  config_version: string;
  target_channel: string | null;
  simulation_minute: number;
  run_status: string;
  delivery_count: number;
  case_id: string | null;
  action_count: number;
  wait_label: string;
  failed: boolean;
}

/** World Adapter M1 远程 DES 读（`/api/world/des`） */
export interface WorldDesBindResult {
  protocol_version: number;
  world_id: string;
  provenance: string;
}
export interface WorldDesEvent {
  event_id: string;
  kind: string;
  source?: string;
  payload?: unknown;
}
export interface WorldDesObserveResult {
  protocol_version: number;
  world_id: string;
  provenance: string;
  cursor: string;
  publish_seq?: number;
  events: WorldDesEvent[];
}

/** World Adapter M3 生产只读（`/api/world/prod`） */
export interface WorldProdBindResult {
  protocol_version: number;
  world_id: string;
  provenance: string;
  site_id?: string;
  authority?: string;
  capabilities?: { write?: boolean; observations?: boolean; actions?: boolean };
}
export interface WorldProdObject {
  source?: string;
  delayed?: boolean;
  observed_at?: string;
  state?: { delay_ms?: number; status?: string; publish_seq?: number };
}
export interface WorldProdObserveResult {
  protocol_version: number;
  world_id: string;
  provenance: string;
  cursor: string;
  publish_seq?: number;
  as_of?: string;
  source?: string;
  consistency?: string;
  events: WorldDesEvent[];
  objects?: WorldProdObject[];
}

const WA_ROLE = { "X-WA-Role": "experimenter" };
const WA_OPERATOR = { "X-WA-Role": "operator", "X-WA-Site": "plant-east" };

/** 统一的请求函数：非 2xx 时抛出后端返回的错误信息 */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const extra = (init?.headers as Record<string, string> | undefined) ?? {};
  const res = await fetch(path, {
    ...init,
    headers: { "Content-Type": "application/json", ...extra },
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.message ?? body.error ?? `HTTP ${res.status}`);
  return body as T;
}

export const api = {
  health: () => request<Health>("/api/health"),
  conversations: () => request<Conversation[]>("/api/conversations"),
  createConversation: (title?: string) =>
    request<Conversation>("/api/conversations", {
      method: "POST",
      body: JSON.stringify(title ? { title } : {}),
    }),
  /** 消息列表：每条消息多一个 card（AG.4 对话里的卡片） */
  messages: (cid: string) => request<MessageView[]>(`/api/conversations/${cid}/messages`),
  /** 在会话里生成确认卡（选择卡点选、结果卡撤销时用） */
  proposeInConversation: (cid: string, change: ChangeRequest) =>
    request<MessageView>(`/api/conversations/${cid}/agent-proposals`, { method: "POST", body: JSON.stringify(change) }),
  send: (cid: string, text: string) =>
    request<TurnOutcome>(`/api/conversations/${cid}/messages`, { method: "POST", body: JSON.stringify({ text }) }),
  /** 运营频道未读数；开关关闭时接口不存在（404），调用方当作没有运营频道 */
  opsUnread: () => request<{ unread: number }>("/api/mom/desk/unread"),
  opsRead: () => request<{ unread: number }>("/api/mom/desk/read", { method: "POST" }),
  /** 运营桌面读模型（D1-2）；开关关闭时 404 */
  opsDesk: () => request<OpsDeskView>("/api/mom/desk"),
  opsDeskCase: (id: string) => request<OpsDeskCaseDetail>(`/api/mom/desk/cases/${encodeURIComponent(id)}`),
  opsOpenInChannel: (id: string) =>
    request<{ message_id: string; conversation_id: string }>(
      `/api/mom/desk/cases/${encodeURIComponent(id)}/open-in-channel`,
      { method: "POST", body: "{}" },
    ),
  /** 运营频道卡片批准（决定 60；批准即执行见决定 2026-11-26-ops-approve-auto-execute）；过期方案回 409 */
  opsApprove: (id: string, body: { scope: string; plan_revision: number }) =>
    request<{ already: boolean; approver: string; executed?: boolean; blocked_reason?: string }>(
      `/api/mom/desk/cases/${encodeURIComponent(id)}/approve`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  /** 业务内省未读；403 时由调用方处理 */
  introUnread: () =>
    request<{ unread: number; conversation_id: string | null; title: string }>("/api/introspection/unread"),
  introRead: () => request<{ ok: boolean; marked: number; unread: number }>("/api/introspection/read", { method: "POST" }),
  introFindings: (includeRejected = false) =>
    request<{ findings: IntroFinding[] }>(
      `/api/introspection/findings${includeRejected ? "?include_rejected=true" : ""}`,
    ),
  introFinding: (id: string) =>
    request<{ finding: IntroFinding; labels: IntroLabel[] }>(`/api/introspection/findings/${encodeURIComponent(id)}`),
  introVerdict: (id: string, verdict: "confirmed" | "rejected", note = "") =>
    request<{ ok: boolean; finding: IntroFinding }>(`/api/introspection/findings/${encodeURIComponent(id)}/verdict`, {
      method: "POST",
      body: JSON.stringify({ verdict, note }),
    }),
  introLabel: (id: string, kind: "should_push" | "should_not_push", note = "") =>
    request<{ ok: boolean; label: IntroLabel }>(`/api/introspection/findings/${encodeURIComponent(id)}/label`, {
      method: "POST",
      body: JSON.stringify({ kind, note }),
    }),
  introRun: (fixture?: string) =>
    request<{ status: string; job_id?: string; reason?: string; error?: string }>("/api/introspection/run", {
      method: "POST",
      body: JSON.stringify(fixture ? { fixture } : {}),
    }),
  tasks: (cid: string) => request<TaskRecord[]>(`/api/tasks?conversation_id=${cid}`),
  /** 全量任务列表（侧栏「任务」分区用；GET /api/tasks 无 conversation_id 过滤） */
  tasksAll: () => request<TaskRecord[]>("/api/tasks"),
  task: (id: string, after?: string) => request<TaskDetail>(`/api/tasks/${id}${after ? `?after=${encodeURIComponent(after)}` : ""}`),
  cancel: (id: string) => request<{ cancelled: boolean }>(`/api/tasks/${id}/cancel`, { method: "POST" }),
  approve: (id: string) => request<{ approved: boolean }>(`/api/tasks/${id}/approve`, { method: "POST" }),
  reviewVerdict: (id: string, verdict: "accept" | "reject", note = "") =>
    request<{ ok: boolean; verdict: string }>(`/api/tasks/${id}/review-verdict`, {
      method: "POST",
      body: JSON.stringify({ verdict, note }),
    }),
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
  /** 排查（AG.5 第 1、2 层）：握手检测 + 本机体检 + 规则结论，不外发 */
  diagnoseAgent: (id: string) =>
    request<DiagnoseReport>("/api/agents/diagnose", { method: "POST", body: JSON.stringify({ id }) }),
  /** 排查第 4 层：生成预览（将发送的完整内容 + 一次性同意凭证），不发送 */
  diagnosePreview: (id: string, helper: string) =>
    request<DiagnosePreview>("/api/agents/diagnose/preview", { method: "POST", body: JSON.stringify({ id, helper }) }),
  /** 排查第 4 层：凭同意凭证发送这一次 */
  diagnoseSend: (p: DiagnosePreview) =>
    request<DiagnoseSendResult>("/api/agents/diagnose/send", {
      method: "POST",
      body: JSON.stringify({ consent_id: p.consent_id, consent_token: p.consent_token }),
    }),
  probeAgent: (id: string) =>
    request<ProbeOutcome>("/api/agents/probe", { method: "POST", body: JSON.stringify({ id }) }),
  proposeAgentChange: (change: ChangeRequest) =>
    request<Proposal>("/api/agents/proposals", { method: "POST", body: JSON.stringify(change) }),
  applyAgentProposal: (id: string, token: string) =>
    request<Applied>(`/api/agents/proposals/${encodeURIComponent(id)}/apply`, {
      method: "POST",
      body: JSON.stringify({ token }),
    }),
  /** World Adapter M0 实验 Mock（决定 62）；未开 world_mock 时 404 */
  worldMock: {
    saveConfig: (body: {
      factory: string;
      scenario: string;
      seed: number;
      duration_minutes: number;
      request_id?: string;
    }) =>
      request<WorldMockConfigResult>("/api/world/mock/experiment/configs", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    startTrial: (body: { config_version: string; request_id?: string }) =>
      request<WorldMockTrialResult>("/api/world/mock/experiment/trials", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    startOfficial: (body: {
      config_version: string;
      target_channel: string;
      start_key: string;
      request_id?: string;
    }) =>
      request<WorldMockOfficialResult>("/api/world/mock/experiment/official", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    getRun: (runId: string) =>
      request<WorldMockRun>(`/api/world/mock/experiment/runs/${encodeURIComponent(runId)}`),
    controlRun: (runId: string, body: { action: string; minutes?: number; request_id?: string }) =>
      request<WorldMockRun>(`/api/world/mock/experiment/runs/${encodeURIComponent(runId)}`, {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    approveOps: (body: { case_id: string; plan_revision: number; request_id?: string }) =>
      request<{ already: boolean; approver: string }>("/api/world/mock/ops/approve", {
        method: "POST",
        headers: { "X-WA-Role": "operator" },
        body: JSON.stringify(body),
      }),
    ensureOpsSeed: () =>
      request<{ seeded: boolean; case_id?: string }>("/api/world/mock/ops/ensure-seed", {
        method: "POST",
        headers: { "X-WA-Role": "operator" },
        body: "{}",
      }),
  },
  /** World Adapter M1 远程 DES 读；未开 world_mock / DES 门控时 404 */
  worldDes: {
    bind: (body: {
      factory?: string;
      scenario?: string;
      seed?: number;
      run_id?: string;
      request_id?: string;
    }) =>
      request<WorldDesBindResult>("/api/world/des/bind", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    observe: (body: { world_id: string; request_id?: string }) =>
      request<WorldDesObserveResult>("/api/world/des/observe", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    subscribe: (body: { world_id: string; cursor: string; request_id?: string }) =>
      request<WorldDesObserveResult>("/api/world/des/subscribe", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    step: (body: { world_id: string; minutes?: number; request_id?: string }) =>
      request<{ publish_seq?: number; published?: boolean }>("/api/world/des/experiment/step", {
        method: "POST",
        headers: WA_ROLE,
        body: JSON.stringify(body),
      }),
    approveExecute: (body: { case_id: string; plan_revision: number; request_id?: string }) =>
      request<{
        already: boolean;
        approver: string;
        execution_id?: string;
        world_id?: string;
        provenance?: string;
      }>("/api/world/des/ops/approve-execute", {
        method: "POST",
        headers: { "X-WA-Role": "operator" },
        body: JSON.stringify(body),
      }),
    confirm: (body: { world_id: string; execution_id: string; request_id?: string }) =>
      request<{
        execution_id: string;
        status: string;
        provenance?: string;
      }>("/api/world/des/confirm", {
        method: "POST",
        headers: { "X-WA-Role": "operator" },
        body: JSON.stringify(body),
      }),
  },
  /** World Adapter M3 生产只读；未开 world_mock 时 404 */
  worldProd: {
    bind: (body: {
      site_id: string;
      authority: string;
      request_id?: string;
      sources?: string[];
    }) =>
      request<WorldProdBindResult>("/api/world/prod/bind", {
        method: "POST",
        headers: WA_OPERATOR,
        body: JSON.stringify(body),
      }),
    observe: (body: { world_id: string; request_id?: string }) =>
      request<WorldProdObserveResult>("/api/world/prod/observe", {
        method: "POST",
        headers: WA_OPERATOR,
        body: JSON.stringify(body),
      }),
    subscribe: (body: { world_id: string; cursor: string; request_id?: string }) =>
      request<WorldProdObserveResult>("/api/world/prod/subscribe", {
        method: "POST",
        headers: WA_OPERATOR,
        body: JSON.stringify(body),
      }),
    expireCursor: (body: { world_id: string; request_id?: string }) =>
      request<{ cursor_generation?: number }>("/api/world/prod/test/expire-cursor", {
        method: "POST",
        headers: WA_OPERATOR,
        body: JSON.stringify(body),
      }),
    injectDelay: (body: {
      world_id: string;
      source: string;
      delay_ms: number;
      request_id?: string;
    }) =>
      request<{ as_of?: string }>("/api/world/prod/test/inject-delay", {
        method: "POST",
        headers: { "X-WA-Role": "experimenter" },
        body: JSON.stringify(body),
      }),
  },

  /** AI Desktop C1：证据 ACL（夹具头 x-desktop-principal） */
  desktopEvidenceList: (taskId: string, principal: string) =>
    request<{ principal: string; evidence: DesktopEvidenceDto[] }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/evidence`,
      { headers: { "x-desktop-principal": principal } },
    ),
  desktopEvidenceGet: async (
    id: string,
    principal: string,
    view: "summary" | "source" | "download",
  ): Promise<
    | { ok: true; evidence: DesktopEvidenceDto }
    | { ok: false; status: number; code: string; message: string }
  > => {
    const res = await fetch(
      `/api/desktop/v1/evidence/${encodeURIComponent(id)}?view=${encodeURIComponent(view)}`,
      { headers: { "Content-Type": "application/json", "x-desktop-principal": principal } },
    );
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      return {
        ok: false,
        status: res.status,
        code: String(body.code ?? "error"),
        message: String(body.message ?? body.error ?? `HTTP ${res.status}`),
      };
    }
    return { ok: true, evidence: body.evidence as DesktopEvidenceDto };
  },
  desktopEvidenceSearch: (q: string, principal: string) =>
    request<{ principal: string; evidence: DesktopEvidenceDto[] }>(
      `/api/desktop/v1/evidence/search?q=${encodeURIComponent(q)}`,
      { headers: { "x-desktop-principal": principal } },
    ),
  desktopEvidenceRevoke: (id: string, principalId: string) =>
    request<{ revoked: boolean }>(`/api/desktop/v1/evidence/${encodeURIComponent(id)}/revoke`, {
      method: "POST",
      body: JSON.stringify({ principal_id: principalId }),
    }),

  /** AI Desktop C2：动作审批网关 */
  desktopApprovalGet: (taskId: string) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/approvals`,
    ),
  desktopApprovalReset: (taskId: string) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/approvals/reset`,
      { method: "POST", body: "{}" },
    ),
  desktopApprovalDecide: (id: string, decision: "approve" | "reject", expectedRevision: number) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/approvals/${encodeURIComponent(id)}/decide`,
      {
        method: "POST",
        body: JSON.stringify({ decision, expected_revision: expectedRevision }),
      },
    ),
  desktopApprovalBump: (id: string, params: string) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/approvals/${encodeURIComponent(id)}/bump`,
      { method: "POST", body: JSON.stringify({ params }) },
    ),
  desktopApprovalExecute: async (
    id: string,
    expectedRevision: number,
    forceUnknown = false,
  ): Promise<
    | { ok: true; approval: DesktopApprovalDto }
    | { ok: false; message: string; approval?: DesktopApprovalDto }
  > => {
    const res = await fetch(`/api/desktop/v1/approvals/${encodeURIComponent(id)}/execute`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ expected_revision: expectedRevision, force_unknown: forceUnknown }),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      return {
        ok: false,
        message: String(body.message ?? `HTTP ${res.status}`),
        approval: body.approval as DesktopApprovalDto | undefined,
      };
    }
    return { ok: true, approval: body.approval as DesktopApprovalDto };
  },
  desktopApprovalAckUnknown: (id: string) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/approvals/${encodeURIComponent(id)}/ack-unknown`,
      { method: "POST", body: "{}" },
    ),
  desktopApprovalExpire: (id: string) =>
    request<{ approval: DesktopApprovalDto }>(
      `/api/desktop/v1/approvals/${encodeURIComponent(id)}/expire`,
      { method: "POST", body: "{}" },
    ),

  /** AI Desktop T-99：关联修正审计（服务端权威；用户事件不播种） */
  desktopLinkAuditsList: (taskId: string) =>
    request<{ items: DesktopLinkAuditDto[] }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/link-audits`,
    ),
  desktopLinkAuditCreate: (
    taskId: string,
    body: { source: string; action: "link" | "revoke" },
  ) =>
    request<{ audit: DesktopLinkAuditDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/link-audits`,
      { method: "POST", body: JSON.stringify(body) },
    ),

  /** AI Desktop T-82：首页快照（AD-G7 三区卡片读模型） */
  desktopSnapshot: () => request<DesktopSnapshotDto>("/api/desktop/v1/snapshot"),

  /** AI Desktop T-86：工作区草稿上云（服务端权威 + 乐观版本） */
  desktopDraftGet: (kind: "task" | "conv", id: string) =>
    request<{ draft: DesktopDraftDto }>(
      `/api/desktop/v1/drafts/${kind}/${encodeURIComponent(id)}`,
    ),
  desktopDraftSave: (
    kind: "task" | "conv",
    id: string,
    body: {
      text: string;
      scope: string;
      continue_task_id: string | null;
      version: number;
      expected_version?: number;
    },
  ) =>
    request<{ draft: DesktopDraftDto }>(
      `/api/desktop/v1/drafts/${kind}/${encodeURIComponent(id)}`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  desktopDraftClearAfterSend: (kind: "task" | "conv", id: string, sentVersion: number) =>
    request<{ draft: DesktopDraftDto }>(
      `/api/desktop/v1/drafts/${kind}/${encodeURIComponent(id)}?sent_version=${sentVersion}`,
      { method: "DELETE" },
    ),

  /** AI Desktop C2：Attention 投递 */
  desktopAttentionList: () =>
    request<{ mute_ordinary: boolean; items: DesktopAttentionDto[] }>("/api/desktop/v1/attention"),
  desktopAttentionPost: (body: {
    kind: "ordinary" | "digest" | "urgent";
    title: string;
    body: string;
    task_id?: string;
  }) =>
    request<{ delivered: boolean; item: DesktopAttentionDto | null }>("/api/desktop/v1/attention", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  desktopAttentionMute: (muteOrdinary: boolean) =>
    request<{ mute_ordinary: boolean }>("/api/desktop/v1/attention/mute", {
      method: "POST",
      body: JSON.stringify({ mute_ordinary: muteOrdinary }),
    }),
  desktopAttentionDismiss: (id: string) =>
    request<{ dismissed: boolean }>(`/api/desktop/v1/attention/${encodeURIComponent(id)}`, {
      method: "DELETE",
    }),

  /** AI Desktop B1：可靠受理（T-61 面板接真） */
  desktopAcceptCreate: async (
    conversationId: string,
    body: {
      client_operation_id: string;
      text: string;
      scope?: string;
      task_id?: string;
    },
  ): Promise<
    | { ok: true; status: number; body: DesktopAcceptDto }
    | { ok: false; status: number; code: string; message: string; request_id?: string; message_id?: string }
  > => {
    const res = await fetch(
      `/api/desktop/v1/conversations/${encodeURIComponent(conversationId)}/requests`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          client_operation_id: body.client_operation_id,
          text: body.text,
          scope: body.scope ?? "continue",
          task_id: body.task_id,
        }),
      },
    );
    const data = await res.json().catch(() => ({}));
    if (res.status === 202) {
      return { ok: true, status: 202, body: data as DesktopAcceptDto };
    }
    return {
      ok: false,
      status: res.status,
      code: String(data.code ?? "error"),
      message: String(data.message ?? `HTTP ${res.status}`),
      request_id: data.request_id as string | undefined,
      message_id: data.message_id as string | undefined,
    };
  },
  desktopAcceptGet: (requestId: string) =>
    request<DesktopRequestRecordDto>(
      `/api/desktop/v1/requests/${encodeURIComponent(requestId)}`,
    ),

  /** AI Desktop D：语音会话（fixture 媒体） */
  desktopVoiceGet: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice`,
    ),
  desktopVoiceReset: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/reset`,
      { method: "POST", body: "{}" },
    ),
  desktopVoicePermission: (taskId: string, permission: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/permission`,
      { method: "POST", body: JSON.stringify({ permission }) },
    ),
  desktopVoiceStart: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/start`,
      { method: "POST", body: "{}" },
    ),
  desktopVoicePartial: (taskId: string, text: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/partial`,
      { method: "POST", body: JSON.stringify({ text }) },
    ),
  desktopVoiceFinalize: (taskId: string, text: string, linkTask = true) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/finalize`,
      { method: "POST", body: JSON.stringify({ text, link_task: linkTask }) },
    ),
  desktopVoicePlay: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/play`,
      { method: "POST", body: "{}" },
    ),
  desktopVoiceBarge: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/barge`,
      { method: "POST", body: "{}" },
    ),
  desktopVoiceOffline: (taskId: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/offline`,
      { method: "POST", body: "{}" },
    ),
  desktopVoiceAsText: (taskId: string, text: string) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/as-text`,
      { method: "POST", body: JSON.stringify({ text }) },
    ),
  desktopVoiceShare: (taskId: string, share: boolean) =>
    request<{ session: DesktopVoiceSessionDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/voice/share`,
      { method: "POST", body: JSON.stringify({ share }) },
    ),

  /** AI Desktop D：跨端续接 */
  desktopHandoffGet: (taskId: string) =>
    request<{ handoff: DesktopHandoffDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/handoff`,
    ),
  desktopHandoffReset: (taskId: string) =>
    request<{ handoff: DesktopHandoffDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/handoff/reset`,
      { method: "POST", body: "{}" },
    ),
  desktopHandoffDraft: (taskId: string, device: "phone" | "desktop", text: string) =>
    request<{ handoff: DesktopHandoffDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/handoff/draft`,
      { method: "POST", body: JSON.stringify({ device, text }) },
    ),
  desktopHandoffResolve: (taskId: string, chosen: "phone" | "desktop") =>
    request<{ handoff: DesktopHandoffDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/handoff/resolve`,
      { method: "POST", body: JSON.stringify({ chosen }) },
    ),
  desktopHandoffShareVoice: (taskId: string, share: boolean) =>
    request<{ handoff: DesktopHandoffDto }>(
      `/api/desktop/v1/tasks/${encodeURIComponent(taskId)}/handoff/share-voice`,
      { method: "POST", body: JSON.stringify({ share }) },
    ),

  /** AI Desktop T-64：Resume fixture + B2 SSE */
  desktopResumeSeed: () =>
    request<DesktopResumeSeedDto>("/api/desktop/v1/resume/fixture/seed", {
      method: "POST",
      body: "{}",
    }),
  desktopResumeReplayA: (conversationId: string) =>
    request<{ ok: boolean; conversation_id: string }>(
      "/api/desktop/v1/resume/fixture/replay-a",
      { method: "POST", body: JSON.stringify({ conversation_id: conversationId }) },
    ),
  desktopResumeContinueB: (conversationId: string) =>
    request<{ ok: boolean; conversation_id: string }>(
      "/api/desktop/v1/resume/fixture/continue-b",
      { method: "POST", body: JSON.stringify({ conversation_id: conversationId }) },
    ),

  /** AI Desktop B3 / T-70：个人助理 preferences */
  desktopPreferencesGet: () =>
    request<DesktopPreferencesDto>("/api/desktop/v1/preferences"),
  /** AI Desktop T-78：记忆列表（SQLite 权威；fixture/reset 仅测试） */
  desktopMemoriesList: () =>
    request<{ items: DesktopMemoryDto[] }>("/api/desktop/v1/memories"),
  desktopMemoryGet: (id: string) =>
    request<DesktopMemoryDto>(`/api/desktop/v1/memories/${encodeURIComponent(id)}`),
  desktopMemoryPatch: (id: string, body: { title?: string; body?: string; expires?: string }) =>
    request<DesktopMemoryDto>(`/api/desktop/v1/memories/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  desktopMemoryDelete: (id: string) =>
    request<{ ok: boolean; id: string; revoked: boolean }>(
      `/api/desktop/v1/memories/${encodeURIComponent(id)}`,
      { method: "DELETE" },
    ),
  desktopMemoriesReset: () =>
    request<{ ok: boolean; reset: boolean }>("/api/desktop/v1/memories/fixture/reset", {
      method: "POST",
      body: "{}",
    }),
  /** AI Desktop T-73：助理消息已读跨端（服务夹具权威） */
  desktopAssistantMessagesList: () =>
    request<{ items: DesktopAssistantMessageDto[]; unread_count: number }>(
      "/api/desktop/v1/assistant/messages",
    ),
  desktopAssistantMessagesGetReads: () =>
    request<{ read_ids: string[] }>("/api/desktop/v1/assistant/messages/read"),
  desktopAssistantMessagesRead: (messageIds: string[]) =>
    request<{ read_ids: string[]; newly_marked: number }>(
      "/api/desktop/v1/assistant/messages/read",
      { method: "POST", body: JSON.stringify({ message_ids: messageIds }) },
    ),
  desktopAssistantMessagesReset: () =>
    request<{ ok: boolean; reset: boolean }>(
      "/api/desktop/v1/assistant/messages/fixture/reset",
      { method: "POST", body: "{}" },
    ),
  desktopPreferencesPatch: async (body: {
    expected_version: number;
    name?: string;
    avatar?: string;
    role?: string;
    soul?: string;
    taste?: string;
    memory_principle?: string;
    proactivity?: string;
  }): Promise<
    | { ok: true; body: DesktopPreferencesDto }
    | { ok: false; status: number; code: string; message: string; current?: DesktopPreferencesDto }
  > => {
    const res = await fetch("/api/desktop/v1/preferences", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({}));
    if (res.ok) return { ok: true, body: data as DesktopPreferencesDto };
    return {
      ok: false,
      status: res.status,
      code: String(data.code ?? "error"),
      message: String(data.message ?? `HTTP ${res.status}`),
      current: data.current as DesktopPreferencesDto | undefined,
    };
  },
};

export type DesktopEvidenceDto = {
  id: string;
  task_id: string;
  title: string;
  object_label: string;
  collected_at: string;
  source_type: string;
  version: string;
  status: string;
  summary: string;
  source_body: string;
  error_message: string;
};

export type DesktopApprovalDto = {
  id: string;
  task_id: string;
  plan_revision: number;
  params: string;
  status: string;
  decided_by: string | null;
  execute_count: number;
  last_receipt: string;
};

export type DesktopAttentionDto = {
  id: string;
  kind: string;
  title: string;
  body: string;
  task_id: string | null;
  created_at: string;
};

export type DesktopLinkAuditDto = {
  id: string;
  task_id: string;
  source: string;
  action: string;
  created_at: string;
};

export type DesktopDraftDto = {
  text: string;
  version: number;
  scope: string;
  continue_task_id: string | null;
  updated_at: string;
};

export type DesktopSnapshotCardDto = {
  id: string;
  zone: string;
  card_type: string;
  entity_id: string;
  title: string;
  subtitle: string;
  badge: string;
  priority: string;
  simulated: boolean;
  updated_at: string;
  source_refs: string[];
};

export type DesktopSnapshotDto = {
  cards: DesktopSnapshotCardDto[];
  counts: { tasks_active: number; attention_unread: number };
  cursor: string;
  generated_at: string;
  /** UXFIX-B（INTENT-003）：服务端告知测试模式（与 desktop.test_fixtures 一致）；
   *  旧后端可能不带该字段，按 false 处理（正常界面不显示测试控件） */
  test_fixtures?: boolean;
};

export type DesktopAcceptDto = {
  request_id: string;
  message_id: string;
  status: string;
  task_id?: string | null;
  dispatch_count: number;
  replayed: boolean;
};

export type DesktopRequestRecordDto = {
  request_id: string;
  message_id: string;
  conversation_id: string;
  client_operation_id: string;
  text: string;
  scope: string;
  task_id: string | null;
  status: string;
  dispatch_count: number;
};

export type DesktopVoiceSessionDto = {
  id: string;
  task_id: string;
  permission: string;
  phase: string;
  playing: boolean;
  partial: string;
  final_text: string;
  linked_task_id: string | null;
  share_transcript: boolean;
};

export type DesktopHandoffDto = {
  task_id: string;
  conversation_id: string;
  run_id: string;
  read_message_id: string;
  phone_text: string;
  phone_version: number;
  desktop_text: string;
  desktop_version: number;
  conflict: boolean;
  chosen: string | null;
  voice_shared: boolean;
};

export type DesktopPreferencesDto = {
  user_id: string;
  version: number;
  name: string;
  avatar: string;
  role: string;
  soul: string;
  taste: string;
  memory_principle: string;
  proactivity: string;
  updated_at: string;
};

export type DesktopMemoryDto = {
  id: string;
  title: string;
  source: string;
  body: string;
  expires: string;
  /** UXFIX-T（#353）：服务端已序列化的类型与确认状态 */
  kind: string;
  confirmed: boolean;
};

export type DesktopAssistantMessageDto = {
  id: string;
  kind: string;
  title: string;
  body: string;
  read: boolean;
};

export type DesktopResumeRunDto = {
  task_id: string;
  status: string;
  progress: string;
  version: number;
};

export type DesktopResumeSeedDto = {
  conversation_id: string;
  after_cursor: string;
  task_a: string;
  task_b: string;
  runs: DesktopResumeRunDto[];
};

export type DesktopEventEnvelopeDto = {
  event_id: string;
  cursor: string;
  conversation_id: string;
  task_id?: string | null;
  kind: string;
  status?: string | null;
  message: string;
  stream_id?: string | null;
  created_at: string;
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

/** B2：桌面游标 SSE（`desktop_event`） */
export function subscribeDesktopEvents(
  conversationId: string,
  after: string | undefined,
  onEvent: (e: DesktopEventEnvelopeDto) => void,
): () => void {
  const q = new URLSearchParams({ conversation_id: conversationId });
  if (after) q.set("after", after);
  const es = new EventSource(`/api/desktop/v1/events?${q.toString()}`);
  es.addEventListener("desktop_event", (msg) => {
    try {
      onEvent(JSON.parse((msg as MessageEvent).data) as DesktopEventEnvelopeDto);
    } catch {
      /* 忽略 */
    }
  });
  return () => es.close();
}
