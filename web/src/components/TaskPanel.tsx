// 任务面板：任务列表，点击展开查看事件时间线、结果、产物、证据、审查意见。
import { useState } from "react";
import { api } from "../api";
import type { TaskEvent } from "../bindings/TaskEvent";
import type { TaskRecord } from "../bindings/TaskRecord";
import type { TaskStatus } from "../bindings/TaskStatus";
import { ROLE_LABEL, roleLabel, STATUS_LABEL } from "../labels";
import { useIncomingDetail } from "../refresh";
import { ApprovalInfo } from "./ApprovalInfo";
import { ArtifactList } from "./ArtifactList";
import { AssignmentLine } from "./AssignmentLine";
import { ConfirmButton } from "./ConfirmButton";
import type { TaskActionKind } from "./ChatView";

export function StatusBadge({ status }: { status: TaskStatus }) {
  return <span className={`badge s-${status.toLowerCase()}`}>{STATUS_LABEL[status]}</span>;
}

export function TaskPanel(props: {
  tasks: TaskRecord[];
  incoming: TaskEvent | null;
  onChanged: () => void;
  /** 批准 / 取消：由 App 执行，失败时 App 显示错误栏；未提供时不显示这两个按钮 */
  onTaskAction?: (kind: TaskActionKind, taskId: string) => Promise<void>;
  messageIds?: Record<string, string>;
}) {
  const { tasks, incoming, onChanged, onTaskAction, messageIds } = props;
  const [openId, setOpenId] = useState<string | null>(null);
  if (tasks.length === 0) return <div className="panel-empty">还没有任务。需要后台执行的请求会出现在这里。</div>;
  return (
    <div className="task-list">
      {tasks.map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          open={openId === t.id}
          onToggle={() => setOpenId(openId === t.id ? null : t.id)}
          incoming={incoming}
          onChanged={onChanged}
          onTaskAction={onTaskAction}
          messageId={messageIds?.[t.id] ?? null}
        />
      ))}
    </div>
  );
}

function TaskCard(props: {
  task: TaskRecord;
  open: boolean;
  onToggle: () => void;
  incoming: TaskEvent | null;
  onChanged: () => void;
  onTaskAction?: (kind: TaskActionKind, taskId: string) => Promise<void>;
  messageId: string | null;
}) {
  const { task, open, onToggle, incoming, onChanged, onTaskAction, messageId } = props;
  const detail = useIncomingDetail(open, task.id, incoming);
  const [busy, setBusy] = useState(false);

  const act = async (kind: TaskActionKind) => {
    if (!onTaskAction) return;
    setBusy(true);
    try {
      await onTaskAction(kind, task.id);
    } finally {
      setBusy(false);
      onChanged();
    }
  };

  const running = !["COMPLETED", "FAILED", "CANCELLED"].includes(task.status);
  const awaitingApproval = task.status === "NEEDS_HUMAN" && !task.result;
  const d = detail?.task ?? task;

  return (
    <div className={`task-card ${open ? "open" : ""}`}>
      {/* 标题是真正的按钮：可以用键盘展开，并告知屏幕阅读器展开状态 */}
      <button type="button" className="task-head" aria-expanded={open} onClick={onToggle}>
        <StatusBadge status={task.status} />
        <span className="role">
          {roleLabel(task.role)}
          {/* 执行者：标题行就能看到是哪个 agent 在做（尚未分配时只显示角色） */}
          {task.assignment && <> · {task.assignment.agent_name}</>}
        </span>
        <span className="goal" title={task.goal}>{task.goal}</span>
      </button>

      {open && (
        <div className="task-body">
          <div className="actions">
            {onTaskAction && awaitingApproval && (
              <button type="button" className="btn primary small" disabled={busy} onClick={() => act("approve")}>批准执行</button>
            )}
            {onTaskAction && running && (
              <ConfirmButton label="取消" confirmLabel="确认取消" disabled={busy} onConfirm={() => act("cancel")} />
            )}
            {messageId && <MisrouteForm messageId={messageId} role={task.role} busy={busy} />}
          </div>

          {awaitingApproval && (
            <Section title="批准前请确认">
              <ApprovalInfo task={d} />
            </Section>
          )}

          {d.assignment && (
            <Section title="执行者">
              <AssignmentLine a={d.assignment} />
            </Section>
          )}

          <Section title="路由">
            <div className="kv">
              类别 {d.route.task_class} · 风险 {d.route.risk_level} · 工具 {d.route.required_tools.join(", ") || "无"}
            </div>
            <div className="kv">理由：{d.route.reason}</div>
            <div className="kv">
              Policy：{d.policy.auto_execute ? "自动执行" : "需审批"} · {d.policy.needs_review ? "需要审查" : "不审查"}
              {d.policy.notes.length > 0 && ` · ${d.policy.notes.join("；")}`}
            </div>
          </Section>

          {detail && detail.events.length > 0 && (
            <Section title="事件">
              <ol className="timeline">
                {/* 交付消息已显示在对话区，这里不重复 */}
                {detail.events.filter((e) => e.kind !== "assistant_message").map((e) => (
                  <li key={e.id} className={e.kind}>
                    <time>{e.created_at.slice(11, 19)}</time>
                    {e.status && <StatusBadge status={e.status} />}
                    <span>{e.message}</span>
                  </li>
                ))}
              </ol>
            </Section>
          )}

          {d.result && (
            <Section title={`执行结果（${d.result.status === "succeeded" ? "进程成功" : "进程失败"}）`}>
              <pre>{d.result.summary}</pre>
              <ArtifactList artifacts={d.result.artifacts} summary={d.result.summary} />
              {d.result.evidence.length > 0 && (
                <details>
                  <summary>证据（{d.result.evidence.length} 条）</summary>
                  {d.result.evidence.map((e, i) => (
                    <pre key={i}>[{e.kind}] {e.content}</pre>
                  ))}
                </details>
              )}
              {d.result.errors.length > 0 && <pre className="bad-text">{d.result.errors.join("\n")}</pre>}
            </Section>
          )}

          {d.review && (
            <Section title={`审查：${d.review.verdict}`}>
              <div className="kv">{d.review.comment}</div>
              {d.review.issues.length > 0 && (
                <ul className="issues">{d.review.issues.map((i, k) => <li key={k}>{i}</li>)}</ul>
              )}
            </Section>
          )}
        </div>
      )}
    </div>
  );
}

const ACTIONS = [
  ["respond", "直接回答"],
  ["clarify", "先追问"],
  ["delegate", "交给后台"],
  ["follow_up", "改进行中的任务"],
] as const;

/** 「派错了」反馈表单。错误在表单内就地显示，不弹出 alert */
function MisrouteForm({ messageId, role, busy }: { messageId: string; role: string; busy: boolean }) {
  const [open, setOpen] = useState(false);
  const [action, setAction] = useState("respond");
  const [correctRole, setCorrectRole] = useState(role);
  const [note, setNote] = useState("");
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  if (done) return <span className="kv">已记下</span>;
  if (!open) {
    return <button type="button" className="btn small" disabled={busy} onClick={() => setOpen(true)}>派错了</button>;
  }
  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.feedback({
        message_id: messageId,
        original_action: "delegate",
        original_role: role,
        correct_action: action,
        correct_role: action === "delegate" ? correctRole : "",
        note,
      });
      setDone(true);
    } catch (e) {
      setError(`保存失败：${e}`);
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="misroute">
      <label>
        正确动作
        <select value={action} onChange={(e) => setAction(e.target.value)}>
          {ACTIONS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>
      </label>
      {action === "delegate" && (
        <label>
          正确角色
          <select value={correctRole} onChange={(e) => setCorrectRole(e.target.value)}>
            {(Object.keys(ROLE_LABEL) as (keyof typeof ROLE_LABEL)[]).map((r) => (
              <option key={r} value={r}>{ROLE_LABEL[r]}（{r}）</option>
            ))}
          </select>
        </label>
      )}
      <input value={note} placeholder="一句说明" onChange={(e) => setNote(e.target.value)} />
      <button type="button" className="btn small" disabled={busy || saving} onClick={save}>保存</button>
      {error && <div className="bad-text" role="alert">{error}</div>}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="section">
      <h4>{title}</h4>
      {children}
    </section>
  );
}
