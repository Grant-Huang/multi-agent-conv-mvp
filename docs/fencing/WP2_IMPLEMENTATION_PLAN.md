# WP2 实施方按：审查「需人工」验收出口（GS3）

| 项目 | 内容 |
|---|---|
| 任务 | T-65（方案卡）；实现另卡 |
| 日期 | 2026-11-22 |
| 状态 | **待用户批准后另卡实现**；本文件与决策 PR **只含文档** |
| 缺口 | G04；黄金场景 GS3 |
| 契约 | [state-and-cancellation](../contracts/state-and-cancellation.md) **T1**、**S7**；宪章原则 4（执行者不当自己的裁判） |
| 既有摘要 | [REVIEW_VERDICT_DESIGN.md](REVIEW_VERDICT_DESIGN.md)（API/用例已冻结进 CASES） |
| 唯一实施负责人（获批后） | 本主线 worker（跨模块语义 + 编排）；实现卡不得并行改 `orchestrator.rs`/`store.rs` |
| 核对基线 | `origin/master` `afb69d6` |

**一句话**：审查进入「待验收」后，人可判定通过或退回返工；与执行前「待批准」在语义与文案上分开；首版用现有 `NEEDS_HUMAN` + `result` 有无作为等待原因，不新增枚举状态。

---

## 1. 问题与目标

| 现状 | 目标（GS3） |
|---|---|
| Reviewer `needs_human` / 审查失败 → `NEEDS_HUMAN` **且带 result**；UI 仍显示「待批准」；无通过/退回按钮；`approve` 只接受无 result 任务 | 用户能验收通过 → `COMPLETED`；退回 → 有额度则返工，否则 `FAILED`；文案为「待验收」 |
| 与「高风险待批准执行」（`NEEDS_HUMAN` 且 **无** result）共用状态字面量 | 同一 `TaskStatus` 下用显式等待原因区分（契约 T1 允许「等价的显式等待原因」） |

**非目标（本 WP2）**：不改自动质检 pass/fail 路径；不引入多用户权限 / 真实登录；不实施 WP4 模板并行；不改 MOM/WA 写路径；不解决 G05（Reviewer 读文件能力）。

---

## 2. 推荐方案（★）

### 2.1 状态模型（满足 T1，最小改动）

| 等待原因 | 判定条件（权威） | 用户文案 | 允许动作 |
|---|---|---|---|
| `awaiting_approval` | `status=NEEDS_HUMAN` 且 `result_json IS NULL` | 待批准 | `POST .../approve` → `QUEUED`；取消 |
| `awaiting_acceptance` | `status=NEEDS_HUMAN` 且 `result_json IS NOT NULL` | 待验收 | `POST .../review-verdict`；取消 |

- **不**新增 `TaskStatus` 变体（避免 an-core / bindings / 桌面映射大范围改动；桌面已把 `NEEDS_HUMAN` 映为「需人工处理」，可由等待原因细化）。
- 编排进入审查需人工时：继续 `finish` 为 `NEEDS_HUMAN` + 写入 `result` + `review`（与现码一致）。
- 可选增强（非本卡必做）：`tasks.wait_reason` 列冗余落库，便于日志过滤；若做，迁移见 §4。首版**可不加列**，以 `result` 有无为权威判定（与现有 `approve_task` SQL 一致）。

### 2.2 API

与 [REVIEW_VERDICT_DESIGN.md](REVIEW_VERDICT_DESIGN.md) 一致：

```http
POST /api/tasks/{id}/review-verdict
Content-Type: application/json

{"verdict":"accept"|"reject","note":"..."}
```

| verdict | 行为 |
|---|---|
| `accept` | 条件更新：`NEEDS_HUMAN` + result 非空 → `COMPLETED`；写任务事件（人工验收 note）；触发 Composer 按完成交付（与自动通过同路径） |
| `reject` | 若 `review_round < max_review_rounds`：把 note/问题写入 context，`NEEDS_HUMAN`→`QUEUED`，编排继续返工轮；否则 → `FAILED`（保留 result/review） |
| 错误 | 非 `awaiting_acceptance` → **409**（含已是待批准、终态、无 result）；body 非法 → 400 |

鉴权：本机 MVP 与现有 `/approve` 同级（Host/本机边界）；契约 B1 完整凭证隔离不在本卡。

幂等：已 `COMPLETED`/`FAILED`/`CANCELLED` 再调用 → 409，状态不变（RT-REV-04）。

### 2.3 编排与存储职责

| 模块 | 改动（实现卡） |
|---|---|
| `store.rs` | 新增 `apply_human_review_verdict(id, accept\|reject, note)`：单条带条件 UPDATE；reject 返工时更新 `context` 与轮次计数（复用现有 context / review 字段，不新造并行状态机） |
| `orchestrator.rs` | 对外方法包装 store 转换；reject 且仍有额度时重新入队执行循环（复用现有返工 context 拼接逻辑）；accept 后走现有完成/交付路径；**禁止** Reviewer agent 调用此入口 |
| `server/src/lib.rs` | 注册路由；薄封装 |
| `web` | `TaskPanel`/`ChatView`：`awaiting_acceptance` 显示「判定通过」「判定不通过」；`labels.ts`：按 result 有无显示「待验收」/「待批准」；`taskStatusLine` 同步 |
| `an-core` | 若 API 响应增加 `wait_reason` 派生字段则导出类型并 `cargo test -p an-core` 再生 bindings；纯服务端派生可不改核心枚举 |

### 2.4 事件与追踪

- 新事件名（须进 DESIGN 第 10 节登记）：`task.human_review_accept`、`task.human_review_reject`（或统一 `task.human_review_verdict` + 字段）。
- 关联 `task_id`；有 `trace_id` 时携带（契约 R1，新写路径强制；本 API 属引擎任务路径，实现卡至少带 `task_id`）。
- 不把 Reviewer 的 `review.verdict` 当成最终业务完成；人工 accept 后交付文案可注明「经人工验收」（对齐建议 T5，本卡 UI 最低做状态文案区分）。

### 2.5 调用方兼容与失败恢复

| 场景 | 行为 |
|---|---|
| 旧客户端只知 `/approve` | 无 result 路径不变；有 result 仍 409/失败（approve SQL 已拒） |
| 并发 accept 与 cancel | `transition`/`finish_task` 条件更新；先入库存为准（S1/S2） |
| 进程在待验收时重启 | 现状：未终态标 FAILED（WP7 另议）；待验收任务同现 NEEDS_HUMAN 中断策略，**本卡不改重启语义** |
| reject 返工后再次 needs_human | 仍进 `awaiting_acceptance`；轮次受 `max_review_rounds` 约束 |

---

## 3. 没选的做法

| 做法 | 为什么没选 |
|---|---|
| 新增 `TaskStatus::WaitingAcceptance` | T1 字面「两状态」更干净，但牵动枚举、DB 字符串、桌面映射、所有 switch；首版用等待原因即可满足契约「或等价」；若实现中证明不够再升级方案 |
| 把验收并进 `/approve` | 与「执行前批准」语义混淆；approve 现条件是无 result |
| 本方案卡改业务代码 | T-65 明确禁止；高风险须方案批准后另卡 |
| 本卡做 WP4 | WP4 依赖 WP2 完成 |

---

## 4. 迁移与回滚

### 4.1 无新列（★默认）

- **迁移**：无 DDL。
- **回滚**：revert 实现 PR；路由消失后旧待验收任务仅能取消（与今相同死胡同）——故合并前须 RT/E2E 绿。

### 4.2 若增加 `wait_reason`（可选升级）

```sql
-- 参考 store.rs 补列写法；默认 NULL
ALTER TABLE tasks ADD COLUMN wait_reason TEXT;
-- 启动时 backfill：
-- NEEDS_HUMAN + result IS NULL → 'awaiting_approval'
-- NEEDS_HUMAN + result IS NOT NULL → 'awaiting_acceptance'
```

- 回滚代码后列可留（兼容读取）；或实现卡注明保留列无害。

---

## 5. 实现卡顺序（批准后）

1. **Red**：`crates/server/tests`（或 runtime）落地 RT-REV-01～04 业务断言失败；不改断言迎合实现。
2. **Green**：`store` → `orchestrator` → `server` 路由 → 前端文案与按钮。
3. **E2E-REV-01**：浏览器点「判定通过」。
4. 更新 `features.json` / COVERAGE / DESIGN §6 状态图注「NEEDS_HUMAN 按 result 分等待原因」；DESIGN §15 #4 改为已解决并链到本方案。
5. 决策状态改为有效；指定实施 PR。

**预估改动路径（实现卡）**：`crates/runtime/src/store.rs`、`orchestrator.rs`；`crates/server/src/lib.rs`；`web/src/labels.ts`、`TaskPanel.tsx`、`ChatView.tsx`；测试与 CASES/features 证据。不改 `mom/engine.rs`、`DesConnector`。

---

## 6. 验收矩阵（允许 + 拒绝）

| Case | 场景 | 期望 |
|---|---|---|
| RT-REV-01 | 待验收 + accept | `COMPLETED`；事件含 note |
| RT-REV-02 | 待验收 + reject，仍有返工额度 | 回到 `QUEUED`/执行侧；context 含 note |
| RT-REV-03 | 返工用尽 + reject | `FAILED` |
| RT-REV-04 | 已完成再 verdict | 409，不变 |
| （拒绝）待批准（无 result）调 review-verdict | 409 |
| （拒绝）待验收调 approve | 现 SQL 不生效 / 明确错误 |
| E2E-REV-01 | 浏览器判定通过 | 状态与交付文案正确 |
| GS3 | 审查 needs_human → 人通过或退回 | 无死胡同（S7） |

证据：真实 HTTP + 内存/临时 SQLite；Mock Reviewer 触发词或现有 harness。标注本地 / CI。

---

## 7. 风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| 误改编排状态机影响批准/取消竞态 | L2 | 唯一负责人；条件 UPDATE；单测竞态 |
| 文案仍统称「待批准」 | 产品 | labels 按 result 分支；单测 `labels.test.ts` |
| 与桌面「需人工处理」映射冲突 | 中 | 桌面可继续总类名；详情用等待原因 |
| 范围蔓延到 WP4/WP7 | 高 | 非目标写死；PR 检查 |

---

## 8. 请批示

**请批准本 ★方案作为 WP2 实现卡的唯一依据**：

1. 等待原因 = `NEEDS_HUMAN` + result 有无（不新增枚举）。
2. API / 用例按 REVIEW_VERDICT_DESIGN + 上表拒绝路径。
3. 唯一实施负责人改 `orchestrator.rs` / `store.rs`；本 T-65 只合文档。
4. 不加 `wait_reason` 列（默认）；若要加列请在批准时写明。

批准后开实现卡：先 Red（RT-REV-01～04）再 Green。
