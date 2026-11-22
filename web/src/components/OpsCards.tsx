// 运营频道里的卡片（docs/mom/OPS_DESK.md，决定 60）：引擎状态快照；批准只在 OpsCase 卡上。
// World Adapter M0-3：world_mock 投影卡含方案/执行/核验入口（DES-UI）。
// World Adapter M2-5：provenance=des 走 approve-execute / 查证结果 / execution-evidence。
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

type Panel = null | "plan" | "execution" | "verify";

function fieldFromSummary(summary: string | undefined, key: string): string | null {
  if (!summary) return null;
  const m = summary.match(new RegExp(`${key}=([^\\s·]+)`));
  return m?.[1] ?? null;
}

export function OpsCaseCard({ card, onChanged }: { card: OpsCase; onChanged?: () => void }) {
  const stage = STAGE_LABEL[card.stage] ?? card.stage;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [localApprover, setLocalApprover] = useState<string | null>(null);
  const [localExecutionId, setLocalExecutionId] = useState<string | null>(null);
  const [localWorldId, setLocalWorldId] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel>(null);
  const [confirmedEvidence, setConfirmedEvidence] = useState<string | null>(null);
  const approver = localApprover ?? card.approver;
  const planRevision =
    card.plan_revision == null ? null : Number(card.plan_revision);
  const isWorldMock = card.provenance === "world_mock";
  const isDes = card.provenance === "des";
  const executionId =
    localExecutionId ?? fieldFromSummary(card.summary, "execution_id");
  const worldId = localWorldId ?? fieldFromSummary(card.summary, "world_id");
  const canApprove =
    card.stage === "WAITING_HUMAN" && !approver && !!card.scope && planRevision != null && !Number.isNaN(planRevision);
  const showReceipt =
    card.stage === "VERIFYING" ||
    card.stage === "CLOSED" ||
    (card.summary?.includes("回执") ?? false) ||
    panel === "execution";
  const showVerify =
    card.stage === "VERIFYING" ||
    card.stage === "CLOSED" ||
    panel === "verify";
  const showDesEvidence =
    isDes && !!executionId && (confirmedEvidence != null || card.stage === "VERIFYING" || card.stage === "CLOSED");

  const approve = async () => {
    if (!card.scope || planRevision == null) return;
    setBusy(true);
    setError(null);
    try {
      if (isDes) {
        const r = await api.worldDes.approveExecute({
          case_id: card.case_id,
          plan_revision: planRevision,
        });
        setLocalApprover(r.approver);
        if (r.execution_id) setLocalExecutionId(r.execution_id);
        if (r.world_id) setLocalWorldId(r.world_id);
      } else if (isWorldMock) {
        const r = await api.worldMock.approveOps({
          case_id: card.case_id,
          plan_revision: planRevision,
        });
        setLocalApprover(r.approver);
      } else {
        const r = await api.opsApprove(card.case_id, {
          scope: card.scope,
          plan_revision: planRevision,
        });
        setLocalApprover(r.approver);
      }
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

  const verifyResult = async () => {
    if (!isDes || !executionId || !worldId) {
      setError("缺少 world_id 或 execution_id");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await api.worldDes.confirm({
        world_id: worldId,
        execution_id: executionId,
      });
      setConfirmedEvidence(r.execution_id || executionId);
      onChanged?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className={`agent-card ops-card level-${card.level.toLowerCase()}`}
      role="region"
      aria-label={`运营案例 ${card.case_id}`}
      data-testid="ops-case-card"
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
      <div className="ops-card-actions">
        <button type="button" className="btn small" onClick={() => setPanel(panel === "plan" ? null : "plan")}>
          查看方案
        </button>
        {canApprove && (
          <button type="button" className="btn small" disabled={busy} onClick={approve}>
            批准
          </button>
        )}
        <button
          type="button"
          className="btn small"
          onClick={() => setPanel(panel === "execution" ? null : "execution")}
        >
          查看执行
        </button>
        {(approver || card.stage === "VERIFYING" || isWorldMock) && (
          <button
            type="button"
            className="btn small"
            onClick={() => setPanel(panel === "verify" ? null : "verify")}
          >
            查看核验证据
          </button>
        )}
        {isDes && executionId && (
          <button type="button" className="btn small" disabled={busy} onClick={verifyResult}>
            查证结果
          </button>
        )}
      </div>
      {panel === "plan" && (
        <p className="hint" data-testid="plan-revision">
          方案修订 {planRevision ?? "—"} · 范围 {card.scope ?? "—"}
        </p>
      )}
      {showReceipt && (
        <p className="hint" data-testid="execution-receipt">
          执行回执：受理 / 已提交
          {card.stage === "VERIFYING" || card.summary?.includes("回执") ? " / 已确认" : ""}
        </p>
      )}
      {isDes && executionId && (
        <p className="hint" data-testid="execution-id">
          {executionId}
        </p>
      )}
      {showDesEvidence && (
        <p className="hint" data-testid="execution-evidence" data-provenance="des">
          {confirmedEvidence ?? executionId}
        </p>
      )}
      {showVerify && (
        <p className="hint" data-testid="verify-evidence">
          核验证据：现场照片与点检记录（模拟）
        </p>
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
