import { useCallback, useEffect, useState } from "react";
import { api, type DesktopSnapshotCardDto } from "../api";
import { SMT03_TASK_ID, type DesktopCard } from "./mockData";
import { S1AskBar, type S1BgJob } from "./S1AskBar";

type Props = {
  onOpenTask: (taskId: string, source: string) => void;
  onOpenEvidence: (taskId: string) => void;
  onOpenDesExperiment?: () => void;
  onOpenOpsChannel?: () => void;
  onOpenMomWorkbench?: () => void;
  opsEnabled: boolean;
  assistantName?: string;
  s1BgJob?: S1BgJob | null;
  onS1Ask?: (text: string) => void;
  onS1Delegate?: (text: string) => void;
  onOpenBgTask?: (job: S1BgJob) => void;
  s1Reply?: string | null;
};

function toCard(d: DesktopSnapshotCardDto): DesktopCard {
  return {
    id: d.id,
    zone: d.zone as DesktopCard["zone"],
    title: d.title,
    subtitle: d.subtitle,
    entityId: d.entity_id,
    badge: d.badge,
    simulated: d.simulated,
  };
}

function Zone({
  title,
  cards,
  onOpen,
  onEvidence,
}: {
  title: string;
  cards: DesktopCard[];
  onOpen: (c: DesktopCard) => void;
  onEvidence: (taskId: string) => void;
}) {
  return (
    <section className="desktop-zone" aria-label={title}>
      <h2 className="desktop-zone-title">{title}</h2>
      <ul className="desktop-card-list">
        {cards.map((c) => (
          <li key={c.id}>
            <div className="desktop-card-wrap">
              <button
                type="button"
                className="desktop-card"
                data-testid={c.id}
                onClick={() => onOpen(c)}
              >
                <span className="desktop-card-title">{c.title}</span>
                <span className="desktop-card-sub">{c.subtitle}</span>
                <span className="desktop-card-id">{c.entityId}</span>
                {c.simulated && <span className="desktop-sim">模拟</span>}
              </button>
              {c.entityId === SMT03_TASK_ID && (
                <button
                  type="button"
                  className="btn small desktop-evidence-btn"
                  data-testid="home-view-evidence"
                  onClick={() => onEvidence(c.entityId)}
                >
                  查看证据
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** 首页三区：卡片来自真实快照 `/api/desktop/v1/snapshot`（T-82）。
 *  加载失败显示明确不可用并允许重试，不静默回退静态卡（宪章原则 8）。 */
export function DesktopHome({
  onOpenTask,
  onOpenEvidence,
  onOpenDesExperiment,
  onOpenOpsChannel,
  onOpenMomWorkbench,
  opsEnabled,
  assistantName,
  s1BgJob = null,
  onS1Ask,
  onS1Delegate,
  onOpenBgTask,
  s1Reply = null,
}: Props) {
  const [cards, setCards] = useState<DesktopCard[] | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(() => {
    setFailed(false);
    api
      .desktopSnapshot()
      .then((r) => setCards(r.cards.map(toCard)))
      .catch(() => setFailed(true));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const open = (c: DesktopCard) => onOpenTask(c.entityId, "home-card");
  return (
    <div className="desktop-home" data-testid="desktop-home">
      <header className="desktop-home-header">
        <h1>我的 AI 桌面</h1>
        {assistantName && (
          <p className="desktop-assistant-name" data-testid="home-assistant-name">
            助理：{assistantName}
          </p>
        )}
        <p className="desktop-home-lead">提问、委派与关注同一处；模拟卡片已标注。</p>
        {!opsEnabled && (
          <p className="desktop-ops-off" data-testid="ops-module-off" role="status">
            运营模块未启用
          </p>
        )}
        <p className="desktop-settings-links">
          {onOpenMomWorkbench && (
            <a
              href="#mom-workbench"
              onClick={(e) => {
                e.preventDefault();
                onOpenMomWorkbench();
              }}
            >
              MOM 工作台
            </a>
          )}
          {onOpenDesExperiment && (
            <a
              href="#des-experiment"
              onClick={(e) => {
                e.preventDefault();
                onOpenDesExperiment();
              }}
            >
              DES 仿真实验
            </a>
          )}
          {onOpenOpsChannel && (
            <a
              href="#ops-channel"
              onClick={(e) => {
                e.preventDefault();
                onOpenOpsChannel();
              }}
            >
              运营频道
            </a>
          )}
          {(onOpenMomWorkbench || onOpenDesExperiment || onOpenOpsChannel) && (
            <span className="desktop-settings-hint">设置与管理</span>
          )}
        </p>
      </header>
      {onS1Ask && onS1Delegate && onOpenBgTask && (
        <>
          <S1AskBar bgJob={s1BgJob} onAsk={onS1Ask} onDelegate={onS1Delegate} onOpenBgTask={onOpenBgTask} />
          {s1Reply && (
            <p className="s1-reply" data-testid="s1-reply" role="status">
              {s1Reply}
            </p>
          )}
        </>
      )}
      {failed && (
        <p className="muted" data-testid="snapshot-unavailable" role="status">
          首页快照暂不可用。
          <button type="button" className="btn small" data-testid="snapshot-retry" onClick={load}>
            重试
          </button>
        </p>
      )}
      {!failed && cards === null && (
        <p className="muted" data-testid="snapshot-loading" role="status">
          首页加载中…
        </p>
      )}
      {cards !== null && (
        <div className="desktop-zones">
          <Zone title="我的工作" cards={cards.filter((c) => c.zone === "mine")} onOpen={open} onEvidence={onOpenEvidence} />
          <Zone title="Agent 工作" cards={cards.filter((c) => c.zone === "agent")} onOpen={open} onEvidence={onOpenEvidence} />
          <Zone title="为你关注" cards={cards.filter((c) => c.zone === "watch")} onOpen={open} onEvidence={onOpenEvidence} />
        </div>
      )}
    </div>
  );
}
