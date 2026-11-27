/** UXFIX-Q（#342，INTENT-2026-029，CH-U5）：运营阶段与约束的用户语言文案。
 *  服务端卡片摘要（ops_desk.rs card_for）已是中文；此模块供前端卡片与工作台
 *  统一阶段显示（避免英文状态码），批准后阶段与摘要一致为“已批准，待执行”。 */
const STAGE_LABEL: Record<string, string> = {
  WAITING_HUMAN: "待批准",
  BLOCKED: "受阻",
  VERIFYING: "核验中",
  CLOSED: "已关闭",
  CANCELLED: "已取消",
  BUDGET_EXHAUSTED: "预算耗尽",
  INVESTIGATING: "调查中",
  ASSESS: "评估中",
  COLLECTING: "收集中",
};

export function opsStageText(stage: string, approver?: string | null): string {
  if (stage === "WAITING_HUMAN" && approver) return "已批准，待执行";
  return STAGE_LABEL[stage] ?? stage;
}

/** 已知约束的中文对照（原文可折叠展示，未知条目原样显示） */
const CONSTRAINT_LABEL: Record<string, string> = {
  "Never bypass safety interlock or quality release": "不得绕过安全联锁或质量放行",
  "Approval must match current plan and resource conditions": "批准须与当前方案和资源条件一致",
  "Mock evidence is not real industrial validation": "模拟证据不构成真实工业验证",
};

export function constraintText(c: string): string {
  return CONSTRAINT_LABEL[c] ?? c;
}
