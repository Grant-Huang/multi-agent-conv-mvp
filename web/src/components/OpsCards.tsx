// 运营频道里的卡片（docs/mom/OPS_DESK.md，决定 60）：引擎状态快照；批准只在 OpsCase 卡上。
import { useState } from "react";
import { api } from "../api";
import type { ChatCard } from "../bindings/ChatCard";

const STAGE_LABEL: Record<string, string> = {
  WAITING_HUMAN: "待批准",
  BLOCKED: "受阻",
  VERIFYING: "核验中",
  CLOSED: "已关闭",
  CANCELLED: "已取消",
  BUDGET_EXHAUSTED: "预算耗尽",
  INVESTIGATING: "调查中",
  ASSESS: "评估中",
};

type OpsCase = Extract<ChatCard, { type: "ops_case" }>;
type OpsSummary = Extract<ChatCard, { type: "ops_summary" }>;

export function OpsCaseCard({ card, onChanged }: { card: OpsCase; onChanged?: () => void }) {
  const stage = STAGE_LABEL[card.stage] ?? card.stage;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [localApprover, setLocalApprover] = useState<string | null>(null);
  const approver = localApprover ?? card.approver;
  const planRevision =
    card.plan_revision == null ? null : Number(card.plan_revision);
  const canApprove =
    card.stage === "WAITING_HUMAN" && !approver && !!card.scope && planRevision != null && !Number.isNaN(planRevision);

  const approve = async () => {
    if (!card.scope || planRevision == null) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.opsApprove(card.case_id, {
        scope: card.scope,
        plan_revision: planRevision,
      });
      setLocalApprover(r.approver);
      onChanged?.();
    } catch (e) {
      const msg = String(e);
      if (msg.includes("stale") || msg.includes("expired") || msg.includes("409") || msg.includes("HTTP 409")) {
        setError("方案已更新，请刷新");
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className={`agent-card ops-card level-${card.level.toLowerCase()}`}
      role="region"
      aria-label={`运营案例 ${card.case_id}`}
    >
      <h3>
        <span className="badge">{card.level}</span> {card.title} <span className="badge sim">模拟</span>
      </h3>
      <p>
        案例 {card.case_id} · {stage}
      </p>
      <p className="hint">{card.summary}</p>
      <p className="hint">
        责任：{card.owner}
        {approver ? ` · 批准人：${approver}` : ""}
      </p>
      {canApprove && (
        <button type="button" className="btn small" disabled={busy} onClick={approve}>
          批准
        </button>
      )}
      {error && (
        <p className="hint" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

export function OpsSummaryCard({ card }: { card: OpsSummary }) {
  return (
    <section className="agent-card ops-card" role="region" aria-label="运营案例汇总">
      <h3>
        另有 {card.count} 条案例未单独发卡 <span className="badge sim">模拟</span>
      </h3>
      <p className="hint">超过每小时上限，已合并；全部案例见运营桌面。</p>
      <p className="hint">{card.case_ids.join("、")}</p>
    </section>
  );
}
