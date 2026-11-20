import { DESKTOP_CARDS, SMT03_TASK_ID, type DesktopCard } from "./mockData";

type Props = {
  onOpenTask: (taskId: string, source: string) => void;
  onOpenEvidence: (taskId: string) => void;
  onOpenDesExperiment?: () => void;
  opsEnabled: boolean;
  cards?: DesktopCard[];
  assistantName?: string;
};

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

export function DesktopHome({
  onOpenTask,
  onOpenEvidence,
  onOpenDesExperiment,
  opsEnabled,
  cards = DESKTOP_CARDS,
  assistantName,
}: Props) {
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
        {onOpenDesExperiment && (
          <p className="desktop-settings-links">
            <a
              href="#des-experiment"
              onClick={(e) => {
                e.preventDefault();
                onOpenDesExperiment();
              }}
            >
              DES 仿真实验
            </a>
            <span className="desktop-settings-hint">设置与管理</span>
          </p>
        )}
      </header>
      <div className="desktop-zones">
        <Zone title="我的工作" cards={cards.filter((c) => c.zone === "mine")} onOpen={open} onEvidence={onOpenEvidence} />
        <Zone title="Agent 工作" cards={cards.filter((c) => c.zone === "agent")} onOpen={open} onEvidence={onOpenEvidence} />
        <Zone title="为你关注" cards={cards.filter((c) => c.zone === "watch")} onOpen={open} onEvidence={onOpenEvidence} />
      </div>
    </div>
  );
}
