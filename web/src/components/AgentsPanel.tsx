// AG.3 右侧「助手」页签：列出全部助手（预设、config.toml 手写、托管、内置），
// 提供接入 / 停用 / 恢复 / 设为默认 / 移除 / 撤销 / 添加自定义。
// 所有修改都走"提议 → 确认卡 → 确认"两步：确认凭证只在这里使用，对话里的模型拿不到。
import { useCallback, useEffect, useState } from "react";
import type { AgentCard } from "../bindings/AgentCard";
import type { CatalogItem } from "../bindings/CatalogItem";
import { api, type AgentConfigView, type AgentWithLink, type Applied, type ChangeRequest, type Proposal } from "../api";
import { ConfirmCard, DiagnoseCard, GuideCard, ResultCard, loadUiLevel, roleLabel } from "./AgentCards";
import type { DiagnoseReport } from "../bindings/DiagnoseReport";

/** 列表里的一行（把目录、注册表、托管配置合并成同一种结构） */
interface Row {
  id: string;
  name: string;
  purpose: string;
  role: string;
  /** builtin / config_toml / managed / auto / preset */
  source: string;
  registered: boolean;
  disabled: boolean;
  /** 目录里的预设才有（安装步骤、状态） */
  catalog?: CatalogItem;
  /** 自定义托管 agent（不在目录里） */
  custom?: boolean;
  link?: string;
  command?: string;
}

const SOURCE_LABEL: Record<string, string> = {
  builtin: "内置",
  node: "其他电脑",
  config_toml: "在 config.toml 中配置",
  managed: "托管",
  auto: "自动发现",
  preset: "",
};

const LINK_LABEL: Record<string, string> = {
  connected: "已连通",
  configured: "未连通",
  unavailable: "不可用",
};

/** 一行的状态文字 */
function stateText(r: Row): string {
  if (r.disabled) return "已停用";
  if (r.registered) return `已接入 · ${LINK_LABEL[r.link ?? ""] ?? "未检测"}`;
  if (r.catalog?.status === "installed") return "已安装，未接入";
  if (r.catalog?.status === "not_installed") return "未安装";
  return "未接入";
}

/** 合并目录、注册表卡片、连接状态和托管配置 */
function buildRows(catalog: CatalogItem[], cards: AgentWithLink[], view: AgentConfigView): Row[] {
  const rows: Row[] = [];
  const linkOf = (id: string) => cards.find((c) => c.id === id)?.link?.status;
  for (const c of catalog) {
    rows.push({
      id: c.id,
      name: c.name,
      purpose: c.purpose,
      role: c.role,
      source: c.source,
      registered: c.status === "registered",
      disabled: c.disabled,
      catalog: c,
      link: c.link_status ?? linkOf(c.id),
      command: [c.command, ...c.args].join(" "),
    });
  }
  const managedAgents = view.managed.acp.agents ?? [];
  const disabled = view.managed.acp.disabled ?? [];
  for (const card of cards) {
    if (rows.some((r) => r.id === card.id)) continue;
    const custom = managedAgents.find((a) => a.id === card.id);
    const source = view.config_toml_ids.includes(card.id)
      ? "config_toml"
      : custom
        ? "managed"
        : card.kind === "node"
          ? "node"
          : "builtin";
    rows.push({
      id: card.id,
      name: card.name,
      purpose: card.description,
      role: card.role,
      source,
      registered: true,
      disabled: false,
      custom: !!custom,
      link: card.link?.status,
      command: custom ? [custom.command, ...(custom.args ?? [])].join(" ") : undefined,
    });
  }
  // 被停用的自定义托管 agent 不在注册表里，也要列出来（可以恢复启用）
  for (const a of managedAgents) {
    if (rows.some((r) => r.id === a.id)) continue;
    rows.push({
      id: a.id,
      name: a.name ?? a.id,
      purpose: `自定义助手，负责「${roleLabel(a.role)}」`,
      role: a.role,
      source: "managed",
      registered: false,
      disabled: disabled.includes(a.id),
      custom: true,
      command: [a.command, ...(a.args ?? [])].join(" "),
    });
  }
  return rows;
}

/** 当前进行中的卡片 */
type Pending =
  | { kind: "confirm"; proposal: Proposal; command?: string }
  | { kind: "result"; applied?: Applied; error?: string }
  | { kind: "guide"; item: CatalogItem }
  | { kind: "diagnose"; report: DiagnoseReport }
  | null;

export function AgentsPanel() {
  const level = loadUiLevel();
  const [rows, setRows] = useState<Row[]>([]);
  const [view, setView] = useState<AgentConfigView | null>(null);
  const [pending, setPending] = useState<Pending>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [catalog, health, cfg] = await Promise.all([api.agentCatalog(), api.health(), api.agentConfig()]);
      setRows(buildRows(catalog, health.agents, cfg));
      setView(cfg);
      setError(null);
    } catch (e) {
      setError(`读取助手列表失败：${e}`);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  /** 第一步：提议（只校验，不写入）。失败时用结果卡说明原因 */
  const propose = async (change: ChangeRequest, command?: string) => {
    setBusy(true);
    try {
      const proposal = await api.proposeAgentChange(change);
      setPending({ kind: "confirm", proposal, command });
    } catch (e) {
      setPending({ kind: "result", error: String(e).replace(/^Error: /, "") });
    } finally {
      setBusy(false);
    }
  };

  /** 第二步：凭证确认，立即生效 */
  const confirm = async (p: Proposal) => {
    setBusy(true);
    try {
      const applied = await api.applyAgentProposal(p.id, p.token);
      setPending({ kind: "result", applied });
    } catch (e) {
      setPending({ kind: "result", error: String(e).replace(/^Error: /, "") });
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const defaultOf = (role: string) => view?.assign[role]?.[0];

  /** 排查（不外发）：握手检测 + 本机体检 + 规则结论 */
  const diagnose = async (id: string) => {
    setBusy(true);
    try {
      setPending({ kind: "diagnose", report: await api.diagnoseAgent(id) });
    } catch (e) {
      setPending({ kind: "result", error: `排查失败：${String(e).replace(/^Error: /, "")}` });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="task-list agents-panel">
      <div className="agents-toolbar">
        <button
          type="button"
          className="btn small"
          disabled={busy || !view?.can_undo}
          onClick={() => propose({ kind: "revert" })}
        >
          撤销上一次修改
        </button>
        <button type="button" className="btn small" onClick={() => setShowForm((v) => !v)}>
          {showForm ? "收起" : "添加自定义助手"}
        </button>
      </div>
      {view?.load_error && <p className="error-text">{view.load_error}</p>}
      {error && <p className="error-text">{error}</p>}

      {pending?.kind === "confirm" && (
        <ConfirmCard
          proposal={pending.proposal}
          command={pending.command}
          level={level}
          busy={busy}
          onConfirm={() => confirm(pending.proposal)}
          onCancel={() => setPending(null)}
        />
      )}
      {pending?.kind === "result" && (
        <ResultCard
          applied={pending.applied}
          error={pending.error}
          canUndo={!!view?.can_undo}
          onUndo={() => propose({ kind: "revert" })}
          onClose={() => setPending(null)}
        />
      )}
      {pending?.kind === "guide" && (
        <GuideCard item={pending.item} onProbe={() => api.probeAgent(pending.item.id)} onClose={() => setPending(null)} />
      )}
      {pending?.kind === "diagnose" && (
        <DiagnoseCard
          report={pending.report}
          helpers={rows
            .filter(
              (r) =>
                r.registered &&
                !r.disabled &&
                r.link === "connected" &&
                r.id !== pending.report.id &&
                r.source !== "builtin" &&
                r.source !== "node",
            )
            .map((r) => ({ id: r.id, name: r.name }))}
          onPreview={(helper) => api.diagnosePreview(pending.report.id, helper)}
          onSend={(p) => api.diagnoseSend(p)}
          onRecheck={() => diagnose(pending.report.id)}
          onClose={() => setPending(null)}
        />
      )}
      {showForm && <CustomAgentForm busy={busy} onSubmit={(change, command) => propose(change, command)} />}

      <section role="region" aria-label="助手列表" className="agent-list">
        {rows.map((r) => {
          const locked = r.source === "config_toml" || r.source === "builtin" || r.source === "node";
          const canDefault =
            r.registered &&
            !r.disabled &&
            r.role !== "reviewer" &&
            !view?.config_toml_roles.includes(r.role) &&
            defaultOf(r.role) !== r.id;
          return (
            <div key={r.id} className="agent-row" data-agent-id={r.id}>
              <div className="agent-head">
                <strong>{r.name}</strong>
                <span className={`agent-state ${r.disabled ? "off" : r.registered ? r.link ?? "" : "idle"}`}>
                  {stateText(r)}
                </span>
              </div>
              <div className="agent-purpose">
                {r.purpose}
                <span className="agent-role">（{roleLabel(r.role)}）</span>
              </div>
              {SOURCE_LABEL[r.source] && <div className="agent-source">{SOURCE_LABEL[r.source]}</div>}
              {defaultOf(r.role) === r.id && <div className="agent-source">「{roleLabel(r.role)}」的默认助手</div>}
              {r.source !== "builtin" && r.source !== "node" && (
                <div className="card-actions">
                  <button type="button" className="btn small" disabled={busy} onClick={() => diagnose(r.id)}>
                    排查
                  </button>
                </div>
              )}
              {!locked && (
                <div className="card-actions">
                  {!r.registered && !r.disabled && r.catalog && (
                    <button
                      type="button"
                      className="btn small primary"
                      disabled={busy}
                      onClick={() => propose({ kind: "add_preset", id: r.id }, r.command)}
                    >
                      接入
                    </button>
                  )}
                  {!r.registered && !r.disabled && r.catalog && r.catalog.install.length > 0 && (
                    <button type="button" className="btn small" onClick={() => setPending({ kind: "guide", item: r.catalog! })}>
                      看安装步骤
                    </button>
                  )}
                  {r.registered && !r.disabled && (
                    <button type="button" className="btn small" disabled={busy} onClick={() => propose({ kind: "disable", id: r.id })}>
                      停用
                    </button>
                  )}
                  {r.disabled && (
                    <button type="button" className="btn small" disabled={busy} onClick={() => propose({ kind: "enable", id: r.id })}>
                      恢复启用
                    </button>
                  )}
                  {canDefault && (
                    <button
                      type="button"
                      className="btn small"
                      disabled={busy}
                      onClick={() => propose({ kind: "set_default", role: r.role, ids: [r.id] })}
                    >
                      设为默认
                    </button>
                  )}
                  {r.custom && (
                    <button type="button" className="btn small" disabled={busy} onClick={() => propose({ kind: "remove", id: r.id })}>
                      移除
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </section>
    </div>
  );
}

/** 添加自定义助手（专业模式）：环境变量只能写 ${变量名} 引用，后端会再校验一次 */
function CustomAgentForm(props: { busy: boolean; onSubmit: (change: ChangeRequest, command: string) => void }) {
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [role, setRole] = useState<AgentCard["role"]>("coding");
  const [env, setEnv] = useState("");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const argList = args.split(/\s+/).filter(Boolean);
    const envMap = Object.fromEntries(
      env
        .split("\n")
        .map((l) => l.trim())
        .filter(Boolean)
        .map((l) => [l.slice(0, l.indexOf("=")).trim(), l.slice(l.indexOf("=") + 1).trim()]),
    );
    props.onSubmit(
      { kind: "add_custom", agent: { id: id.trim(), name: name.trim() || null, command: command.trim(), args: argList, role, env: envMap } },
      [command.trim(), ...argList].join(" "),
    );
  };
  return (
    <form className="agent-card" aria-label="添加自定义助手" onSubmit={submit}>
      <h3>添加自定义助手</h3>
      <p className="hint">适合已经支持 ACP 协议的助手。提交后会先显示确认卡，确认了才生效。</p>
      <label>
        标识（id）
        <input value={id} onChange={(e) => setId(e.target.value)} required />
      </label>
      <label>
        显示名称
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        启动命令
        <input value={command} onChange={(e) => setCommand(e.target.value)} required placeholder="例如 /usr/local/bin/my-agent" />
      </label>
      <label>
        参数（空格分隔）
        <input value={args} onChange={(e) => setArgs(e.target.value)} placeholder="例如 acp" />
      </label>
      <label>
        负责
        <select value={role} onChange={(e) => setRole(e.target.value as AgentCard["role"])}>
          <option value="coding">写代码</option>
          <option value="general">通用任务</option>
          <option value="reviewer">审查</option>
        </select>
      </label>
      <label>
        环境变量（每行一个，只能写引用，例如 API_KEY=${"{"}MY_API_KEY{"}"}）
        <textarea value={env} onChange={(e) => setEnv(e.target.value)} rows={2} />
      </label>
      <div className="card-actions">
        <button type="submit" className="btn small primary" disabled={props.busy}>
          生成确认卡
        </button>
      </div>
    </form>
  );
}
