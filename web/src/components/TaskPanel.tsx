// 任务面板：任务列表，点击展开查看事件时间线、结果、产物、证据、审查意见。
import { useState } from "react";
import { api } from "../api";
import type { TaskEvent } from "../bindings/TaskEvent";
import type { TaskRecord } from "../bindings/TaskRecord";
import type { TaskStatus } from "../bindings/TaskStatus";
import { useIncomingDetail } from "../refresh";
import { ArtifactList } from "./ArtifactList";

const STATUS_LABEL: Record<TaskStatus, string> = {
  CREATED: "已创建",
  QUEUED: "排队中",
  RUNNING: "执行中",
  VERIFYING: "审查中",
  COMPLETED: "已完成",
  FAILED: "失败",
  NEEDS_HUMAN: "待人工",
  CANCELLED: "已取消",
};

export function StatusBadge({ status }: { status: TaskStatus }) {
  return <span className={`badge s-${status.toLowerCase()}`}>{STATUS_LABEL[status]}</span>;
}

export function TaskPanel({ tasks, incoming, onChanged, messageIds }: { tasks: TaskRecord[]; incoming: TaskEvent | null; onChanged: () => void; messageIds?: Record<string, string> }) {
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
          messageId={messageIds?.[t.id] ?? null}
        />
      ))}
    </div>
  );
}

function TaskCard(props: { task: TaskRecord; open: boolean; onToggle: () => void; incoming: TaskEvent | null; onChanged: () => void; messageId: string | null }) {
  const { task, open, onToggle, incoming, onChanged, messageId } = props;
  const detail = useIncomingDetail(open, task.id, incoming);
  const [busy, setBusy] = useState(false);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      alert(String(e));
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
      <div className="task-head" onClick={onToggle}>
        <StatusBadge status={task.status} />
        <span className="role">{task.role}</span>
        <span className="goal" title={task.goal}>{task.goal}</span>
      </div>

      {open && (
        <div className="task-body">
          <div className="actions">
            {awaitingApproval && (
              <button className="btn primary small" disabled={busy} onClick={() => act(() => api.approve(task.id))}>批准执行</button>
            )}
            {running && (
              <button className="btn small" disabled={busy} onClick={() => act(() => api.cancel(task.id))}>取消</button>
            )}
            {messageId && <MisrouteForm messageId={messageId} role={task.role} busy={busy} />}
          </div>

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

function MisrouteForm({ messageId, role, busy }: { messageId: string; role: string; busy: boolean }) {
  const [open, setOpen] = useState(false);
  const [action, setAction] = useState("respond");
  const [correctRole, setCorrectRole] = useState(role);
  const [note, setNote] = useState("");
  const [done, setDone] = useState(false);
  if (done) return <span className="kv">已记下</span>;
  if (!open) {
    return <button className="btn small" disabled={busy} onClick={() => setOpen(true)}>派错了</button>;
  }
  const save = async () => {
    await api.feedback({
      message_id: messageId,
      original_action: "delegate",
      original_role: role,
      correct_action: action,
      correct_role: action === "delegate" ? correctRole : "",
      note,
    });
    setDone(true);
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
            <option value="general">general</option>
            <option value="coding">coding</option>
            <option value="reviewer">reviewer</option>
          </select>
        </label>
      )}
      <input value={note} placeholder="一句说明" onChange={(e) => setNote(e.target.value)} />
      <button className="btn small" disabled={busy} onClick={() => save().catch((e) => alert(String(e)))}>保存</button>
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
