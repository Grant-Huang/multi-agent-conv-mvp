# AI Desktop D：语音与跨端服务权威（fixture 媒体）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 用户确认（选①：D1+D2 服务权威 + fixture 媒体；真机另卡） |
| 状态 | 有效 |
| 影响范围 | 特性 ai-desktop；store 新表；`/api/desktop/v1` 语音与跨端；Voice/Handoff 面板 |
| 相关设计 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) §8、D01；MOCK_REGISTRY M6 |

**一句话结论**：D 用服务端权威替换语音会话与跨端草稿 Mock；CI 用 fixture 注入麦权限/相位/转写，不做真实 getUserMedia/双真机；不改编排器。

## 背景

A5.3 已有 `voiceMock` / `handoffMock`；B–C 已接桌面扩展 API。M6/D 要求可验收的服务路径与 E2E 真实 HTTP。

## 问题

仅 localStorage 时无法跨刷新/进程证明拒麦、最终转写才关联、插话停播报、草稿冲突择一、未授权转写不共享。

## 决策依据

1. 用户 2026-11-21 批准范围①（对话）。
2. DESIGN D01：手机语音发起、纠正关联、桌面续接；未授权语音不共享（[ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) §9）。
3. 宪章原则 8：真实与模拟可辨——fixture 媒体在证据中标明 NOT_VERIFIED 真机指标（[PRODUCT.md](../PRODUCT.md) §5）。
4. 与 C1/C2 同路径：`/api/desktop/v1` + 兼容建表。

## 定了什么

- store：`desktop_voice_sessions`、`desktop_handoff`；**不改** `orchestrator.rs`。
- 语音：reset/permission/start/partial/finalize/barge/offline/as-text/share；部分转写不关联任务。
- 跨端：双端草稿版本、冲突、resolve、voice_shared。
- RT-ADVOICE-01～04、RT-ADHAND-01～03；E2E-ADVOICE/ADHAND 真实 HTTP。
- 真机 mic/WebRTC/语音质量指标：**本卡非目标**。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 本卡强制 getUserMedia + 双真机 | CI 不稳定；用户选① |
| 只做 D1 | 用户选 D1+D2 整卡 |

## 影响与后续

- 实现见 PR；真机试点另卡。

## 出问题先看

- finalize 前 `linked_task_id` 是否为空；handoff `voice_shared` 默认 false。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-21 | 新建 | 用户选① |
