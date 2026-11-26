import { useEffect, useState } from "react";
import { api, type DesktopAssistantMessageDto } from "../api";

/** UXFIX-H（INTENT-015）：委派任务终结通知（客户端生成，按任务去重，带“查看结果”） */
export type TaskNotice = { taskId: string; title: string; status: string };

type Props = {
  taskNotices?: TaskNotice[];
  onOpenNotice?: (taskId: string) => void;
};

/** 助理消息已读跨端（T-73）：服务夹具权威；双端同未读。 */
export function AssistantReadPanel({ taskNotices = [], onOpenNotice }: Props) {
  const [items, setItems] = useState<DesktopAssistantMessageDto[]>([]);
  const [unread, setUnread] = useState(0);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refresh = async () => {
    const r = await api.desktopAssistantMessagesList();
    setItems(r.items);
    setUnread(r.unread_count);
    setErr(null);
  };

  useEffect(() => {
    void refresh().catch((e) => setErr(String(e)));
    // 跨设备已读同步：轻量轮询（服务端权威）
    const t = setInterval(() => void refresh().catch(() => {}), 5_000);
    return () => clearInterval(t);
  }, []);

  const markRead = async (ids: string[]) => {
    setBusy(true);
    setErr(null);
    try {
      await api.desktopAssistantMessagesRead(ids);
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="asst-read-root" data-testid="asst-read-root">
      <div className="asst-read-head">
        <strong>助理消息</strong>
        <span className="muted" data-testid="asst-read-unread">
          未读 {unread}
        </span>
      </div>
      <ul className="asst-read-list" data-testid="asst-read-list">
        {taskNotices.map((n) => (
          <li key={`notice-${n.taskId}`} className="asst-read-item" data-testid="task-notice">
            <span className="muted">{n.status} · 普通优先级</span>
            <strong>{n.title}</strong>
            <span className="muted">任务 {n.taskId}</span>
            {onOpenNotice && (
              <button
                type="button"
                className="btn small"
                data-testid="task-notice-open"
                onClick={() => onOpenNotice(n.taskId)}
              >
                查看结果
              </button>
            )}
          </li>
        ))}
        {items.map((it) => (
          <li
            key={it.id}
            className={`asst-read-item${it.read ? " is-read" : ""}`}
            data-testid={`asst-read-item-${it.id}`}
            data-read={it.read ? "1" : "0"}
            data-msg-id={it.id}
          >
            <strong>{it.title}</strong>
            <span className="muted">{it.body}</span>
            <span className="muted" data-testid={`asst-read-flag-${it.id}`}>
              {it.read ? "已读" : "未读"}
            </span>
            {!it.read && (
              <button
                type="button"
                className="btn small"
                data-testid={`asst-read-mark-${it.id}`}
                disabled={busy}
                onClick={() => void markRead([it.id])}
              >
                标记已读
              </button>
            )}
          </li>
        ))}
      </ul>
      {err && (
        <p className="error" data-testid="asst-read-error">
          {err}
        </p>
      )}
    </div>
  );
}
