# AI Desktop C2/M5：真实厂写方案卡（桌面审批网关 → World Adapter → DES）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23 |
| 来源 | 助手建议（主线 #1:1 指示开设计卡；**§三问待用户确认后转实现卡**） |
| 状态 | 草案（docs-only；未开工实现） |
| 影响范围 | 特性 ai-desktop（M5）；后续触及 `store.rs` / `desktop_api.rs` / 审批前端的任务卡 |
| 相关设计 | [ai-desktop/NEXUSOPS_REAL_WRITE_PLAN.md](../ai-desktop/NEXUSOPS_REAL_WRITE_PLAN.md)；[contracts/identity-and-approval.md](../contracts/identity-and-approval.md) A1–A3、B3/D1；[mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §4.3–4.5 |

**一句话结论**：桌面审批网关的执行从「计数模拟」接真实业务写入——经 World Adapter Propose/Execute/Confirm 写 DES 仿真工厂（质量处置→维修工单单一动作类型），与运营频道 OpsCase 入口同为 A1–A3 模型、两入口不合并 API；§三问（目标世界/动作类型/表扩展方式）待用户确认后开实现卡。

## 背景

MOCK_REGISTRY M5 的 C2 行只剩「真实 NexusOps 厂写仍待」：桌面审批网关（#198）的批准/升版/拒绝/恰好一次语义已真实，但执行副作用是 `execute_count` 计数模拟。另一侧，WA M2（#199/#200/#202）已落地真实 DES 写链路（Propose/Execute/Confirm，RT-WADES-09～17），且运营频道入口已在用它做真实厂写。

## 问题

不接真实写入，「人批准 → 执行 → 核验」的受控闭环在桌面入口缺最后一环：批准是真的、写入是假的；宪章原则 6（业务事实归源系统）与 7（有后果动作由固定代码执行）在桌面入口不成立。

## 决策依据

1. 主线 #1:1 用户指示继续主线下一步（2026-11-23，对话）；本卡为其建议方案，待用户确认。
2. 共享契约 B3/D1（2026-11-22 用户确认）：同一批准模型、两入口、不合并 API——桌面接入 WA 是契约预期路径。
3. 代码事实：`/api/world/des/propose|execute|confirm` 已存在（`mom_world_des.rs` router）；桌面网关 `execute_desktop_approval` 当前仅计数（`store.rs` execute_count/last_receipt）。
4. 先例：T-74 真机 mic 方案卡（docs-only → 用户确认 → 实现卡）。

## 定了什么

见 [NEXUSOPS_REAL_WRITE_PLAN.md](../ai-desktop/NEXUSOPS_REAL_WRITE_PLAN.md)：单一动作类型最小切片、案例绑定、执行链与幂等键映射、证据回填、验收草案、§三问。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 桌面网关直接写 DES、绕过 WA | 违反宪章「统一世界访问」与 WA §4 契约；两套执行路径 |
| 并入运营 OpsCase 批准 API | 决定 60 / 契约 D1：两入口不合并 |
| 多动作类型一次做完 | 超出最小切片；单一类型先打通链路 |
| 保持计数模拟不动 | M5 验收（真实动作网关：权限、审计、副作用恰好一次）无法达成 |

## 影响与后续

- 用户确认 §三问 → 开实现卡（L2，唯一实施负责人）；本文档状态改「已确认（部分：按确认结果）」。
- 实现落地后：MOCK_REGISTRY M5 状态更新、DESIGN 0.3 NexusOps 行改「部分实现」、0.3 顶部「无设计」清单移除 NexusOps。

## 出问题先看

- WA execute 幂等冲突日志（`IDEMPOTENCY_CONFLICT`）；网关 409 `revision_conflict`；approval `execution_ref` 是否与 WA `execution_id` 一致。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建（草案，待 §三问确认） | 主线 #1:1 |
