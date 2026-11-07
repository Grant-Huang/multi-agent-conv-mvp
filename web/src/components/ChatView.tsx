// 对话区：消息列表 + 输入框（Enter 发送，Shift+Enter 换行）
import { memo, useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Message } from "../bindings/Message";
import type { TaskRecord } from "../bindings/TaskRecord";
import { taskStatusLine } from "../labels";
import { ArtifactList } from "./ArtifactList";
import { ConfirmButton } from "./ConfirmButton";
import { TaskTicker } from "./TaskTicker";

/** 对话里可以直接对任务做的操作（批准 / 取消），由 App 执行并负责刷新 */
export type TaskActionKind = "approve" | "cancel";

const EMPTY_LINES: string[] = [];

/** 空会话时的示例：点击后填入输入框，而不是只当作说明文字 */
const EXAMPLES = [
  "你好，介绍一下你能做什么",
  "帮我算一下 (1280 * 0.85 + 36) / 12",
  "写一份关于本地小模型选型的简短报告，保存成 Markdown",
  "在代码目录里写一个 Python 的斐波那契函数并附带测试",
];

/** 助手回复按 Markdown 渲染（react-markdown 默认不渲染原始 HTML） */
const MessageRow = memo(function MessageRow(props: {
  m: Message;
  /** 任务的最后一条消息：显示状态、操作和产物 */
  task: TaskRecord | null;
  /** 任务的第一条消息（简短回复）：走马灯挂在这里，最终正文在它之后 */
  tickerTask: TaskRecord | null;
  rank: number | null;
  lines: string[];
  busy: boolean;
  onAct?: (kind: TaskActionKind, taskId: string) => void;
}) {
  const { m, task, tickerTask, rank, lines, busy, onAct } = props;
  // 有进度或仍在运行时才显示走马灯；没有任务的消息不显示
  // 失败任务也显示走马灯：失败原因就是历史的最后一行
  const showTicker = !!tickerTask && (isRunning(tickerTask) || lines.length > 0 || tickerTask.status === "FAILED");
  return (
    <div className={`msg ${m.role}`}>
      <div className={m.role === "user" ? "bubble" : "bubble md"}>
        {m.role === "user" ? m.content : <MarkdownText text={m.content} />}
        {showTicker && tickerTask && (
          <TaskTicker lines={lines} running={isRunning(tickerTask)} failed={tickerTask.status === "FAILED"} />
        )}
        {task && (
          <div className="task-live">
            {/* 失败时状态已并入走马灯历史的最后一行，不再单独显示"失败"行 */}
            {task.status !== "FAILED" && (
              <div className={`task-status s-${task.status.toLowerCase()}`}>{taskStatusLine(task, rank)}</div>
            )}
            {/* 等待批准：直接在对话里给出操作，不需要去右侧面板找 */}
            {task.status === "NEEDS_HUMAN" && !task.result && onAct && (
              <div className="task-actions">
                <button type="button" className="btn primary small" disabled={busy} onClick={() => onAct("approve", task.id)}>
                  批准执行
                </button>
                <ConfirmButton
                  label="取消"
                  confirmLabel="确认取消"
                  disabled={busy}
                  onConfirm={() => onAct("cancel", task.id)}
                />
              </div>
            )}
            {isRunning(task) && onAct && (
              <div className="task-actions">
                <ConfirmButton
                  label="取消任务"
                  confirmLabel="确认取消"
                  disabled={busy}
                  onConfirm={() => onAct("cancel", task.id)}
                />
              </div>
            )}
            {task.result && <ArtifactList artifacts={task.result.artifacts} summary={task.result.summary} />}
          </div>
        )}
      </div>
    </div>
  );
}, (a, b) => a.m.id === b.m.id && a.m.content === b.m.content && a.m.role === b.m.role
  && a.task?.status === b.task?.status && a.rank === b.rank && a.lines === b.lines && a.busy === b.busy
  && a.onAct === b.onAct
  && a.tickerTask?.id === b.tickerTask?.id && a.tickerTask?.status === b.tickerTask?.status
  && artifactStamp(a.task) === artifactStamp(b.task));

/** 运行中（可取消）：待批准的任务不算运行中，它的取消按钮已由"待批准"分支提供，避免重复 */
function isRunning(task: TaskRecord): boolean {
  return !["COMPLETED", "FAILED", "CANCELLED", "NEEDS_HUMAN"].includes(task.status);
}

function artifactStamp(task: TaskRecord | null): string {
  if (!task?.result) return "";
  return `${task.result.summary.length}|${task.result.errors.length}|${task.result.artifacts.map((a) => `${a.kind}:${a.name}:${a.content.length}`).join("|")}`;
}

/**
 * 失败任务：历史末尾的"任务结束：FAILED"换成失败原因本身，原因并入历史，不再单独显示。
 * 没有终态行时直接追加；非失败任务原样返回。
 */
function withFailure(lines: string[], task: TaskRecord): string[] {
  const errors = task.result?.errors ?? [];
  if (task.status !== "FAILED" || errors.length === 0) return lines;
  const reason = errors.join("；");
  if (lines.at(-1)?.startsWith("任务结束")) return [...lines.slice(0, -1), reason];
  return [...lines, reason];
}

function queueRank(task: TaskRecord, tasks: TaskRecord[]): number | null {
  if (task.status !== "QUEUED") return null;
  const queued = tasks
    .filter((t) => t.role === task.role && t.status === "QUEUED")
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const i = queued.findIndex((t) => t.id === task.id);
  return i < 0 ? null : i + 1;
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

/** 等待回复时显示已等待的秒数；等待较久时提示可能在排队，避免用户以为卡死 */
function Thinking() {
  const [secs, setSecs] = useState(0);
  useEffect(() => {
    const started = Date.now();
    const t = setInterval(() => setSecs(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="bubble thinking" role="status">
      正在等待回复…（已等 {secs} 秒）
      {secs >= 20 && <div className="thinking-hint">模型可能正在处理其他请求，请稍候。</div>}
    </div>
  );
}

export function ChatView(props: {
  messages: Message[];
  onSend: (text: string) => Promise<void>;
  /** 正在流式生成的回答（最终消息到达后不再显示） */
  streaming: { id: string; text: string } | null;
  tasks?: TaskRecord[];
  /** 每个任务的进度历史（按时间顺序），用来做进度条和展开查看 */
  notes?: Record<string, string[]>;
  /** 批准 / 取消任务；由 App 调用接口并刷新 */
  onTaskAction?: (kind: TaskActionKind, taskId: string) => Promise<void>;
}) {
  const { messages, onSend } = props;
  const tasks = props.tasks ?? [];
  const notes = props.notes ?? {};
  const streaming = props.streaming && !messages.some((m) => m.id === props.streaming!.id) ? props.streaming : null;
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [busyTask, setBusyTask] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

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

  // 通过 ref 读取最新的回调，使 act 的引用保持稳定，memo 才能跳过无关行的重渲染
  const actionRef = useRef(props.onTaskAction);
  actionRef.current = props.onTaskAction;
  const act = useCallback(async (kind: TaskActionKind, taskId: string) => {
    if (!actionRef.current) return;
    setBusyTask(taskId);
    try {
      await actionRef.current(kind, taskId);
    } finally {
      setBusyTask(null);
    }
  }, []);

  // 同一个任务只在它的最后一条消息旁显示状态，避免重复
  const latest = new Map<string, string>();
  for (const m of messages) if (m.task_id) latest.set(m.task_id, m.id);
  // 走马灯挂在任务的第一条消息（简短回复）下面，最终正文在它之后
  const first = new Map<string, string>();
  for (const m of messages) if (m.task_id && !first.has(m.task_id)) first.set(m.task_id, m.id);

  return (
    <div className="chat">
      <div className="messages">
        {messages.length === 0 && (
          <div className="empty">
            <h2>试试这些</h2>
            <ul className="examples">
              {EXAMPLES.map((ex) => (
                <li key={ex}>
                  <button
                    type="button"
                    onClick={() => {
                      setText(ex);
                      inputRef.current?.focus();
                    }}
                  >
                    {ex}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {messages.map((m) => {
          const tid = m.task_id;
          const owner = tid ? tasks.find((t) => t.id === tid) ?? null : null;
          const task = owner && tid && latest.get(tid) === m.id ? owner : null;
          const tickerTask = owner && tid && first.get(tid) === m.id ? owner : null;
          return (
            <MessageRow
              key={m.id}
              m={m}
              task={task}
              tickerTask={tickerTask}
              rank={task ? queueRank(task, tasks) : null}
              lines={tickerTask && tid ? withFailure(notes[tid] ?? EMPTY_LINES, tickerTask) : EMPTY_LINES}
              busy={!!task && busyTask === task.id}
              onAct={props.onTaskAction ? act : undefined}
            />
          );
        })}
        {streaming && (
          <div className="msg assistant">
            <div className="bubble md">
              {/* 流式过程中用纯文本（保留换行），避免每个片段都重新解析 Markdown；结束后由消息列表完整渲染 */}
              <span className="stream-plain">{streaming.text}</span>
              <span className="cursor">▍</span>
            </div>
          </div>
        )}
        {sending && !streaming && (
          <div className="msg assistant">
            <Thinking />
          </div>
        )}
        <div ref={bottomRef} />
      </div>
      <div className="composer">
        <textarea
          ref={inputRef}
          value={text}
          aria-label="输入消息"
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
