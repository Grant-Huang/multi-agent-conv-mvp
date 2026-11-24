# trace_id 先做最小版：受理时生成、写路径日志必带；持久化推迟到 M4 写入前

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-24 |
| 来源 | 用户确认（2026-11-24 对话：对齐审查报告第 4 项“按你的推荐做”，即方案 A 最小版） |
| 状态 | 有效（契约修订已生效；实现见下方实现卡 TRACE-1，未开工） |
| 影响范围 | [contracts/events-and-trace.md](../contracts/events-and-trace.md) R1；`crates/server`（受理层、World Adapter 写路径、桌面审批）；DESIGN 第 10 节事件清单 |
| 相关设计 | [contracts/events-and-trace.md](../contracts/events-and-trace.md) 第 3 节 R1、R2 与第 6 节验收；[DESIGN.md](../DESIGN.md) 第 10.2、10.3 节；[2026-11-22-c2-m2-minimal-contracts](2026-11-22-c2-m2-minimal-contracts.md) D5 |

**一句话结论**：契约 R1 分两步落地——先在 HTTP 受理时生成 `trace_id` 并让 C2/M2 写路径的**日志**必带（TRACE-1）；把 `trace_id` 写进事件表、随事件流下发，推迟到 World Adapter M4 生产写入开工之前。

## 背景

`trace_id` 是一次用户请求从“桌面或 HTTP 受理”开始、经过任务、案例、提案、执行一路携带的关联键（标识权威见契约第 3 节表格：受理层拥有）。2026-11-22 的决定 D5 把它定为契约 R1：“C2/M2 及之后的新写路径日志与事件必须带 `trace_id`”。C2 指 AI 桌面的动作审批，M2 指 World Adapter 接模拟工厂的写路径（提案 → 批准 → 执行 → 确认）。

## 问题

R1 已经生效两天，但代码里没有一处实现，导致契约与代码不一致：之后每个触及写路径的 PR 按规则都“违反契约”，评审只能逐个豁免，契约失去约束力。另一方面，T-87 已提出 NexusOps 真实厂写方案，真实写入一旦开始，出了问题却没有贯穿的关联键，就无法从桌面一路查到工厂回执。

## 决策依据

1. **代码事实：0 处实现**。2026-11-24 在 `33b2d05` 上执行 `grep -rn "trace_id\|traceId" crates web/src`，结果为 0 行；契约第 6 节的 RT-TRACE-01 标“待补”。
2. **代码事实：受理层没有追踪中间件**。`crates/server/src` 唯一的请求中间件是 `host_guard.rs` 第 65 行的 Host 校验（`middleware::from_fn`）；没有 `TraceLayer` 或请求 ID 处理。
3. **代码事实：写路径成功时没有结构化日志**。`mom_world_des.rs` 的 `propose`（第 1462 行）、`execute`（第 1539 行）、`confirm`（第 1943 行）和 `desktop_api.rs` 的 `decide_approval`（第 1588 行）、`execute_approval`（第 1645 行）只在失败时返回错误码；`mom_world_des.rs` 里的 `tracing::warn!` 只有两处，都是 `task.status`。也就是说，现在“写路径日志”本身就不存在，`trace_id` 无处可带。
4. **代码事实：持久化会碰高风险文件**。把 `trace_id` 写进事件或执行记录，需要改表结构与 `crates/runtime/src/store.rs`，属于 AGENTS.md“先停下来问人”的文件，须另出方案。
5. **时间线**：World Adapter M4 生产写入的准入门槛已冻结（[2026-11-23-wa-m4-write-entry-gates](2026-11-23-wa-m4-write-entry-gates.md)，保持 `WRITE_DISABLED`）；真实写入之前，模拟工厂与桩数据路径的排查用日志已经够用（推断）。
6. 用户 2026-11-24 确认按方案 A 最小版推进。

## 定了什么

- **契约 R1 修订**为两段：
  - R1a（本次生效）：`trace_id` 在 HTTP 受理时确定——请求头 `X-Trace-Id` 合法则沿用，否则服务端生成；放进本次请求的 tracing 上下文；C2/M2 写路径的**结构化日志**必带 `trace_id`，并同时带上已有的业务标识（`case_id`、`proposal_id`、`execution_id`，有任务时带 `task_id`）。响应头回写 `X-Trace-Id`，方便调用方和测试关联。
  - R1b（推迟）：`trace_id` 写入事件表与执行记录、随事件流下发，在 World Adapter M4 生产写入开工前完成，须另出改表方案并获批准。
- **旧路径**（C2/M2 之前的写路径）不设截止期，触及时顺带补齐；回答契约第 5 节待决定第 1 条。
- **实现**按下方实现卡 TRACE-1 另开 PR，由 World Adapter 线或桌面线认领；编号以主线 #1 发号为准。

## 简单设计

```
浏览器 / 调用方 ──HTTP──▶ 受理层中间件（新）
                           │ 读 X-Trace-Id（合法：1～64 位 [A-Za-z0-9_-]）否则生成 UUID v4
                           │ tracing span 字段 trace_id；响应头回写 X-Trace-Id
                           ▼
        ┌──────────── 写路径处理函数（现有） ────────────┐
        │ WA：propose / execute / confirm               │
        │ 桌面：decide_approval / execute_approval      │
        └─ 成功与拒绝各打一条结构化日志（新）──────────┘
             event=world.propose|world.execute|world.confirm|desktop.approval.decide|desktop.approval.execute
             trace_id（来自 span）+ case_id / proposal_id / execution_id / task_id（有则带）
```

- 不改 HTTP 请求体与响应体字段、不改 `config.toml`、不改 `AgentAdapter`、不改表结构；只新增一个可选请求头和响应头（属于接口的增量扩展，须在 PR 中说明）。
- 新事件名登记到 DESIGN 第 10.3 节（契约 E1）。
- 日志只记标识，不记正文和凭证（契约 E5）。

### 实现卡 TRACE-1（待认领）

| 项 | 内容 |
|---|---|
| 目标 | 契约 R1a 在代码中成立 |
| 范围 | `crates/server/src/`：新增受理层中间件（可与 `host_guard.rs` 并列）；`mom_world_des.rs`、`mom_world_mock.rs`、`mom_world_prod.rs` 的 propose/execute/confirm；`desktop_api.rs` 的 decide_approval/execute_approval；`docs/DESIGN.md` 第 10.3 节 |
| 不做 | 改表、改 `store.rs`、事件流字段、前端生成 `trace_id`（前端可后续在请求头带上） |
| 验收（先写 Red） | **RT-TRACE-01**：对模拟工厂走一次 propose → execute → confirm，带 `X-Trace-Id: t-rt-01`；断言三次响应头均回写 `t-rt-01`，捕获的日志中三条 `world.*` 事件都含 `trace_id=t-rt-01`，且 execute 事件同时含 `proposal_id` 与 `execution_id`。**RT-TRACE-02**：不带请求头时，响应头有服务端生成的 `X-Trace-Id`，且与同次请求日志中的 `trace_id` 相同。**RT-TRACE-03**：非法请求头（超长或含非法字符）被丢弃并重新生成，不写进日志 |
| 风险等级 | 预计 L1～L2（不碰高风险文件；以 `risk_classify.py` 为准） |
| 超时与清理 | 中间件同步执行，不引入新的异步任务或子进程 |

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 一次做全（日志 + 事件表 + 事件流） | 要改 `store.rs` 和表结构，属高风险，须另批；模拟与桩数据阶段收益小 |
| 方案 B：只改契约，把 R1 整体推迟到 M4 | 追踪能力继续完全缺席；而且写路径连成功日志都没有，越晚补越难 |
| 由前端生成 `trace_id` | 只覆盖桌面入口，HTTP 直接调用与测试没有；受理层生成能覆盖全部入口，前端以后可在请求头里带上 |
| 复用 World Adapter 的 `request_id` | `request_id` 由调用方提供、可选，且只在 WA 内有效，不能代表“从桌面受理开始”的一次请求 |

## 影响与后续

- 已同步：契约 `events-and-trace.md` R1 拆为 R1a/R1b、第 5 节待决定第 1 条、第 6 节验收；`docs/fencing/GAP_ANALYSIS.md` G18。
- 待做：实现卡 TRACE-1（认领方先补 RT-TRACE-01～03 到 `docs/testing/` 与 `features.json`，Red→Green）。
- 待做：R1b 改表方案，在 World Adapter M4 生产写入开工前提交。
- 实现落地前，评审对 C2/M2 写路径 PR 不再以“缺 `trace_id`”判为违反契约，改为指向 TRACE-1。

## 出问题先看

- 按 `trace_id` 搜日志查不到写入记录：先看该处理函数是否已按 TRACE-1 打 `world.*` / `desktop.approval.*` 事件；再看请求是否经过受理层中间件（路由是否挂在中间件之后）。
- 调用方传入的 `X-Trace-Id` 没生效：检查是否被合法性规则丢弃（RT-TRACE-03）。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-24 | 新建；修订契约 R1 为 R1a/R1b | 补充 [2026-11-22-c2-m2-minimal-contracts](2026-11-22-c2-m2-minimal-contracts.md) D5 |
