# C2/M2 最小共享契约：写路径授权、取消与追踪

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-22 |
| 来源 | 用户确认（「按推荐的来」；拍板方案 D1–D7） |
| 状态 | 有效（契约目录标为部分有效；实现缺口另卡） |
| 影响范围 | `docs/contracts/*`；后续触及批准 / 取消 / 追踪的任务卡；不改 PRODUCT |
| 相关设计 | [contracts/README.md](../contracts/README.md)；[identity-and-approval](../contracts/identity-and-approval.md)；[state-and-cancellation](../contracts/state-and-cancellation.md)；[events-and-trace](../contracts/events-and-trace.md)；方案稿（Project store）`docs/c2-m2-minimal-contracts-proposal.md`；#198、#199、#200、#202 |

**一句话结论**：冻结 C2 与 WA M2 已交付的写路径语义为共享契约最小集；同一批准模型两入口；取消后待发不发；新写路径强制 `trace_id`；T1/WP2 与 H2 引擎守卫另卡实现。

## 背景

三份共享契约原为草案。Desktop C2（#198）与 World Adapter M2-2～4（#199/#200/#202）已在服务层落地批准版本绑定、幂等冲突、取消待发守卫与 unknown 查证。需要把跨模块必须一致的规则写成契约，避免后续任务卡各写一套。

## 问题

若不确认最小集：新卡可能改 engine/orchestrator 扩大面；Desktop 与 OpsCase 批准语义漂移；取消/重试/迟到回执解释不一致；`task_id`/`case_id`/`execution_id` 仍无法串时间线。

## 决策依据

1. 用户 2026-11-22 对 T-59 方案回复「按推荐的来」，采纳 D1–D7 推荐项（对话；任务卡授权出方案后落契约）。
2. 已合并实现与用例：RT-ADAPPR-01～05（#198）；RT-WADES-09～17（#199/#200/#202）；服务层优先决定 [2026-11-21-wa-m2-1-green-service-layer](2026-11-21-wa-m2-1-green-service-layer.md)。
3. 产品宪章原则 4、7、8；决定 60（运营卡批准入口）；WORLD_ADAPTER §4.3–4.5；ai-desktop DESIGN §5–6。
4. 文档分层：契约改动须用户确认（[2026-11-21-doc-layers](2026-11-21-doc-layers.md)）。

## 定了什么

| 编号 | 决定 |
|---|---|
| D1 | Desktop 网关与 OpsCase **同一批准模型（A1–A3）、两入口**（B3）；不合并 API |
| D2 | **B1 凭证隔离**写入契约；实现可分阶段 |
| D3 | **真实登录本轮不排期**；不挡契约生效 |
| D4 | **S3 语义生效**；R-20261120-H2 向 engine/DesConnector 下推**实现另卡** |
| D5 | `trace_id` 在**桌面/HTTP 受理时生成**；**新写路径日志/事件必带**；旧路径渐进 |
| D6 | **对外错误码保持**现有风格；**仅日志**映射为 `E_`/`W_`（R3） |
| D7 | **确认 T1**（待批准 / 待验收分状态）；**WP2 另卡**，改 orchestrator/store 须唯一实施负责人 |
| 生效 | 已交付 C2/M2 写路径语义冻结；**下一新任务卡起**受本最小集约束 |

另确认写入契约正文：A1 版本字段注解（`plan_revision` / `revision`）；A3 幂等冲突语义；R2 保留三套业务 ID 并记关联；MOM 拥有业务状态、桌面拥有交互状态。

## 简单设计

见三份契约第 3 节表格。关联链：

```
trace_id（受理生成）
  ├─ task_id（若有）
  ├─ case_id（若进入 MOM）
  ├─ proposal_id
  └─ execution_id
```

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 合并 OpsCase 与 Desktop 批准 API | 宪章/决定 60：运营批准在频道卡；扩大非目标 |
| 本卡改 engine / DesConnector / orchestrator | 高风险；已批服务层优先 |
| 对外错误码一律改 `E_` | 破坏已绿合同与客户端；只做日志映射 |
| 本卡实施 WP2/WP4 | WP4 依赖 WP2；须另指定负责人与方案 |
| 契约追溯打断进行中里程碑实现 | 只冻结已交付语义 + 约束新卡 |

## 影响与后续

- 本 PR：更新 `docs/contracts/*`；本决策文件。
- 建议补用例 `RT-TRACE-01`（新写路径带 `trace_id`）。
- WP2：T1 实现卡（orchestrator/store）；WP4 仍依赖 WP2。
- H2：engine 取消守卫实现卡。
- 真实登录、夹具迁移、T2–T5、R4、重启语义：仍待定。

## 出问题先看

- 批准 409 / `APPROVAL_INVALID` / `revision_conflict`
- 取消后 `dispatched_count` / `cancelled_pending`（RT-WADES-11）
- 日志是否缺 `trace_id`（新写路径）

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-22 | 新建 | T-59；用户「按推荐的来」 |
