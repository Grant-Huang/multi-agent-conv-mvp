// 运营频道里的卡片（docs/mom/OPS_DESK.md，决定 60）：引擎状态快照；批准只在 OpsCase 卡上。
// World Adapter M0-3：world_mock 投影卡含方案/执行/核验入口（DES-UI）。
// World Adapter M2-5：provenance=des 走 approve-execute / 查证结果 / execution-evidence。
// UXFIX-P（#341，INTENT-2026-026）：执行回执与核验证据从引擎读取（CH-P6），
// 分层显示 已提交 → 源系统已受理 → 已核验，标来源；未执行时显示“尚未执行”（CH-P8）。
import { useEffect, useState } from "react";
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

/** 引擎案例视图（GET /api/mom/desk/cases/{id} → human_view）中回执相关字段的窄化类型 */
type OpsActionView = {
  key: string;
  tool: string;
  state: string;
  accepted_at: number;
  complete_at: number;
  receipt: string;
  verification: string | null;
};
type OpsOutboxView = { key?: string; sent_at?: number; provenance?: string };
type OpsEvidenceView = { source?: string; provenance?: string };
type OpsCaseView = {
  actions?: Record<string, OpsActionView>;
  outbox?: OpsOutboxView[];
  evidence?: Record<string, OpsEvidenceView>;
  provenance?: string;
};

const PROV_LABEL: Record<string, string> = {
  mock: "模拟",
  world_mock: "模拟",
  simulation: "仿真",
  des: "仿真",
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
  // UXFIX-P：引擎案例视图（回执 / 证据 / 来源），null=加载中，"error"=读取失败
  const [caseView, setCaseView] = useState<OpsCaseView | null | "error">(null);
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

  // 引擎回执数据：非 DES 案例从 human_view 读取；阶段变化时重读（就地更新）
  useEffect(() => {
    if (isDes) return;
    let cancelled = false;
    api
      .opsDeskCase(card.case_id)
      .then((d) => {
        if (!cancelled) setCaseView(d as OpsCaseView);
      })
      .catch(() => {
        if (!cancelled) setCaseView("error");
      });
    return () => {
      cancelled = true;
    };
  }, [card.case_id, card.stage, isDes]);

  const provLabel = PROV_LABEL[caseView && caseView !== "error" ? (caseView.provenance ?? "") : ""] ?? PROV_LABEL[card.provenance] ?? "模拟";

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
        <div className="hint" data-testid="execution-receipt">
          {isDes ? (
            <p>
              执行回执（仿真）：已提交 → 已受理（{executionId}）
              {confirmedEvidence ? ` → 已核验（${confirmedEvidence}）` : ""}
            </p>
          ) : caseView === null ? (
            <p>执行回执：读取中…</p>
          ) : caseView === "error" ? (
            <p className="bad-text">执行回执：暂不可获取（引擎无此案例记录或读取失败）。</p>
          ) : (caseView.actions == null || Object.keys(caseView.actions).length === 0) &&
            (caseView.outbox == null || caseView.outbox.length === 0) ? (
            <p>
              执行回执（{provLabel}）：尚未执行
            </p>
          ) : (
            <>
              <p>
                执行回执（{provLabel}，来自引擎）：
              </p>
              <ul>
                {(caseView.outbox ?? []).map((o, i) => (
                  <li key={`outbox-${i}`}>
                    已提交
                    {o.sent_at != null ? `（t=${o.sent_at}）` : ""}
                    {o.key ? ` · ${o.key}` : ""}
                  </li>
                ))}
                {Object.values(caseView.actions ?? {}).map((a) => (
                  <li key={`action-${a.key}`}>
                    {a.key} · {a.tool}：已提交 → 源系统已受理（回执 {a.receipt} · t={a.accepted_at}）
                    {a.verification != null ? ` → 已核验（t=${a.complete_at} · ${a.verification}）` : ""}
                    {a.state === "FAILED" ? " · 执行失败" : ""}
                    {a.state === "UNKNOWN" ? " · 受理状态未知（未收到回执）" : ""}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
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
        <div className="hint" data-testid="verify-evidence">
          {isDes ? (
            <p>核验证据（仿真）：{confirmedEvidence ?? executionId ?? "暂无核验证据"}</p>
          ) : caseView === null ? (
            <p>核验证据：读取中…</p>
          ) : caseView === "error" ? (
            <p className="bad-text">核验证据：暂不可获取（引擎无此案例记录或读取失败）。</p>
          ) : caseView.evidence && Object.keys(caseView.evidence).length > 0 ? (
            <>
              <p>核验证据（来自引擎）：</p>
              <ul>
                {Object.entries(caseView.evidence).map(([k, v]) => (
                  <li key={k}>
                    {k} · {v.source ?? "来源未知"}
                    {v.provenance ? `（${PROV_LABEL[v.provenance] ?? v.provenance}）` : ""}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p>核验证据（{provLabel}）：暂无核验证据（未执行或未核验）。</p>
          )}
        </div>
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
