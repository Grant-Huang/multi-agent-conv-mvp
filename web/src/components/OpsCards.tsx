// 运营频道里的卡片（docs/mom/OPS_DESK.md）：引擎状态的快照，只读；批准不在这里做，沿用已有的批准链路。
// 数据都来自模拟，卡片上必须标明。
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

export function OpsCaseCard({ card }: { card: OpsCase }) {
  const stage = STAGE_LABEL[card.stage] ?? card.stage;
  return (
    <section className={`agent-card ops-card level-${card.level.toLowerCase()}`} role="region" aria-label={`运营案例 ${card.case_id}`}>
      <h3>
        <span className="badge">{card.level}</span> {card.title} <span className="badge sim">模拟</span>
      </h3>
      <p>
        案例 {card.case_id} · {stage}
      </p>
      <p className="hint">{card.summary}</p>
      <p className="hint">
        责任：{card.owner}
        {card.approver ? ` · 批准人：${card.approver}` : ""}
      </p>
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
