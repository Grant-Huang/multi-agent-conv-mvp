// 执行者：由哪个 agent 执行、在哪台机器、是执行组的主力还是备份、排队了多久、第几轮。
// 任务卡标题行用简短形式（只显示名称），展开后的“执行者”节用完整形式。
import type { Assignment } from "../bindings/Assignment";
import { memberLabel, waitedLabel } from "../labels";

/** 完整一行：General Agent · 本机 · 主力（无备份）· 排队 1.2s · 第 1 轮返工 */
export function AssignmentLine({ a }: { a: Assignment }) {
  const parts = [
    a.agent_name,
    a.kind === "node" ? a.location || "节点" : `${a.location}（${a.kind}）`,
    memberLabel(a.member_index, a.group_size),
    waitedLabel(a.waited_ms),
    a.round > 0 ? `第 ${a.round} 轮返工` : "",
  ].filter(Boolean);
  return (
    <div className="kv assignment" title={a.agent_id}>
      {parts.join(" · ")}
    </div>
  );
}
