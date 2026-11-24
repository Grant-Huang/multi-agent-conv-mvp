# NexusOps 真实厂写方案（桌面审批网关 → World Adapter → DES）

| 项目 | 内容 |
|---|---|
| 状态 | **设计草案，待用户确认**（§7 三问拍板后开实现卡；本文为 docs-only，不改业务代码） |
| 提出任务 | T-87（建议号，**待 #1 发号确认**） |
| 主线 | #1:1 AI 桌面产品入口 |
| 相关 | [MOCK_REGISTRY.md](MOCK_REGISTRY.md) M5「真实 NexusOps 厂写仍待」；[ai-desktop/DESIGN.md](DESIGN.md) §6；[contracts/identity-and-approval.md](../contracts/identity-and-approval.md) A1–A3、B3/D1；[mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §4.3–4.5；C2 已落地（#198）；WA M2 已落地（#199/#200/#202） |

## 1. 背景与现状

- 桌面审批网关（C2，#198）已落地：`revision` 版本绑定（A1）、展示即批准内容（部分，A2）、`execute_count`/`last_receipt` 恰好一次（A3 的**计数模拟**）。批准后执行的真实业务写入仍是计数器，即 MOCK_REGISTRY M5 的最后一块。
- World Adapter 的真实 DES 写链路（WA M2，#199/#200/#202）已落地并验收：`POST /api/world/des/propose → execute → confirm`，含提案摘要（RFC 8785 + SHA-256）、幂等键唯一约束、`IDEMPOTENCY_CONFLICT` 找回、`accepted→applied→confirmed/unknown` 状态机（[WORLD_ADAPTER.md §4.3–4.5](../mom/WORLD_ADAPTER.md)）。
- 运营频道入口已有真实厂写：OpsCase 批准经 `/api/world/des/ops/approve-execute` 驱动 WA（决定 60；RT-WADES-09～17）。
- 共享契约 B3/D1：**所有批准走同一模型（A1～A3）；运营频道卡与桌面网关是两个授权入口，不合并 API**。

## 2. 目标

SMT-03 质量处置类任务从**正常桌面入口**发起 → 桌面审批卡批准 → 经 World Adapter **真实写入仿真工厂（DES）** → 执行证据在审批卡与证据面板可查。成功路径、服务拒绝（过期/升版/撤权）、unknown 回执查证三场景可验收。

## 3. 最小切片（一种动作类型）

| 项 | 建议 |
|---|---|
| 动作类型 | 质量处置 → DES 维修工单（`action_type` 以 WA 已注册 schema 为准，实现卡核对 `mom_world_des.rs` 的 propose 校验） |
| 案例绑定 | 桌面审批增加可选 `case_ref`（MOM 案例号）；fixture 种子绑定既有运营案例，使 SMT-03 审批卡有可写的世界对象 |
| 提案时机 | 审批卡渲染时（或批准后首次 execute 前）调 WA `propose`，绑定 MOM 案例 + `select_des_plan` 选定方案 + `plan_revision`；审批卡按 **WA 保存的提案**展示（A2） |
| 执行链 | 网关 `execute` → WA `execute(proposal_id, approval_ref=approval_id@revision, idempotency_key=desktop-approval-{id}-r{revision})` |
| 证据回填 | approval 新增 `execution_ref`（execution_id + status），`last_receipt` 保留兼容；证据面板按 WA Confirm 证据展示 |
| unknown 查证 | 网关既有 `ack-unknown` 对接 WA `confirm`（查证只用原幂等键） |

## 4. 语义映射（A1–A3 两入口一致）

| 契约 | 运营频道入口（已有） | 桌面网关入口（本方案） |
|---|---|---|
| A1 版本绑定 | OpsCase `plan_revision` | 网关 `revision` ↔ WA 提案 digest 变更即新提案、重新审批 |
| A2 展示即批准 | OpsCase 卡按提案展示 | 审批卡按 WA 保存的提案展示 |
| A3 恰好一次 | `ops/approve-execute` 幂等 | `execute_count`+`last_receipt` ↔ WA 幂等键唯一约束；`IDEMPOTENCY_CONFLICT` 找回原 `execution_id` |

## 5. 验收草案（实现卡落地时定稿断言）

- RT-ADAPPR-06：批准 → 执行 → DES 真实写入成功，审批卡/证据面板可见 `execution_id` 与 WA 证据
- RT-ADAPPR-07：同审批重复执行 → 返回同一 `execution_id`（不二次写入）
- RT-ADAPPR-08：升版（bump）后旧 revision 执行 → 网关 409 与 WA 拒绝语义一致
- RT-ADAPPR-09：回执 unknown → `ack-unknown` 经 WA `confirm` 查证回填
- E2E-ADAPPR 升级：真实厂写断言替换计数断言（保留拒绝/过期路径）
- 回归：RT-WADES-09～17 全量（运营入口不受影响）；`desktop.spec` 全量

## 6. 非目标

- 生产 MES/ERP 世界不接（生产适配器按 WORLD_ADAPTER §7 另期）
- 运营频道 OpsCase 入口与 API 不动
- 不做断点续跑 / 步骤级恢复（WORLD_ADAPTER §4.4 收敛项属 WA 线）
- 不合并两入口 API（D1）

## 7. 待用户确认的三问

1. **写入目标世界**：确认为 DES 仿真工厂（经 World Adapter），生产世界不接？
2. **最小动作类型**：质量处置 → DES 维修工单，还是先做排产类？
3. **approval 表扩展**：新增 `execution_ref` 列（兼容迁移，推荐）还是复用 `last_receipt` 字符串编码？

## 8. 影响与风险

- 实现卡预计触及：`store.rs`（approval 关联列/表）、`desktop_api.rs`、`mom_world_des.rs`（只复用，争取不改）、前端 Approval 面板、RT/E2E —— L2，需唯一实施负责人并在用户确认 §7 后开工
- 风险：触碰 WA 已绿链路（RT-WADES）；缓解：WA 侧只读复用、全部改动在桌面入口侧、全量回归 WA 套件
