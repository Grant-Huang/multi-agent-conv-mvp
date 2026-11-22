# 证据：AI Desktop D — 语音与跨端（fixture 媒体）

- **日期**：2026-11-21（America/Vancouver）
- **决定**：[2026-11-21-ai-desktop-d-voice-handoff](../../decisions/2026-11-21-ai-desktop-d-voice-handoff.md)
- **范围**：`desktop_voice_sessions` + `desktop_handoff`；`/api/desktop/v1` 语音/跨端；Voice/Handoff 面板接真实 HTTP；不改 orchestrator；不做真机 getUserMedia/双设备

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_d
→ 7 passed（RT-ADVOICE-01～04、RT-ADHAND-01～03）

cd web && npm run build
→ tsc + vite build 通过

cd web && npm run test:map
→ Feature map valid（本证据登记后）

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADVOICE|ADHAND'
→ 2 passed（E2E-ADVOICE、E2E-ADHAND；真实 HTTP + fixture）
```

## 断言摘要

| Case | 结果 |
|---|---|
| RT-ADVOICE-01 | deny → as-text 可继续 |
| RT-ADVOICE-02 | partial 不关联；finalize 关联任务 |
| RT-ADVOICE-03 | barge 停播报并保留 final |
| RT-ADVOICE-04 | offline 后 as-text 保留 final |
| RT-ADHAND-01 | 同 task/run/锚点；默认不共享转写 |
| RT-ADHAND-02 | 双端冲突 → resolve phone 一致 |
| RT-ADHAND-03 | voice_shared 可开关 |
| E2E-ADVOICE / ADHAND | 面板走服务权威，断言同 A5.3 |

## 非本卡

- 真机麦克风 / WebRTC / 语音质量指标
- 双真机跨端同步
