// AG.4 对话里的卡片：按消息里的结构化卡片数据渲染（不渲染模型生成的 HTML）。
// 卡片上的操作都走接口：选择卡点选 → 在会话里生成确认卡；确认卡点确认 → 用凭证确认，结果卡由后端写回会话。
// 操作完成后调用 onChanged 让对话区重新读取消息。
import { useState } from "react";
import type { ChatCard } from "../bindings/ChatCard";
import type { ChoiceOption } from "../bindings/ChoiceOption";
import { api, type ChangeRequest } from "../api";
import { OpsCaseCard, OpsSummaryCard } from "./OpsCards";
import { ConfirmCard, DiscoveryCard, GuideCard, ResultCard, loadUiLevel, roleLabel } from "./AgentCards";

/** 选择卡的点选 → 对应的修改请求 */
function changeFor(op: string, o: ChoiceOption): ChangeRequest {
  switch (op) {
    case "set_default":
      return { kind: "set_default", role: o.role, ids: [o.id] };
    case "disable":
      return { kind: "disable", id: o.id };
    case "enable":
      return { kind: "enable", id: o.id };
    default:
      return { kind: "add_preset", id: o.id };
  }
}

export function ChatCardView(props: { card: ChatCard; conversationId: string; onChanged: () => void }) {
  const { card, conversationId, onChanged } = props;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState(false);
  const level = loadUiLevel();

  /** 调接口；失败时在卡片下方用一句话说明 */
  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  if (hidden) return null;
  let body: React.ReactNode = null;
  switch (card.type) {
    case "discovery":
      body = <DiscoveryCard catalog={card.items} />;
      break;
    case "choice":
      body = (
        <section className="agent-card" role="region" aria-label="选择助手">
          <h3>选一个助手</h3>
          <ul className="choice-list">
            {card.options.map((o) => (
              <li key={o.id}>
                <button
                  type="button"
                  className="btn small"
                  disabled={busy}
                  onClick={() => run(() => api.proposeInConversation(conversationId, changeFor(card.op, o)))}
                >
                  {o.name}
                </button>{" "}
                <span className="hint">
                  {o.purpose}（{roleLabel(o.role)}
                  {o.installed ? "" : "，还没安装"}）
                </span>
              </li>
            ))}
          </ul>
        </section>
      );
      break;
    case "guide":
      body = <GuideCard item={card.item} onProbe={() => api.probeAgent(card.item.id)} onClose={() => setHidden(true)} />;
      break;
    case "confirm":
      body = (
        <ConfirmCard
          proposal={{
            id: card.proposal_id,
            kind: card.kind,
            summary: card.summary,
            warnings: card.warnings,
            before: card.before,
            after: card.after,
            token: card.token,
            expires_in_secs: Number(card.expires_in_secs),
          }}
          command={card.command ?? undefined}
          level={level}
          busy={busy}
          state={card.state}
          onConfirm={() => run(() => api.applyAgentProposal(card.proposal_id, card.token))}
          onCancel={() => setHidden(true)}
        />
      );
      break;
    case "result":
      body = (
        <ResultCard
          applied={card.ok ? { kind: "", summary: card.summary, probes: card.probes } : undefined}
          error={card.error ?? undefined}
          canUndo={card.ok}
          onUndo={() => run(() => api.proposeInConversation(conversationId, { kind: "revert" }))}
          onClose={() => setHidden(true)}
        />
      );
      break;
    case "ops_case":
      body = <OpsCaseCard card={card} onChanged={onChanged} />;
      break;
    case "ops_summary":
      body = <OpsSummaryCard card={card} />;
      break;
  }
  return (
    <div className="chat-card">
      {body}
      {error && <p className="error-text">{error}</p>}
    </div>
  );
}
