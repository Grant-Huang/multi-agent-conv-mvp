// 待批准任务的说明：批准之前告诉用户，这个任务会交给谁、预计用哪些工具、为什么需要批准、完成后是否审查。
// 对话区和任务面板共用。
import type { TaskRecord } from "../bindings/TaskRecord";
import { roleLabel } from "../labels";

export function ApprovalInfo({ task }: { task: Pick<TaskRecord, "role" | "route" | "policy"> }) {
  const candidates = task.policy.candidates ?? [];
  const tools = task.route.required_tools;
  return (
    <dl className="approval-info">
      <dt>将交给</dt>
      <dd>
        {candidates.length === 0 ? (
          <>{roleLabel(task.role)}角色的第一个可用 agent</>
        ) : (
          candidates.map((c, i) => (
            <span key={c} className="candidate">
              {c}
              {candidates.length > 1 && <span className="muted">{i === 0 ? "（主力）" : "（备份）"}</span>}
            </span>
          ))
        )}
      </dd>
      <dt>预计工具</dt>
      <dd>
        {tools.length > 0 ? tools.map((t) => <code key={t}>{t}</code>) : "无"}
        {/* required_tools 只是 Router 的预估，写进提示词，不限制 agent 实际可用的工具 */}
        <span className="muted"> Router 预估，执行时可能用到该 agent 的其他工具</span>
      </dd>
      <dt>为什么要批准</dt>
      <dd>{task.policy.notes.length > 0 ? task.policy.notes.join("；") : "Policy 未说明"}</dd>
      <dt>完成后</dt>
      <dd>{task.policy.needs_review ? "交给 Reviewer 审查" : "不审查"}</dd>
    </dl>
  );
}
