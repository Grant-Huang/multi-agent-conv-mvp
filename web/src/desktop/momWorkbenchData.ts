/** M0-4 MOM 三入口工作台种子数据（模拟；与 world_mock case/world 共用标识约定）。 */

export type MomWorld = {
  world_id: string;
  label: string;
  provenance: "mock" | "des" | "production";
};

export type MomCase = {
  case_id: string;
  world_id: string;
  title: string;
  stage: string;
  owner: string;
  impact: string;
  next_step: string;
  /** 任务视图分组 */
  task_group: "mine" | "running" | "blocked" | "done";
  /** 资源缺口标签（人机料法环测） */
  resource_gaps: string[];
  summary: string;
};

export const MOM_WORLDS: MomWorld[] = [
  { world_id: "world-demo-east", label: "华东精加工 · 模拟", provenance: "mock" },
  { world_id: "world-des-toolwear", label: "刀具磨损演练 · DES", provenance: "des" },
  {
    world_id: "world-prod-east",
    label: "华东精加工 · 生产",
    provenance: "production",
  },
];

export const MOM_CASES: MomCase[] = [
  {
    case_id: "case-insert-a",
    world_id: "world-demo-east",
    title: "插单 A · CNC-XX 振动",
    stage: "WAITING_HUMAN",
    owner: "调度 · 张明",
    impact: "交期 / 质量",
    next_step: "批准改排与维护方案",
    task_group: "mine",
    resource_gaps: ["机", "料"],
    summary: "B 线振动联锁；备用机缺刀具，首件未检。",
  },
  {
    case_id: "case-c101",
    world_id: "world-demo-east",
    title: "客诉 C101",
    stage: "INVESTIGATING",
    owner: "质量 · 李华",
    impact: "客户承诺",
    next_step: "补充 8D 证据",
    task_group: "running",
    resource_gaps: ["测"],
    summary: "外径超差投诉；4h 回复窗口未关闭。",
  },
  {
    case_id: "case-pack-1",
    world_id: "world-demo-east",
    title: "包装配送缺口",
    stage: "BLOCKED",
    owner: "仓储",
    impact: "发运",
    next_step: "等待纸箱到货交接",
    task_group: "blocked",
    resource_gaps: ["料", "人"],
    summary: "纸箱批次延迟；任务已受理，实物未交接。",
  },
  {
    case_id: "case-des-tool",
    world_id: "world-des-toolwear",
    title: "DES 刀具磨损演练事项",
    stage: "WAITING_HUMAN",
    owner: "调度",
    impact: "仿真",
    next_step: "在运营频道批准",
    task_group: "mine",
    resource_gaps: ["机"],
    summary: "与正式运行/运营频道共用 case_id / world_id 约定。",
  },
  {
    case_id: "case-prod-east-1",
    world_id: "world-prod-east",
    title: "华东生产 · CNC-1 状态观测",
    stage: "OBSERVING",
    owner: "调度",
    impact: "生产只读",
    next_step: "只读观测水位；写走隔离 Mock/DES",
    task_group: "mine",
    resource_gaps: [],
    summary: "生产 stub 观测；provenance=production；写入口关闭。",
  },
];

export function casesForWorld(worldId: string): MomCase[] {
  return MOM_CASES.filter((c) => c.world_id === worldId);
}

export function findCase(caseId: string): MomCase | undefined {
  return MOM_CASES.find((c) => c.case_id === caseId);
}
