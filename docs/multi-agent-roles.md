# 多 Agent 分工与协作：C-Agent / Decision Harness / O-Agent / S-Agent 权威协作设计

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 状态 | **设计中（方案稿，待用户批准）**——批准后本行改为「有效」并按分期开工 |
| 来源 | 派工 [Issue #397](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/397)（用户 2026-11-26 确认的[职责补全决定](decisions/2026-11-26-workstream-realignment.md)） |
| 上位文档 | [INTEGRATION_PLAN.md](INTEGRATION_PLAN.md) 五线表（各线职责唯一权威描述，本文不复述）；[DESIGN.md](DESIGN.md) 第 0、5～9 节（分层与权威设计）；[ai-desktop/DESIGN.md](ai-desktop/DESIGN.md) 第 6 节（异步执行、证据与受控动作） |
| 负责线 | 第 4 线（调度使用、路由效果、结果整合）；Agent/连接器接入与资料注入见五线表（第 5 线） |

**一句话**：把 ai-desktop/DESIGN 第 6 节遗留的「后续职责」空位补全——四个 Agent 角色各管什么、交接时数据长什么样、失败怎么回退、结果按什么规则整合；已实现部分引用现有权威设计，只把**缺口和分期**写成可开工的条目。

## 1. 角色与现有部件（只对应，不复述职责）

| 角色 | 对应部件 | 权威设计 | 负责线 |
|---|---|---|---|
| C-Agent（会话前台） | `runtime/src/conversation.rs`、回合控制器（TurnDecision 分派、会话锁、风险补齐） | [DESIGN.md](DESIGN.md) 第 5 节；本文第 3.1、4 节 | 4 |
| Decision Harness（意图/关联/路由） | `router.rs`、`policy.rs`、`prompts.rs` | [DESIGN.md](DESIGN.md) 第 9 节；本文第 3.2 节 | 4（资料注入由第 5 线提供，路由效果由第 4 线负责，以路由评测集共同验收） |
| O-Agent（编排与执行组） | `orchestrator.rs`、`registry.rs`、`crates/agents`、`crates/node` | [DESIGN.md](DESIGN.md) 第 7 节；本文第 3.3、4 节 | 4 |
| S-Agent（结果整合） | `composer.rs`、`report.rs`、任务结果与核验呈现 | [DESIGN.md](DESIGN.md) 第 8 节 + **本文第 5 节（新增权威规则）** | 4 |

接入侧（Agent 发现、配置、准入、握手、连接器标准）与资料注入（S4/S5 会话选材）归第 5 线，见五线表与 [workstream-realignment](decisions/2026-11-26-workstream-realignment.md) 第 7 项：「接进来归 5 线，怎么用归 4 线」。

## 2. 交接数据契约（现状已实现，此处固定为协作契约）

跨角色传递的数据结构**已经存在并被测试覆盖**；本节把它们钉为角色间契约——任何一侧改动都要过对端评审（第 4/5 线交叉文件见五线表接口约定）。

### 2.1 外壳 → C-Agent
- 用户消息：`Message { role, content }`（会话历史持久化于 `messages` 表）。
- 当前作用域与入口信号：桌面端经 `x-desktop-principal` 夹具头（无头回退 `local`，5.2a 契约）；由第 1 线提供（场合信号接口约定）。

### 2.2 C-Agent → Decision Harness
- 回合判断输入：历史窗口 + 判断清单摘要（第 5 线 S4/S5 注入，**只进路由请求、不进任务上下文**——ROUTE_DIGEST_HEADER 线索段与 route_ctx 剥离，#360/#435）。
- 输出：`TurnDecision`（Respond / Delegate / Clarify / ManageAgents / FollowUp）——结构化 JSON，模型输出格式错误按 `LLM_FORMAT` 重试后报错（AG.2：无可用执行者不换派，前台大白话说明）。

### 2.3 Decision Harness → O-Agent
- 路由决策：`RouteDecision { task_class, execution_role, required_tools, risk_level, reason, template_id, deliverable, slots_ok }`。
- Policy 裁决：`PolicyVerdict { auto_execute, needs_review, notes, candidates }`（确定性规则，不调模型）。
- 提交：`Orchestrator::submit(conversation_id, goal, context, route, policy, exec_material)`——`context` 只装路由纯净摘录；会话资料/个人记忆走独立的 `exec_material`（tasks.exec_material 列，#360），执行时仅本机执行者注入，外部 A2A 不下发。

### 2.4 O-Agent → 执行者（内置 / ACP / A2A / 节点）
- 任务规格：`TaskSpec { task_id, conversation_id, goal, context, route }`（an-core 公开类型；TS 绑定随之再生）。
- 租约：`registry.acquire_for(role, risk)` → `Assignment`（agent_id/kind/location/member_index/round），先记执行者再转 RUNNING。
- 外发边界：本机工具与 LLM 走 EgressGuard（enforce 拒发 / audit 记录）；外部 A2A 出站 goal/context 分字段过守卫（#360 防线二），资料块不下发并记 `task.exec_material_withheld`。

### 2.5 执行者 → O-Agent → S-Agent → C-Agent
- 结果：`TaskResult { status, summary, artifacts, evidence, errors }`；外部 agent 结果标注「未经核实」（RT-ADM-15）。
- 质检：`ReviewVerdict`（返工轮次上下文追加审查问题，`set_task_context`）。
- 事件：DESIGN 第 10 节事件族（`task.*`、`agent.run_start/run_end`、`conversation.*`、`store.*`），全部关联 task_id/conv_id。
- 交付：Composer 组装交付消息（LLM 不可达退回模板）；回复落 `messages` 表 + 任务卡片（AG.4）。

## 3. 失败与回退矩阵（现有语义，固定为契约）

| 失败点 | 现行为 | 契约依据 |
|---|---|---|
| 回合判断格式错误 | `LLM_FORMAT` 重试后前台致歉，不换派 | AG.2、prompts.rs |
| 路由无可执行 agent | `ROUTE_NO_AGENT` → 前台大白话说明 + 替代办法；管理类意图给引导话术（两道安全网） | AG.2、conversation.rs |
| 执行者获取失败 | `task.no_agent` 事件，任务带错误码失败 | orchestrator |
| 执行超时 | deadline 到点 `TASK_TIMEOUT` 失败；取消经进程组 SIGTERM→SIGKILL | proc_group、orchestrator |
| 同会话并发 | ConvLock 串行（先到先得），无竞争走 try_lock 快路径（`waited` 布尔判据，#359） | conv_lock.rs |
| 模板并行分支 | 子任务独立跑；汇合 `insert_join_if_absent` 原子去重（#301；exec_material 插列后参数位 ?14，#407 回归教训） | template_orch |
| 质检返工 | `ReviewVerdict` 触发重试轮，审查问题追加进 context；名额在审查前归还 | orchestrator run_with_review |
| 高风险/参数不足 | Policy 判 `needs_human` → NeedsHuman 待审批；批准按 A1～A3（绑定操作者/参数版本/有效期，幂等一次） | identity-and-approval 契约 |
| 外发守卫拒绝 | 工具/LLM/A2A：enforce 拒发（`E_EGRESS_BLOCKED/REVIEW/UNAVAILABLE`），audit 记录放行 | egress.rs、#360 |
| 外部 agent 失败 | 结果映射 Failed + 错误透传；HTTP 错误/坏 JSON/超时分别带错误码 | a2a.rs（RT-ADM-15） |
| 进程重启 | 任务状态与上下文在 tasks 表（服务重启后可批准续跑）；**进程内断点续跑依赖持久队列，未承诺**（ai-desktop DESIGN §6 声明） | orchestrator、ai-desktop §6 |

## 4. 结果整合规则（S-Agent 权威规则，新增）

首版已实现：结果 = `TaskRecord.result`（summary/artifacts/evidence）+ 核验标注 + 任务事件呈现；Composer 出交付消息。在此之上固定四条规则：

1. **来源标注**：外部 agent（A2A/节点）的产出一律带「未经核实」证据标注；内置执行者带工具与命令证据（已有）。
2. **事实/推断/待核实分层**：报告类产物区分三段（对齐 ai-desktop §6 产物面板）；资料块与内省发现带「待核实」状态的，回答若省略风险提示由程序补齐（CH-G2c，#402）——S-Agent 呈现层不得剥掉该提示。
3. **单任务权威**：一个任务的历史 Run 保持可查，迟到事件不覆盖当前主 Run 状态（ai-desktop §6 已定）；S-Agent 不做跨 Run 合并，只呈现当前权威 Run。
4. **多任务聚合（缺口 → P1）**：跨任务汇总（如一周报告聚合多个 Run 的产物）首版未实现；以「聚合只读投影 + 逐条来源链接」实现（见第 5 节 P1），不做产物改写。

## 5. 缺口与分期（可开工条目）

| 期 | 条目 | 验收锚点 | 依赖 |
|---|---|---|---|
| 已实现 | 上表 2、3 节全部契约 + 4 节规则 1～3 | 既有 RT 用例族（RT-ADM-15、RT-EXECMAT、RT-A2AEGR、RT-G2C、RT-RMAT、rt_tpl_*、RT-AGCAT、RT-ADAPPR 等） | — |
| P1 | Decision Harness 路由评测基线：`evals/routing*.jsonl` 固化 ≤200 条代表性回合（respond/delegate/follow_up/管理意图），FT 实验以通过率为验收 | PLAN「FT」；第 4/5 线共同验收 | 路由评测集格式（evals/ 已有 harness 雏形） |
| P1 | S-Agent 整合规则 4（跨任务聚合只读投影） | 新 RT 用例（聚合投影含逐条来源链接） | 任务事件查询 API |
| P2 | 服务端身份认证与权限实现（A7 + B1～B4 落地）：见配套[实施方案](multi-agent-roles-identity-plan.md) | 契约第 6 节验收表 + 新增 A7 用例 | 身份方案批准 |
| P3 | 进程重启断点续跑（持久队列 + 检查点） | ai-desktop §6 Run 状态机验收 | 持久队列基建（跨线立项） |

## 6. 与横切的边界（只列接口，职责见五线表）

- **准入（5 线）→ 调度（4 线）**：准入记录（AdmissionRecord/激活）是 registry 注册的唯一来源；调度侧只按 kind/role/risk 选用。
- **资料（5 线）→ 路由效果（4 线）**：S4/S5 注入物以「判断线索 + 资料块」两路交付；路由效果以路由评测集共同验收（workstream-realignment 接口约定）。
- **主动消息候选（4/5 线）→ 出口（1 线）**：任务事件（完成/失败/待处理）以候选交第 1 线提醒管道，不直接推用户。
- **身份（4 线实现）→ 各消费方**：A7 过滤在服务端各读模型与执行入口落地；夹具身份（alice/bob/supervisor）标明「测试身份」（B4），迁移方案见配套实施方案。

## 7. 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-27 | 方案稿新建（#397 首交付物，待批准） | #397、workstream-realignment |
