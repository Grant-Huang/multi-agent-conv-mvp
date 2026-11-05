// 对话区：消息列表 + 输入框（Enter 发送，Shift+Enter 换行）
import { memo, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Message } from "../bindings/Message";
import type { TaskRecord } from "../bindings/TaskRecord";
import { ArtifactList } from "./ArtifactList";

/** 助手回复按 Markdown 渲染（react-markdown 默认不渲染原始 HTML） */
const MessageRow = memo(function MessageRow({ m, task, rank, note }: { m: Message; task: TaskRecord | null; rank: number | null; note: string }) {
  return (
    <div className={`msg ${m.role}`}>
      <div className={m.role === "user" ? "bubble" : "bubble md"}>
        {m.role === "user" ? m.content : <MarkdownText text={m.content} />}
        {task && (
          <div className="task-live">
            <div className={`task-status s-${task.status.toLowerCase()}`}>{statusLine(task, rank)}</div>
            {note && (
              <div className="marquee" title={note}>
                <span>{note}</span>
              </div>
            )}
            {task.result && <ArtifactList artifacts={task.result.artifacts} summary={task.result.summary} />}
          </div>
        )}
      </div>
    </div>
  );
}, (a, b) => a.m.id === b.m.id && a.m.content === b.m.content && a.m.role === b.m.role
  && a.task?.status === b.task?.status && a.rank === b.rank && a.note === b.note
  && artifactStamp(a.task) === artifactStamp(b.task));

function artifactStamp(task: TaskRecord | null): string {
  if (!task?.result) return "";
  return `${task.result.summary.length}|${task.result.artifacts.map((a) => `${a.kind}:${a.name}:${a.content.length}`).join("|")}`;
}

function queueRank(task: TaskRecord, tasks: TaskRecord[]): number | null {
  if (task.status !== "QUEUED") return null;
  const queued = tasks
    .filter((t) => t.role === task.role && t.status === "QUEUED")
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const i = queued.findIndex((t) => t.id === task.id);
  return i < 0 ? null : i + 1;
}

function statusLine(task: TaskRecord, rank: number | null): string {
  switch (task.status) {
    case "QUEUED":
      return rank ? `排队中（第 ${rank} 位）` : "排队中";
    case "RUNNING":
      return `${task.role} 执行中`;
    case "VERIFYING":
      return "审查中";
    case "COMPLETED":
      return "执行成功";
    case "FAILED":
      return "执行失败";
    case "NEEDS_HUMAN":
      return "待确认";
    case "CANCELLED":
      return "已取消";
    default:
      return "已接收";
  }
}

function MarkdownText(props: { text: string }) {
  return (
    <Markdown
      remarkPlugins={[remarkGfm]}
      components={{
        a: ({ node: _node, ...rest }) => <a {...rest} target="_blank" rel="noopener noreferrer" />,
      }}
    >
      {props.text}
    </Markdown>
  );
}

export function ChatView(props: {
  messages: Message[];
  onSend: (text: string) => Promise<void>;
  /** 正在流式生成的回答（最终消息到达后不再显示） */
  streaming: { id: string; text: string } | null;
  tasks?: TaskRecord[];
  /** 每个任务最新一条事件原文，用来做走马灯 */
  notes?: Record<string, string>;
}) {
  const { messages, onSend } = props;
  const tasks = props.tasks ?? [];
  const notes = props.notes ?? {};
  const streaming = props.streaming && !messages.some((m) => m.id === props.streaming!.id) ? props.streaming : null;
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  // 有新消息时滚动到底部
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, sending, streaming?.text.length]);

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
        {(() => {
          const latest = new Map<string, string>();
          for (const m of messages) if (m.task_id) latest.set(m.task_id, m.id);
          return messages.map((m) => {
            const task = m.task_id && latest.get(m.task_id) === m.id ? tasks.find((t) => t.id === m.task_id) ?? null : null;
            return (
              <MessageRow
                key={m.id}
                m={m}
                task={task}
                rank={task ? queueRank(task, tasks) : null}
                note={task && m.task_id ? notes[m.task_id] ?? "" : ""}
              />
            );
          });
        })()}
        {streaming && (
          <div className="msg assistant">
            <div className="bubble md">
              {/* 流式过程中用纯文本，避免每个片段都重新解析 Markdown；结束后由消息列表完整渲染 */}
              <span className="stream-plain">{streaming.text}</span>
              <span className="cursor">▍</span>
            </div>
          </div>
        )}
        {sending && !streaming && (
          <div className="msg assistant">
            <div className="bubble thinking">思考中…</div>
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
