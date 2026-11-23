# WP4：任务拆分与模板执行方案

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-22 |
| 来源 | 用户授权 T-73 出方案；#216 合入 = ★方案批准；实现卡按同一惯例立即开工 |
| 状态 | **有效**（方案已批准；实现见 WP4 实现 PR） |
| 影响范围 | 执行引擎任务拆分与并行；`orchestrator.rs` / `store.rs`；Router 字段；任务面板文案 |
| 相关设计 | [WP4_IMPLEMENTATION_PLAN.md](../fencing/WP4_IMPLEMENTATION_PLAN.md)；[DESIGN.md](../DESIGN.md) §9.6；[INTEGRATION_PLAN.md](../INTEGRATION_PLAN.md) WP4 / GS4；G02；契约 S1/S2/S7、E1/R1/R2；WP2 决定 [2026-11-22-wp2-review-acceptance-plan](2026-11-22-wp2-review-acceptance-plan.md) |

**一句话结论**：采纳「父/分支/汇合 + 最小 DDL + 现有 template 校验接入编排」；#216 批准后实现卡落地编排与 E2E-TPL。

## 背景

G02：`template.rs` 已有校验与 RT-TPL-01～10，但生产零调用；GS4「多问题拆分 → 并行 → 汇合」无法端到端。WP2（#215）已提供审查待验收出口，去掉了并行分支卡在 `NEEDS_HUMAN` 的死胡同前置障碍。DESIGN §9.6 与 PLAN M4.3 要求：能串行不拆、只跑手写模板、信封缺路不填补、Composer 只转述。

## 问题

若不先定父子模型、DDL、取消/验收与汇合策略，实现会话易直接改 orchestrator 做成隐式内存图或动态 DAG，回滚面大且与契约 S7/R2 冲突。

## 决策依据

1. 用户 T-73：只出方案 PR，禁止改业务代码；触 orchestrator/store 须方案批准（对话 2026-11-22；INTEGRATION_PLAN WP4）。
2. 代码事实：`template.rs` 已实现 `TemplateRegistry` / `decide_split` / `validate_envelope` / `assemble_join`；`orchestrator`/`conversation` 未调用；`TaskRecord` 无 `parent_id`（`crates/core`）。
3. 缺口与场景：SYSTEM_GAPS G02；黄金场景 GS4；COVERAGE templates 行写明并行与 Composer 未实现。
4. WP2 已合入：分支待验收可走 `/review-verdict`（决定 2026-11-22-wp2-review-acceptance-plan）。
5. 设计约束：DESIGN §9.6「模型不现场编图」「吃不准就不拆」。

## 定了什么（方案层；实现待再批）

- ★父任务 `root` + 并行 `branch` + `join`；汇合用现有 `assemble_join`；缺路 `missing`、不填补。
- ★最小 DDL：`parent_id` / `fanout_kind` / `branch_id` / `template_id`（可空兼容旧行）。
- Router 输出 §9.6 三字段；`decide_split` 非 Split 则整单。
- 批准以 **parent** 为准；分支可走 WP2 验收；取消 parent 取消 ACTIVE 子（默认）；一支取消 → 汇合记 missing（默认不连带杀兄弟）。
- Composer **只**在 join 后对 parent 交付一次。
- 唯一实施负责人：主线编排/语义 worker；T-73 本 PR 只文档。

## 简单设计

见 [WP4_IMPLEMENTATION_PLAN.md](../fencing/WP4_IMPLEMENTATION_PLAN.md) §2～§6。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 本卡改 orchestrator/store | T-73 禁止 |
| 零 DDL 仅内存/context JSON | 取消与重启脆弱（除非用户明示接受） |
| 模型动态 DAG | §9.6 禁止 |
| 每分支单独交付 | 违背汇合回复 |

## 影响与后续

- #216 合入后已开实现卡：Red RT-TPL-11～16 → Green → E2E-TPL；DESIGN §0.3 / §9.6 / §10 已同步。
- M4.4 评测门槛可另卡；不阻塞编排接通。
- WP5/WP7 仍独立。

## 出问题先看

- `docs/fencing/WP4_IMPLEMENTATION_PLAN.md`
- `crates/runtime/src/template.rs`（`decide_split` / `assemble_join`）
- RT-TPL-01～10（基线）与拟议 RT-TPL-11～16

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-22 | 新建（待确认） | T-73 |
| 2026-11-22 | #216 合入 → 状态改有效；实现卡开工 | 用户确认 = 方案批准 |
