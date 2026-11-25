# conversation-memory S6-exec：执行路径会话资料完整块（随任务持久）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-24（America/Vancouver） |
| 来源 | **用户确认**（2026-11-24 对话四项拍板：完整块 / 独立开关+512 / 仅 general / 随任务持久） |
| 状态 | **有效** |
| 影响范围 | 特性 conversation-memory（S6-exec）；`conversation.rs`（delegate 的任务上下文追加 + `session_materials_exec*`）、`config.rs`；RT-EXEC-01～04 |
| 相关设计 | [conversation-memory/README.md](../features/conversation-memory/README.md) §3；S5-route 决策（线索段先行）；宪章原则 5（状态由系统保存、每一步可追查） |

**一句话结论**：general 角色的委派任务，任务上下文（`tasks.context`）在 S5-route 线索段之后追加完整资料块（回答路径同款 render 输出，预裁 ≤512 token），随任务持久化——执行者拿到全文干活，回查任务可追查资料快照。

## 决策依据

1. 用户 2026-11-24 四项拍板（对话，推荐项全中）。
2. 代码事实：`delegate` 把 ctx 写进 `orchestrator.submit` 的 context 参数 → `tasks.context` 列 → General 步骤提示词「## 背景信息」段原样嵌入；S5-route 后线索行（40 字）已在，但执行需要全文。
3. 预算事实：General 的 system 含 ctx，超预算时 fit() 不裁 system——资料块必须自身预裁（512 上限）防膨胀。

## 定了什么

1. **形态**：`render()` 完整块（标记 + 摘要全文 + 来源行）追加到任务上下文（线索段之后）。
2. **角色**：仅 `execution_role = general`（路由后过滤；coding 代码任务/reviewer 不注入）。
3. **开关与预算**：`session_materials_exec` 默认开；`session_materials_exec_max_tokens` 默认 512（render 内部逐条均分截断）。
4. **持久化**：随 `tasks.context` 落库——重试/续跑/回查均可追查资料快照（保存时点语义，重试不刷新）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 每步动态注入 | 需要改 AgentAdapter trait 通道；task.context 是既有通道，零接口变更 |
| 仅线索段维持现状 | 40 字要点不够执行"整理那个任务的结论成报告"类任务 |
| 全角色注入 | coding 代码任务与 reviewer 审查场景价值存疑且多耗预算（用户拍板仅 general） |
| 不落库 | 重试/回查丢资料，违反可追查原则（用户拍板持久） |

## 出问题先看

- 「执行时没用到资料」：`session.materials` 事件与 `tasks.context` 列；执行角色是否 general；开关状态；资料段是否被 512 上限截断。
- context 列膨胀：512 上限预裁保证有界。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-24 | 新建 | S6-exec 立项 |
