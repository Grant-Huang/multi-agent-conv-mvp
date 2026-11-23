import { useEffect, useState } from "react";
import { api, type DesktopAssistantMessageDto } from "../api";

/** 助理消息已读跨端（T-73）：服务夹具权威；双端同未读。 */
export function AssistantReadPanel() {
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

  const reset = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.desktopAssistantMessagesReset();
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
        <strong>助理已读（服务）</strong>
        <span className="muted" data-testid="asst-read-unread">
          未读 {unread}
        </span>
        <button
          type="button"
          className="btn small"
          data-testid="asst-read-reset"
          disabled={busy}
          onClick={() => void reset()}
        >
          重置夹具
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="asst-read-refresh"
          disabled={busy}
          onClick={() => void refresh().catch((e) => setErr(String(e)))}
        >
          刷新
        </button>
      </div>
      <ul className="asst-read-list" data-testid="asst-read-list">
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
