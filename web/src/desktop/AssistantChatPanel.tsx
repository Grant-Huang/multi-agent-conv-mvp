import { useEffect, useState } from "react";

type Props = {
  name: string;
  avatar: string;
  /** 发送对话（App 传入与 S1 相同的真实问答链路） */
  onAsk: (text: string) => void;
  /** 最近一轮回答（App 状态透传） */
  reply?: string | null;
  /** 「调整助理」→ 设置中心对应分类 */
  onOpenSettings: () => void;
  onClose: () => void;
};

/** UXFIX-G（INTENT-012）：「我的助理」打开可直接对话的个人助理面板——
 *  顶部对话输入与最近消息；「调整助理」进设置中心；关闭恢复下层界面与草稿（CH-U1、CH-U5）。 */
export function AssistantChatPanel({ name, avatar, onAsk, reply, onOpenSettings, onClose }: Props) {
  const [text, setText] = useState("");
  const [sent, setSent] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const send = () => {
    const t = text.trim();
    if (!t) return;
    setSent(t);
    onAsk(t);
    setText("");
  };

  return (
    <div className="assistant-panel-root">
      <div className="assistant-panel-backdrop" onClick={onClose} aria-hidden="true" />
      <section className="assistant-panel" data-testid="assistant-panel" role="region" aria-label="我的助理">
        <header className="assistant-panel-head">
          <span className="assistant-panel-avatar">
            {avatar} {name}
          </span>
          <button type="button" className="btn small" data-testid="assistant-panel-settings" onClick={onOpenSettings}>
            调整助理
          </button>
          <button type="button" className="btn small" data-testid="assistant-panel-close" onClick={onClose}>
            关闭
          </button>
        </header>
        <div className="assistant-panel-msgs" data-testid="assistant-panel-msgs">
          <p className="muted">最近消息</p>
          {sent && (
            <p className="assistant-panel-line">
              我：{sent}
            </p>
          )}
          {reply && <p className="assistant-panel-line">助理：{reply}</p>}
          {!sent && !reply && <p className="muted">有什么想让我做的，直接说。</p>}
        </div>
        <div className="assistant-panel-inputrow">
          <input
            data-testid="assistant-panel-input"
            aria-label="与助理对话"
            placeholder="对助理说点什么…（Enter 发送）"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                send();
              }
            }}
          />
          <button type="button" className="btn small primary" data-testid="assistant-panel-send" onClick={send}>
            发送
          </button>
        </div>
      </section>
    </div>
  );
}
