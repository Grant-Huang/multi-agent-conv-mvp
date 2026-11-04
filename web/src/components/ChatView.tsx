// 对话区：消息列表 + 输入框（Enter 发送，Shift+Enter 换行）
import { useEffect, useRef, useState } from "react";
import type { Message } from "../bindings/Message";

export function ChatView({ messages, onSend }: { messages: Message[]; onSend: (text: string) => Promise<void> }) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  // 有新消息时滚动到底部
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, sending]);

  const submit = async () => {
    const t = text.trim();
    if (!t || sending) return;
    setText("");
    setSending(true);
    try {
      await onSend(t);
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="chat">
      <div className="messages">
        {messages.length === 0 && (
          <div className="empty">
            <h2>试试这些</h2>
            <ul>
              <li>你好，介绍一下你能做什么</li>
              <li>帮我算一下 (1280 * 0.85 + 36) / 12</li>
              <li>写一份关于本地小模型选型的简短报告，保存成 Markdown</li>
              <li>在代码目录里写一个 Python 的斐波那契函数并附带测试</li>
            </ul>
          </div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`msg ${m.role}`}>
            <div className="bubble">
              {m.content}
              {m.task_id && <div className="msg-task">任务 {m.task_id.slice(0, 13)}</div>}
            </div>
          </div>
        ))}
        {sending && (
          <div className="msg assistant">
            <div className="bubble thinking">思考中…（本地小模型可能需要几秒到几十秒）</div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>
      <div className="composer">
        <textarea
          value={text}
          placeholder="输入消息，Enter 发送，Shift+Enter 换行"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            // 输入法组字过程中按 Enter 不发送
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          rows={3}
        />
        <button className="btn primary" onClick={submit} disabled={sending || !text.trim()}>
          发送
        </button>
      </div>
    </div>
  );
}
