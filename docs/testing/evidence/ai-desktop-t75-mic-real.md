# 证据：T-75 真机 mic 首刀（Web Speech + fake device）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：#225 合入 = MIC_REAL_PLAN 首刀批准
- **范围**：`voiceMedia` / `voiceAsr` + VoiceMockPanel `media=real` → 现有 `/voice/*`；**未改** store/orchestrator；不上传送音频
- **模式**：默认 fixture；E2E-ADMIC 设 `__AN_DESKTOP_VOICE_MEDIA__=real` + Chromium fake device；ASR 文本经 Harness 注入（CI 无真实 SpeechRecognition 结果）

## Case

| Case | 场景 |
|---|---|
| E2E-ADMIC-01 | getUserMedia 拒绝 → denied + 文字入口 |
| E2E-ADMIC-02 | 授权 → partial 不关联 → finalize 关联 |
| E2E-ADMIC-03 | 播报中插话停播报 |
| E2E-ADMIC-04 | offline 后 as-text |
| E2E-ADVOICE | 回归 fixture 路径 |

## 命令与结果

```text
cd web && npm run build
→ 通过

cd web && npm run test:map
→ Feature map valid: 33 features, 80 browser cases

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADMIC|ADVOICE'
→ 5 passed

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 23 passed
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 拒麦 | E2E-ADMIC-01 | 通过 |
| 开麦+partial 不关联+finalize | E2E-ADMIC-02 | 通过 |
| barge | E2E-ADMIC-03 | 通过 |
| offline as-text | E2E-ADMIC-04 | 通过 |
| fixture 回归 | E2E-ADVOICE | 通过 |

## 非本卡

- 付费远端 ASR；双真机 D01；存录音；持久化记忆表
