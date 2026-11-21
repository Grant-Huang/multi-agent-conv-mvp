# 文档分层：宪章与共享契约是刚性层，其余保持柔性；按低冲突方式推进

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 用户确认（方向“认可”，并要求“制定计划，按计划推进”） |
| 状态 | 部分生效：分层和新文件已落地（PR #188）；修改 `AGENTS.md`、PR 模板、CI 待单独批准；三份共享契约是草案，待确认 |
| 影响范围 | 全部文档；`AGENTS.md`、`.github/pull_request_template.md`、CI（待批准） |
| 相关设计 | [文档体系整理计划](../DOC_SYSTEM_PLAN.md)、[产品宪章](../PRODUCT.md)、[共享契约](../contracts/README.md)、[DESIGN.md 第 0.5 节](../DESIGN.md#05-文档规则) |

**一句话结论**：文档分成五层，只有产品宪章和共享契约改动要用户确认，其余走普通 PR；推进时只加新文件、不碰热点文件，新规则先警告后强制，契约不追溯到进行中的里程碑。

## 背景

这个项目由多个 AI agent 团队并行开发（Claude Code、Cursor 的会话，以及夜间评审和修复 agent），人只在关键处拍板。agent 每次开工都按根目录的 `AGENTS.md` 读取规则，不记得之前的讨论。项目两周里从对话调度 MVP 扩展到制造业 AI 桌面，文档随之长到约 190 篇。

“共享契约”指多个特性都要用、而且必须一致的约定，例如“谁能批准、一次批准绑定什么”“取消后还能不能发外部写操作”“用什么标识把一件事从头串到尾”。

## 问题

1. 产品主线容易分散，新工作线开工时没有要求说明为哪个目标服务。
2. 跨特性的约定各特性各写一套，结果不一致。
3. 设计文档没有核对基线，过时了没人发现。
4. 决策编号撞号，很多文档找不到入口。
5. 整理文档本身不能打断其他团队。

## 决策依据

1. **主线分散**：2026-11-16～19 约 6 条线并行（AG 对话式接入、FT 提速、DLP、内省、MOM/DES、围栏），2026-11-20 才由决定 61（AI 桌面）、61（World Adapter）、62 收敛（见 `DECISIONS.md` 对应条目的日期）。
2. **约定各做一套**：任务审批、MOM 批准、桌面审批是三套写法；对话任务重启即失败，MOM 案例可恢复（[DESIGN.md](../DESIGN.md) 第 6 节、[contracts/state-and-cancellation.md](../contracts/state-and-cancellation.md) 第 2 节）；台账 R-20261120-H2：已取消案例仍下发写操作。
3. **文档过时**：`DESIGN.md` 16 天未跟上系统（见 [2026-11-21-design-overview](2026-11-21-design-overview.md) 的依据第 1 条）。
4. **热点文件**：2026-11-18～21 三天内，`docs/testing/CASES.md` 被改 61 次、`docs/PLAN.md` 59 次、`docs/testing/features.json` 57 次、`docs/DECISIONS.md` 55 次（`git log --name-only` 统计）；三个活跃分支的冲突都在 `DECISIONS.md`（`git merge-tree` 模拟合并）。
5. **改规则的入口**：Claude Code 会话开始时加载 `CLAUDE.md`（本仓库经 `@AGENTS.md` 导入 `AGENTS.md`），Cursor 自动读取根目录 `AGENTS.md`；`docs/` 下的新文档不会被自动读取（[Claude Code 文档](https://code.claude.com/docs/en/memory)、[Cursor Rules 文档](https://cursor.com/docs/rules)）。
6. **用户意见**（2026-11-21）：认可“宪章 + 共享契约作为刚性层”和“决策一条一个文件”；担心其他团队推进中会冲突；要求“制定计划，按计划推进”。

## 定了什么

- **分层**：L0 产品宪章 → L1 架构与共享契约 → L2 特性设计 → L3 计划与任务卡 → 探索区（有到期日），另加决策目录。
- **门槛**：L0、L1 改动要出方案、经用户确认；L2、L3、探索区走普通 PR；特性偏离契约时先改契约。
- **决策**：一条一个文件（写法见 [2026-11-21-decision-format](2026-11-21-decision-format.md)），`DECISIONS.md` 冻结为历史档案。
- **推进方式**：先加新文件；不碰 `CASES.md`、`PLAN.md`、`features.json`；不搬目录、不改编号、不批量改文档头；CI 规则先警告约一周再阻断；契约从下一个里程碑起生效，进行中的工作不追溯。

## 简单设计

| 层 | 位置 | 改动门槛 |
|---|---|---|
| L0 产品宪章 | [PRODUCT.md](../PRODUCT.md) | 用户确认 + 决策 |
| L1 架构与共享契约 | [DESIGN.md 第 0 节](../DESIGN.md#0-系统全景)、[contracts/](../contracts/README.md) | 方案 + 用户批准 |
| L2 特性设计 | DESIGN 第 0.3 节索引里每个特性的权威设计 | 普通 PR |
| L3 计划与任务卡 | [PLAN.md](../PLAN.md) 与各特性计划 | 随时 |
| 探索区 | [explore/](../explore/README.md) | 有到期日，不作依据 |
| 决策 | [decisions/](README.md) | 只增不改 |

完整的阶段划分、验收和 L3 改动方案见 [文档体系整理计划](../DOC_SYSTEM_PLAN.md) 第 3、4 节。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 一次性搬目录、全面套模板 | 约 400 处链接、要改 L3 文件，正在推进的分支会大量冲突 |
| 契约立即约束进行中的里程碑（WA M1、AI 桌面 C2） | 会打断正在推进的团队；改为下一个里程碑起生效 |
| 继续在 `DECISIONS.md` 末尾追加编号 | 撞号和冲突的主要来源（依据第 4 条） |
| 只靠文档约定，不加 CI 检查 | agent 不一定会读到或遵守；需要机器守边界 |

## 影响与后续

- 已完成（PR #188）：`DESIGN.md` 第 0 节、`PRODUCT.md`、`contracts/` 三份草案、`decisions/`、`explore/`、整理计划。
- 待用户批准：修改 `AGENTS.md`（阅读顺序、决策写法）、PR 模板加“设计一致性”三问、CI 文档检查（计划第 4 节）；建议同时把宪章和契约加进 `REVIEW.md`、`FIX.md` 的必读清单。
- 待用户确认：三份共享契约的“建议补充”和“待决定”。
- 已在跑的 agent 会话不会自动读到新规则，需要单独通知一次。

## 出问题先看

- [文档体系整理计划](../DOC_SYSTEM_PLAN.md) 的阶段状态。
- 新特性是否在 `DESIGN.md` 第 0.3 节登记；新决策是否写进了 `decisions/`。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-21 | 新建 | [2026-11-21-design-overview](2026-11-21-design-overview.md) |
| 2026-11-21 | 按新的决策写法补齐背景、决策依据、简单设计；状态改为“部分生效” | [2026-11-21-decision-format](2026-11-21-decision-format.md) |
| 2026-11-21 | 文档层级编号改为 D0～D3（本文中的 L0～L3 即 D0～D3），以免与风险等级 L0～L3 重名；本文正文不改 | [2026-11-21-dev-standard-and-guardrails](2026-11-21-dev-standard-and-guardrails.md) |
