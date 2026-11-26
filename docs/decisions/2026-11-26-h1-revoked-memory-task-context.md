# 个人记忆撤回后，已保存的任务 context 必须一并失效（按宪章 CH-G2e）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 来源 | 用户确认（本会话明确拍板「是，按宪章 CH-G2e」，2026-11-26） |
| 状态 | 有效 |
| 影响范围 | conversation-memory（S6 执行资料）、runtime 任务 context、宪章 CH-G1/CH-G2e |
| 相关设计 | `docs/PRODUCT.md` 宪章 v1.6 CH-G2e；`docs/decisions/2026-11-24-convmem-s6-exec-materials.md` 第 4 条（待修订）；评审 `docs/review/2026-11-26.md` R-20261126-H1 |

## 一句话结论

个人记忆撤回的效力及于已保存在任务 context 中的副本：任务只保存记忆来源 id，执行、重试、重启续跑前按撤回状态重新过滤；S6 决策第 4 条「保存时点语义，重试不刷新」相应修订。

## 背景

S6（会话资料进执行）合入时（`docs/decisions/2026-11-24-convmem-s6-exec-materials.md`）定了「保存时点语义，重试不刷新」：资料在提交时渲染进 `tasks.context`，重试/续跑复用，不随源刷新。夜间评审 2026-11-26（PR #356）以 P1 复现了它的盲区：撤回个人记忆后批准待审任务，执行者 system「背景信息」仍含已撤回正文——渲染点 `crates/runtime/src/conversation.rs:584-593`、拼装点 `:831-845`，经 `orchestrator.submit` 落 `tasks.context`，批准、重试、WP7 重启续跑都复用这份 context。

## 问题

宪章 CH-G2e（约束级，用户 2026-11-25 确认）要求「本人可查、可改、可撤……撤回后不再使用」；S6 第 4 条没有处理撤回，与宪章冲突（权威顺序以宪章为准）。不解决，用户撤回的个人信息继续被使用并长期留存在 `tasks.context`，违反数据主体控制承诺。

## 决策依据

1. 用户确认（2026-11-26，四选一问题中明确选「是，按宪章 CH-G2e」）。
2. 评审复现证据：`docs/review/2026-11-26.md` R-20261126-H1（已复现，脚手架复用 `crates/server/tests/exec_materials.rs`，假模型 route_decision 返回 high）。
3. 宪章权威顺序：`AGENTS.md` 开始前第 1 条以 `docs/PRODUCT.md` 为产品约束来源；CH-G2e 为约束级。

## 定了什么

1. 撤回效力及于已存任务 context。
2. 实现路径二选一（由修复方案定，触 `orchestrator.rs` 续跑路径须先出方案经用户批准）：任务 context 不保存记忆正文只保存来源 id，执行/重试/续跑前按撤回状态过滤再渲染；或撤回时清理未终态任务 context 中的对应段。
3. S6 决策第 4 条按本决策修订并互链。
4. 派工卡 [#358](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/358)（第 5 线，severity:high）随之解锁。

## 没选什么

维持 S6 保存时点语义、不追溯已存 context——需要修订宪章 CH-G2e 或登记约束级例外，用户未选。

## 影响与后续

- #358 按「验收标准」（撤回后批准/重试/续跑，General 请求不含该记忆正文和 id）修复；第 5 线出方案。
- AR-13 起本线复验；`R-20261125-M1` 待批方案（#357）相关口径同步对齐本决策。
- 与 #362（M1 collect 开关）同文件，注意错峰。

## 出问题先看

`crates/server/tests/exec_materials.rs`；`tasks.context` 实际内容；撤回事件与渲染过滤相关日志（event=session.materials 家族）。

## 变更记录

- 2026-11-26 新建。
