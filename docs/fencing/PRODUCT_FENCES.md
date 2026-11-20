# 阶段 4：工程围栏 → 平台能力

原文要求：围栏不仅是公司开发规范，还要进入 AgentNexus 产品设计。
本阶段**不大新建系统**，先把已有能力对齐到统一叙事，并标出缺口。

| 工程概念 | 已有产品能力 | 状态 | 下一步 |
|---|---|---|---|
| 任务边界 / 验收 | Conversation 目标、CASES、PR「非目标」+ Case ID | 工程侧已强制模板警告 | 任务卡片展示 `acceptance` / Case 绑定见下 |
| Risk Engine | Router 风险 + `approval_risk` + 规则只升不降 | 设计原则已有「模型建议、规则终裁」 | 在 DESIGN §7/§13 用 Risk Engine 名称归档 |
| 操作审批 | `NEEDS_HUMAN` +「批准执行」 | 执行前审批已有 | 审查进待人工的通过/退回见 [REVIEW_VERDICT_DESIGN.md](REVIEW_VERDICT_DESIGN.md) |
| 独立质检 | Reviewer + 返工轮次 | 执行/验收分离 | 保持；默认勿无审计 `allow_all` |
| Verification | Feature Map + Playwright + runtime 测试 | 工程侧 | 「完成」应对齐证据状态（COVERAGE） |
| 外发 / 权限 | DLP egress、节点本机权限、准入 S1–S7 | 部分 enforce | 继续；开发 Agent 凭证勿直推 master |
| Eval | routing eval、feedback、dream | 有工具 | 事故样本入库流程见 EVAL_LOOP.md |

## 任务验收绑定（产品侧约定）

在实现 orchestrator 字段之前，约定：

1. 每个会改行为的需求必须有 Case/Feature ID（PR 模板已要求）。
2. 前端任务卡「完成」文案不得暗示验收通过；已有「Worker 完成 ≠ 验收通过」原则。
3. 待实现：任务创建时可选 `acceptance_criteria` / `case_ids`（持久化 + UI）；**改 `store`/`orchestrator` 须另开方案并获批准**。

## Risk Engine 一句话

模型可以给出 risk；Policy / 关键词规则只能升高不能降低；达到 `approval_risk` 必须人工批准后才进入队列。这就是平台上的 Risk Engine。
