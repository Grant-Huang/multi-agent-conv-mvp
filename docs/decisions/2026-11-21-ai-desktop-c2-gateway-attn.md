# AI Desktop C2：动作网关 + Attention 投递（真实 HTTP）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 用户确认（批准范围①：网关+Attention；决定 69 镜像暂不做） |
| 状态 | 有效 |
| 影响范围 | 特性 ai-desktop；`store` 新表；`/api/desktop/v1` 审批与 Attention；前端 Approval / Attention 面板 |
| 相关设计 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) 第 6～7 节；契约草案 [identity-and-approval](../contracts/identity-and-approval.md) A1–A3、[state-and-cancellation](../contracts/state-and-cancellation.md) S5–S6 |

**一句话结论**：C2 在桌面扩展 API 落地「方案版本绑定的动作审批网关」与「Attention 静音/去重投递」；副作用用 store 计数模拟恰好一次；不改编排器、不并 OpsCase 批准、不改共享契约正文。

## 背景

A5 已有 `approvalMock` / `attentionStore`；B1–C1 已接受理、SSE、关联、配置与证据 ACL。MOCK_REGISTRY M5/C2 要求真实网关与投递策略可验收。

## 问题

无服务端权威时，审批版本失效、拒绝后执行、未知回执查证、静音丢弃普通建议等无法在重启与多入口间一致证明。

## 决策依据

1. 用户 2026-11-21 明确批准范围①（网关+Attention），决定 69 镜像暂不做（对话）。
2. 产品宪章原则 7：有后果动作由固定代码执行并要人确认（[PRODUCT.md](../PRODUCT.md) §5）。
3. 契约草案不变量 A1–A3（版本绑定、展示即批准内容、一次消费）与 S5–S6（副作用先查证、unknown 不盲重发）与 FEATURE_MAP AD-APPROVE / AD-ATTENTION 一致（草案；本卡引用不修改契约）。
4. 既有桌面扩展路径：`/api/desktop/v1` + 兼容建表（C1 先例，决定 69 档案）。

## 定了什么

- store 兼容新建 `desktop_approvals`、`desktop_attention_items`、`desktop_attention_prefs`；**不改** `orchestrator.rs`。
- 审批：ensure/reset、decide、bump、execute（幂等副作用计数）、ack-unknown、expire；版本不符 → 409；拒绝/过期后执行拒绝。
- Attention：投递、列表、mute；静音丢弃 ordinary/digest；urgent 必达；ordinary 同 title 去重。
- 硬门禁 RT-ADAPPR-01～05、RT-ADATTN-01～02；E2E-ADAPPR / E2E-ADATTN 接真实 HTTP。
- 不删除 Mock 源文件外壳；不合并 OpsCase `approve`；不做真实厂写；不做完整记忆 UI。

## 简单设计

见 C2 方案 API 草图：任务级当前审批一行；执行用 `execute_count` + `last_receipt`；Attention 队列按用户 `local` 存 mute。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 只做网关、Attention 另卡 | 用户选①整卡 |
| 并入 OpsCase 批准 | 宪章/决定 60：运营批准在频道卡 |
| 改 orchestrator / Run waiting_approval | 超出本卡；高风险另批 |
| 推进契约 B3 统一批准模型 | 扩大契约须另批 |

## 影响与后续

- 实现：`store.rs`、`desktop_api.rs`、前端面板、CASES/features、MOCK_REGISTRY、PLAN、evidence。
- 后续：记忆建议 UI、真实 NexusOps、完整 IdP、决定 69 镜像（另批）。

## 出问题先看

- 409 `revision_conflict`；execute 后 `execute_count`；mute 后 ordinary 是否入队。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-21 | 新建 | 用户批范围① |
