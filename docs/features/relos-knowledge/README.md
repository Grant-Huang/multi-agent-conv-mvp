# 组织知识 RelOS（设计草案）

| 项目 | 内容 |
|---|---|
| 特性 ID | relos-knowledge |
| 状态 | 设计中（本文件即首个交付物：设计草案，待用户批准；批准前登记为「无设计」的现状不变） |
| 所属层 | 横切 · 第 5 研发线（与[分层记忆](../conversation-memory/README.md) L4 组织知识同源） |
| 服务宪章 | [PRODUCT.md](../../PRODUCT.md) §5 CH-G4（外部系统只经标准连接器接入）；L4 组织知识（用户确认 2026-11-26 分层模型） |
| 核对基线 | 无（尚无实现基线；现状见 §4） |
| 当前版本 | 未发布 |

## 1. 意图

组织知识（设备关系、工艺因果、批次质量模式等）目前散落在仿真数据与单点调用里。RelOS 作为**组织知识的权威服务**：把「什么与什么有关系、因果如何、证据在哪」编译成可注入决策的上下文，供诊断（MOM）、分层记忆（L4 组织知识）与后续智能体使用——证据可查、发布可控、按预算取用。

## 2. 需求

以下为**提出**状态（待用户确认后冻结）：

| 编号 | 需求 | 验收标准 | 状态 | 用例 |
|---|---|---|---|---|
| REQ-relos-knowledge-01 | 上下文编译查询 | 经连接器按锚点（资产 / 产线 / 批次）请求关系上下文，返回候选关系与证据引用，带 `token_budget` 限制；超时 / 失败降级为带原因的 unknown，不阻塞调用方 | 提出 | 待登记（现状 RT 已有部分：RT-MOM-06/07 的 RelOS 观测） |
| REQ-relos-knowledge-02 | 知识发布与版本 | 组织知识条目由**有权限者**发布、带版本；未发布 / 已撤版本的候选不得作为事实返回；与 L4 记忆同源（分层记忆 §3.1） | 提出 | 待登记 |
| REQ-relos-knowledge-03 | 证据可追溯 | 每条返回的关系带证据引用（案例 / 记录 id），可点回原始记录；无证据的推断标「假设」 | 提出 | 待登记（RT-MOM-06 的 `cause_status=HYPOTHESIS` 为雏形） |

## 3. 设计（草案要点）

- **服务边界**：RelOS 是独立服务，经[连接器契约](../../contracts/connector.md)（K1–K5）接入；调用方只持连接器注册引用，不直连地址与凭据（现状 `mom_relos.rs` 直连 URL 待迁移，见契约 §6）。
- **核心 API**：`POST /v1/context/compile`（现状已对接）：锚点节点 + 策略（causal_path）+ `token_budget` + `max_relations` → 候选关系与证据引用。
- **与 L4 的同源关系**：L4 组织知识（分层记忆 §3.1）的「已发布、带版本、按授权」规则即 RelOS 的发布规则；RelOS 是 L4 的**存储与编译引擎**，分层记忆通过连接器按预算取用，不复制数据。
- **知识分级**：已发布事实（带版本）> 候选假设（带证据引用、标 HYPOTHESIS）> 仿真数据（标注 `mock_ids`，不得混入真实诊断）。现状全部为第三级（迁移目标）。
- **降级语义**：超时 / 失败 / 空 → `status = unavailable / timeout / empty`，诊断节点降级为 unknown、排除可疑设备，交给人工（现状范式保留）。

## 4. 实现入口（现状）

`crates/server/src/mom_relos.rs`（227 行）：`RelosClient`（超时 5s、token 预算 1000、max_relations 40）、`fetch()` 降级范式；唯一调用方为 MOM 诊断 E1-C；知识目前为仿真数据。

## 5. 质量

REQ-01 的现状部分已有 RT-MOM-06/07 覆盖（观测写入 / 降级）；REQ-02/03 待登记用例（发布 / 版本 / 证据追溯），登记后再实现。

## 6. 版本历史

尚未发布。

## 7. 决策

[五线分工补全](../../decisions/2026-11-26-workstream-realignment.md)（第 4 条：RelOS 设计归第 5 线；第 7 条：接入归第 5 线、调度使用归第 4 线）；[分层模型](../../decisions/2026-11-26-memory-layers-and-history.md)（L4 同源）；本文件为设计草案（待用户批准）。

## 8. 待决定

1. RelOS 服务的所有权与部署形态（本机服务 vs 远端多租）——影响连接器 transport。
2. 发布权限模型：复用[身份与审批 A7](../../contracts/identity-and-approval.md) 的审批人体系，还是 RelOS 自带角色。
3. 仿真数据与真实知识的切换开关与迁移方式。

## 9. 来源

[五线分工补全决定](../../decisions/2026-11-26-workstream-realignment.md)；[分层记忆特性档案](../conversation-memory/README.md) §3.1（L4）；`crates/server/src/mom_relos.rs`（现状代码）；`ai-desktop/DESIGN.md` 第 6 节（散见的 RelOS 描述，登记后以本档案为准）。
