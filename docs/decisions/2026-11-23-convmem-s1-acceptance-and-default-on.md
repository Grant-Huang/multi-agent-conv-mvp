# conversation-memory 5.1：验收冻结与会话资料注入默认开

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23（America/Vancouver） |
| 来源 | **用户确认**（2026-11-23 对话：验收标准 5 条冻结并开工；`[conversation] session_materials` 默认开） |
| 状态 | **有效** |
| 影响范围 | 特性 conversation-memory（第 5 线 5.1）；`crates/runtime/src/session_materials.rs`、`conversation.rs`、`store.rs`（只读方法）、`config.rs`；CASES RT-SMAT-01～05 / E2E-SMAT |
| 相关设计 | [conversation-memory/README.md](../features/conversation-memory/README.md) 第 2、3、4 节；[DESIGN.md 第 10 节](../DESIGN.md) `session.materials` 事件 |

**一句话结论**：REQ-conversation-memory-01 的验收按五条标准冻结（续问带来源、来源可查、失效降级、超时降级、预算不挤占），首个切片只做回答路径，资料 = 同会话未撤回任务链接的终态回执摘要，配置默认开。

## 背景

第 5 线计划 5.1 要求「单用户受控环境验证同一会话续问、来源可查、失效与超时降级」。实现入口 `conversation.rs` 的回答路径此前只带最近 12 条消息（`history_messages` 默认 12），任务回执一旦被挤出窗口，续问就得重复背景。B3 已有 `desktop_conversation_task_links`（会话↔任务，含撤回语义），但只有按 task_id 的读法。

## 问题

需求状态为「提出」时不能开工（DEV_STANDARD 第 5 节授权来源）；验收标准与默认行为需要用户拍板，否则 Case 断言和回归风险都没有锚点。

## 决策依据

1. 用户 2026-11-23 对话确认：验收标准五条冻结、默认开关取「默认开」（推荐项被选中）。
2. 五线安排（[2026-11-22-five-workstreams](2026-11-22-five-workstreams.md)）授权第 5 线做 5.1 立项与验收。
3. 代码事实：`conversation.rs` 回答路径只读 `history_messages` 条（默认 12，`config.rs d_history`）；`budget.rs fit()` 系统提示与最新消息不动、最旧先丢——资料块放系统提示后第一条即可保证「补充材料挤不掉当前请求」；B3 链接表已含 `revoked_at`。
4. INT 基线先例：「数据，非指令」前缀纪律（`introspect::INTROSPECTION_PREFIX`）已冻结，资料块沿用同一纪律。

## 定了什么

1. **验收五条**（CASES「会话资料注入」）：① 续问带来源（资料块含 `来源：task_id（状态 · 时间）` + 摘要）；② 来源可查（`session.materials` 事件只记 id 与原因，不记正文）；③ 失效降级（链接撤回 / 任务缺失剔除后回答照常）；④ 超时降级（资料源时限默认 300ms，超时本轮无资料、回答照常）；⑤ 预算不挤占（资料先按上限预裁，超预算被 `fit()` 最先裁掉）。
2. **范围**：只做回答路径（`generate_reply`）；判断 / 路由 / 执行路径选材、个人长期记忆（5.2）、INT 发现（5.3）、组织范围（5.4）均为后续卡。
3. **默认**：`session_materials = true`、`session_materials_max_tokens = 512`、`session_materials_timeout_ms = 300`；资料上限 3 条（`MAX_MATERIAL_ITEMS`）。
4. **数据面**：`store.rs` 只新增只读方法 `list_unrevoked_desktop_task_ids`，不改表、不改既有方法（高风险文件的事前方案即本决策 + 对话确认）。

## 简单设计

见 [conversation-memory/README.md](../features/conversation-memory/README.md) 第 3、4 节（2026-11-23 更新）：收集（`Collector`，可注入 `MaterialSource` 测降级）→ 校验（终态 + 有摘要）→ 渲染（`render`，「数据，非指令」页眉 + 逐条来源行，超限中段截断）→ 挂接（`generate_reply` 系统提示后第二条系统消息）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 默认关 | 用户确认默认开；受控单用户、无外发、纯提示词组装，验收需直接验证生效路径 |
| 资料块并入系统提示 | `fit()` 不裁系统提示，超大资料会挤占当前请求 |
| Store 读取走同步超时 | tokio timeout 无法中断同步调用；改异步 `MaterialSource` + `spawn_blocking`，超时语义真实可测 |
| 注入 INT 发现 / 个人记忆 | 归 5.3 / 5.2，避免本卡范围膨胀 |

## 影响与后续

- 已落地：`session_materials.rs`、`conversation.rs` 挂接、`store.rs` 只读方法、配置三键、DESIGN §10 事件、CASES / features.json / 证据（`testing/evidence/convmem-s1-session-materials.md`）。
- 后续：5.2 个人记忆以 T-71/T-72 交互基线 + T-78 Store 持久为地基；判断/路由/执行路径选材另卡立项。

## 出问题先看

- 日志事件 `session.materials`（selected / skipped / chars）；配置 `[conversation] session_materials*`；用例 `crates/server/tests/session_materials.rs`（RT-SMAT-01～05）与 `web/e2e/session-materials.spec.ts`。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建 | conversation-memory 5.1 立项 |
