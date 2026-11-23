# AI Desktop：真机 mic 接入方案（T-74）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23 |
| 来源 | **助手建议**（方案卡；待用户确认） |
| 状态 | 待用户确认 |
| 影响范围 | 特性 ai-desktop；文档；后续实现卡范围 |
| 相关设计 | [ai-desktop/MIC_REAL_PLAN.md](../ai-desktop/MIC_REAL_PLAN.md)；DESIGN §8/D01；MOCK_REGISTRY M6；[2026-11-21-ai-desktop-d-voice-handoff](2026-11-21-ai-desktop-d-voice-handoff.md) |

**一句话结论（建议）**：真机麦只负责采集与 ASR；语音会话状态继续走现有 `/api/desktop/v1/.../voice/*` 服务权威；CI 默认 fixture；首实现用 Web Speech + fake device E2E，不上传送音频、不改 orchestrator。

## 背景

D 已接服务权威 + fixture 媒体。用户要求 Mock M3 夹具主线收口后，下一刀只出真机 mic **方案**，不开实现。

## 问题

缺一份可批准的接法：如何把 `getUserMedia` 挂到现有 voice 契约，并划清验收、风险与非目标。

## 决策依据

1. 用户 2026-11-23 授权「仅方案卡」。
2. 既有决定：D 用 fixture、真机另卡（[2026-11-21-ai-desktop-d-voice-handoff](2026-11-21-ai-desktop-d-voice-handoff.md)）。
3. DESIGN D01 与 §9 语音指标；宪章真实/模拟可辨。

## 定了什么（建议，待确认）

- 分层：MediaCapture → ASR 适配器 → 现有 voice API。
- 首版 ASR：Web Speech；远端 ASR / 全双工后置。
- 不改 orchestrator；不新建表；首版不上传原始音频。
- 用例草案 E2E-ADMIC-01～04；确认后再冻 CASES。
- 双真机 / D01 指标另实现卡。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 本卡直接写 getUserMedia 代码 | 用户要求仅方案 |
| 首版强制远端 ASR + 存录音 | 隐私与 egress 未批；范围过大 |
| 合并双真机到第一刀 | CI/设备不稳定；依赖真机链路先通 |

## 影响与后续

- 全文：[MIC_REAL_PLAN.md](../ai-desktop/MIC_REAL_PLAN.md)。
- 用户确认 §9 三问后，再开建议编号 T-75 实现卡。

## 出问题先看

- 是否仍保持「partial 不关联 Task」；`media` 旗标是否默认 fixture。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建（助手建议） | T-74 方案卡 |
