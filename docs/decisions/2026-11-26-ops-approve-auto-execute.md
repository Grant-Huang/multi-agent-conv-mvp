# 运营案例批准即执行：频道卡批准后引擎按批准自动执行，受阻保留批准、过期提示重新批准

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 来源 | 用户确认（Issue #340 派工明确授权第 3 线「先定设计……二选一并写进 OPS_DESK.md」；「批准即执行」这一选型为助手建议，随 PR 评审确认） |
| 状态 | 有效 |
| 影响范围 | 特性 ops-desk（运营频道批准链路）；`crates/runtime/src/mom/engine.rs`、`model.rs`、`ops_desk.rs`；`crates/server/src/mom_ops.rs`；`web/src/components/OpsCards.tsx`；`docs/mom/OPS_DESK.md` |
| 相关设计 | [OPS_DESK.md](../mom/OPS_DESK.md) 第 1、4 节；[PRODUCT.md](../PRODUCT.md) 原则 6、7；[UI_UX_PRINCIPLES.md](../mom/UI_UX_PRINCIPLES.md) §2（一个主操作）；决定 56、57、60 |

**一句话结论**：运营频道卡上的「批准」= 人确认 + 立即执行——引擎在同一事务里登记批准并马上按批准执行；执行前提不满足时保留批准、发 `ExecutionBlocked` 事件、卡片显示「已批准，待执行（受阻：原因）」，到期前再点批准即重试；批准到期仍未执行则发 `ApprovalExpired` 事件，卡片明确提示「批准已过期，需重新批准」，再点批准按新批准重来。不设单独的「执行」按钮。

## 背景

MOM 引擎里「批准」（`approve`：登记批准人、方案版本、有效期）和「执行」（`execute`：按批准派发动作、产生回执）是两个独立命令。运营频道卡片只有批准按钮；批准之后没有任何入口触发执行——引擎记录了批准，案例停在 `WAITING_HUMAN`、动作为空，批准在 `expires_at`（s1 固定 5 分钟）后静默失效。产品意图审查 UX-07（2026-11-26 基线）把这一缺陷立为 INTENT-027（高），派工 Issue #340 要求二选一：批准后引擎自动执行，或卡片上加「执行」按钮（二次确认）。

## 问题

不解决的话：主管在频道批准后案例永远不进入执行，卡片与引擎「已批准」的事实和「什么都没发生」的表现互相矛盾；批准闲置过期无人知晓；现有 E2E 只能绕过界面直接调 API 执行（`seedClosed`），掩盖了入口缺失。

## 决策依据

1. 宪章原则 7：「改配置、写业务这类有后果的动作，由固定代码执行，并要人确认」（`docs/PRODUCT.md` 原则表第 7 行）。批准本身就是那条「人确认」；执行由引擎固定代码完成，自动执行不需要第二次确认——第二次点击出自同一个人、同一方案版本、同一范围，不增加安全性。
2. 宪章原则 6：「只有源系统受理并核验后，写动作才能标为完成」。执行之后案例进 `VERIFYING`（核验中），核验仍独立存在，自动执行不越权（`crates/runtime/src/mom/engine.rs` `execute()`：动作落 `VERIFYING`，`close()` 要求全部 `VERIFIED`）。
3. 运营桌面原则 §2：事项行「只有**一个主操作**」（`docs/mom/UI_UX_PRINCIPLES.md`）。加「执行」按钮会让一个事项出现批准、执行两个串行主操作，违反原则；批准即执行保持主操作只有「批准」一个。
4. 缺陷本体：Issue #340 报告的正是「批准后无入口、批准闲置过期」。手动「执行」按钮不解决批准闲置——仍要人记得回来点第二次。批准即执行让正常路径上的批准不再闲置；过期提示只出现在执行受阻的分支。
5. 先例：World Adapter DES 投影卡已经走「批准即执行」合一步接口 `/api/world/des/ops/approve-execute`（`crates/server/src/mom_world_des.rs` `ops_approve_execute`；`web/src/components/OpsCards.tsx` M2-5 注释），两套卡片行为一致比两套语义更好维护。
6. 代码事实：`engine.approve()` 只登记不执行（`crates/runtime/src/mom/engine.rs`）；`mom_ops.rs` 的 `approve` handler 批准后只刷新卡片；发布器只在有新事件时更新卡片——所以过期提示必须由引擎在时钟推进时发事件驱动（`tick()`），卡片无法用墙钟对比引擎模拟时间（引擎 `now` 是仿真分钟）。
7. 引擎既有契约测试 `changed_resource_and_expired_approval_prevent_dispatch`（`crates/runtime/tests/mom.rs`）断言「批准后世界变了，再调 execute 必须被拒」。因此不改 `approve()`/`execute()` 原语语义，新增 `approve_and_execute()` 供运营频道批准接口使用；脚本 Master、an-mom、既有测试种子路径行为不变。

## 定了什么

1. **批准即执行**：`POST /api/mom/desk/cases/{id}/approve`（频道卡批准按钮）改走引擎新方法 `approve_and_execute(id, plan_revision, scope, actor)`——一个事务：先按现行 `approve` 全部校验登记批准（失败则整体失败、什么都不留，与今天一致），成功后立刻按批准执行。
2. **执行受阻不回滚批准**：执行侧前提不满足（资源被其他案例占用、资源/认证/工具不可用、工具预算不足）时，批准保留，发 `ExecutionBlocked { scope, reason }` 事件，案例记录 `execution_block`（范围、原因、时刻；纯提示性字段，`#[serde(default)]` 兼容旧快照）；接口返回 `{already:false, executed:false, blocked_reason}`。到期前主管再点「批准」= 用同一批准重试执行（不刷新有效期）。
3. **批准过期显式作废提示**：引擎 `tick()` 里发现「批准存在、未被执行消费、`now > expires_at`」时发一次 `ApprovalExpired { scope }` 事件；批准记录保留（审计）。卡片摘要显示「批准已过期，需重新批准」；再点「批准」= 重新登记批准并立即执行。证据快照 TTL 与批准 TTL 相同（s1 均 5 分钟），过期后证据多半已陈旧：先走现行补救路径 `refresh_plan`（MOM 工作台）再批准，卡上暂无「刷新方案」入口（归第 1 线后续 UX 卡）。卡上按钮在 `WAITING_HUMAN` 且有范围/方案版本时始终可见，标签按状态为 批准 / 重试执行 / 重新批准。
4. **卡片状态全部由引擎状态推导**（`ops_desk.rs` `card_for`，新增 `now` 参数）：已批准未执行+未过期 →「已批准，待执行」；有过 `execution_block` → 附受阻原因；过期未执行 →「批准已过期，需重新批准」；执行成功 → 既有「核验中」与回执路径。`ChatCard::OpsCase` 结构不变（不改公共契约、不重生成 bindings）。
5. **执行成功路径与既有语义完全一致**：`ActionAccepted` 事件、动作回执、`VERIFYING`/`INVESTIGATING` 阶段、DES 案例的 `des_after_execute` 后处理照旧；成功时清除 `execution_block`。
6. **范围**：只改运营频道批准链路。引擎 `approve`/`execute` 原语、an-mom `/api/mom/cases/{id}/commands`、脚本 Master 流程不变。

## 简单设计

见 [OPS_DESK.md](../mom/OPS_DESK.md) 第 4 节「批准与执行」小节（本决定随附更新）：

```
频道卡[批准] → POST /approve → engine.approve_and_execute（一个事务）
  ├─ 批准校验失败 → HTTP 409/404（同今天，什么都不留）
  ├─ 执行成功 → ActionAccepted → 卡片「核验中 + 回执」（发布器照常就地更新）
  └─ 执行受阻 → 批准保留 + ExecutionBlocked 事件 + execution_block 字段
                → 卡片「已批准，待执行（受阻：原因）」；到期前点批准=重试
时钟 tick 越过 expires_at 且批准未被消费 → ApprovalExpired 事件
                → 卡片「批准已过期，需重新批准」；点批准=重新批准+执行
```

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 卡片加「执行」按钮（二次确认） | 违反原则 §2「一个主操作」；不解决批准闲置过期（Issue #340 的缺陷本体）；比 WA 卡的 approve-execute 多一套语义 |
| 直接改 `engine.approve()` 内部自动执行 | 破坏既有引擎契约测试（`changed_resource_and_expired_approval_prevent_dispatch` 等）与 an-mom 脚本流程；改动面大且改了与运营频道无关的语义 |
| 前端用墙钟判断过期 | 引擎时间是仿真分钟（`expires_at≈519`），与墙钟不可比；过期必须由引擎时钟推进时发事件驱动 |
| 过期时删除批准记录 | 卡片摘要无法再区分「从未批准」与「批准已过期」；审计链断（决定 60 批准记录） |
| 卡片加 `expires_at`/审批状态字段 | 要改 `an_core::ChatCard::OpsCase` 公共契约并重生成 TS bindings；摘要文本已够表达，前端已有从摘要取字段的先例 |

## 影响与后续

- 实现：`engine.rs`（`approve_and_execute`、`tick` 过期事件、执行核心抽函数）、`model.rs`（`execution_block`）、`ops_desk.rs`（`card_for` 带 now）、`mom_ops.rs`（handler 换方法+响应加字段）、`OpsCards.tsx`（按钮可见性与标签）。
- 测试：新增 E2E-OPSAUTOEXEC、E2E-OPSEXPIRE（Red→Green）；RT-OPSDESK-12/13（引擎层：受阻保留批准、过期事件）；既有用例断言不变。
- 回执展示的字段级呈现归第 1 线 #341（UXFIX-P）；本决定只保证卡片显示真实执行状态。
- 批准人身份仍是本机测试身份 `supervisor`（决定 60，企业认证未实现）。

## 出问题先看

- 批准后没执行：看 `mom_events` 里该案例有没有 `ActionAccepted` / `ExecutionBlocked`；`GET /api/mom/cases/{id}` 的 `decision_inbox`、`actions`。
- 卡片不更新：发布器游标（`*.ops.db` `ops_meta.cursor`）与 `ops_skips` 没发原因。
- 过期没提示：引擎时钟是否推进（`advance` 命令/播放中）；`ApprovalExpired` 事件是否发出。
- 开关：`[mom] chat_bridge=false` 时整条链路不启用，行为与从前一致。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-26 | 新建 | Issue #340（INTENT-027） |
