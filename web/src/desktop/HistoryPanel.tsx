import { useMemo, useState } from "react";
import {
  HISTORY_SEED,
  loadHistoryPrefs,
  saveHistoryPrefs,
  type HistoryItem,
  type HistoryPrefs,
} from "./evidenceMock";

type Props = {
  onOpenTask: (taskId: string) => void;
  onClose: () => void;
};

export function HistoryPanel({ onOpenTask, onClose }: Props) {
  const [prefs, setPrefs] = useState<HistoryPrefs>(() => loadHistoryPrefs());

  const update = (next: HistoryPrefs) => {
    setPrefs(next);
    saveHistoryPrefs(next);
  };

  const visible = useMemo(() => {
    const q = prefs.filter.trim().toLowerCase();
    let items = HISTORY_SEED.filter((h) => !prefs.hidden.includes(h.id));
    if (q) {
      items = items.filter(
        (h) =>
          h.title.toLowerCase().includes(q) ||
          h.objectLabel.toLowerCase().includes(q) ||
          (h.taskId ?? "").toLowerCase().includes(q),
      );
    }
    const rank = (h: HistoryItem) => {
      if (prefs.pinned.includes(h.id)) return 0;
      if (prefs.watched.includes(h.id)) return 1;
      return 2;
    };
    return [...items].sort((a, b) => rank(a) - rank(b) || b.updatedAt.localeCompare(a.updatedAt));
  }, [prefs]);

  const toggle = (field: "pinned" | "hidden" | "watched", id: string) => {
    const set = new Set(prefs[field]);
    if (set.has(id)) set.delete(id);
    else set.add(id);
    update({ ...prefs, [field]: [...set] });
  };

  return (
    <div className="desktop-panel history-panel" data-testid="history-panel" role="dialog" aria-label="历史">
      <header className="desktop-panel-head">
        <button type="button" className="btn small" onClick={onClose}>
          关闭历史
        </button>
        <h2>历史</h2>
      </header>
      <label className="history-filter">
        筛选
        <input
          data-testid="history-filter"
          aria-label="历史筛选"
          value={prefs.filter}
          onChange={(e) => update({ ...prefs, filter: e.target.value })}
          placeholder="标题 / 对象 / 任务 ID"
        />
      </label>
      <ul className="history-list" data-testid="history-list">
        {visible.map((h) => (
          <li key={h.id} className="history-row" data-testid={`history-item-${h.id}`}>
            <button
              type="button"
              className="history-open"
              onClick={() => h.taskId && onOpenTask(h.taskId)}
              disabled={!h.taskId}
            >
              <span className="history-title">{h.title}</span>
              <span className="history-meta">
                {h.objectLabel} · {h.updatedAt}
              </span>
              {h.reason && (
                <span className="history-reason" data-testid={`history-reason-${h.id}`}>
                  推荐：{h.reason}
                </span>
              )}
            </button>
            <div className="history-prefs">
              <button type="button" className="btn small" aria-pressed={prefs.pinned.includes(h.id)} onClick={() => toggle("pinned", h.id)}>
                固定
              </button>
              <button type="button" className="btn small" aria-pressed={prefs.watched.includes(h.id)} onClick={() => toggle("watched", h.id)}>
                关注
              </button>
              <button type="button" className="btn small" aria-pressed={prefs.hidden.includes(h.id)} onClick={() => toggle("hidden", h.id)}>
                隐藏
              </button>
            </div>
          </li>
        ))}
      </ul>
      {visible.length === 0 && (
        <p className="desktop-panel-empty" data-testid="history-empty">
          无匹配历史
        </p>
      )}
    </div>
  );
}
