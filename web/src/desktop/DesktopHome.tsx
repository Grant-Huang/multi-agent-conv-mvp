import { DESKTOP_CARDS, type DesktopCard } from "./mockData";

type Props = {
  onOpenTask: (taskId: string, source: string) => void;
  opsEnabled: boolean;
};

function Zone({
  title,
  cards,
  onOpen,
}: {
  title: string;
  cards: DesktopCard[];
  onOpen: (c: DesktopCard) => void;
}) {
  return (
    <section className="desktop-zone" aria-label={title}>
      <h2 className="desktop-zone-title">{title}</h2>
      <ul className="desktop-card-list">
        {cards.map((c) => (
          <li key={c.id}>
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
          </li>
        ))}
      </ul>
    </section>
  );
}

export function DesktopHome({ onOpenTask, opsEnabled }: Props) {
  const open = (c: DesktopCard) => onOpenTask(c.entityId, "home-card");
  return (
    <div className="desktop-home" data-testid="desktop-home">
      <header className="desktop-home-header">
        <h1>我的 AI 桌面</h1>
        <p className="desktop-home-lead">提问、委派与关注同一处；模拟卡片已标注。</p>
        {!opsEnabled && (
          <p className="desktop-ops-off" data-testid="ops-module-off" role="status">
            运营模块未启用
          </p>
        )}
      </header>
      <div className="desktop-zones">
        <Zone title="我的工作" cards={DESKTOP_CARDS.filter((c) => c.zone === "mine")} onOpen={open} />
        <Zone title="Agent 工作" cards={DESKTOP_CARDS.filter((c) => c.zone === "agent")} onOpen={open} />
        <Zone title="为你关注" cards={DESKTOP_CARDS.filter((c) => c.zone === "watch")} onOpen={open} />
      </div>
    </div>
  );
}
