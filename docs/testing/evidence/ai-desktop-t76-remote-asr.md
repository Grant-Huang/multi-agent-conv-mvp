# 证据：T-76 可选远端 ASR

- **日期**：2026-11-23（America/Vancouver）
- **授权**：#229 后按 MIC_REAL_PLAN §7 T-76
- **范围**：`voiceRemoteAsr` + 后端调度；`desktop-asr-stream` 目的地登记；**未改** store/orchestrator；默认不上送 PCM
- **真钥**：本卡以 `remote-stub` / 无钥回退落地合同；**真钥另验**

## Case

| Case | 场景 |
|---|---|
| E2E-ADASR-01 | remote-stub 自动 partial → finalize 关联 |
| E2E-ADASR-02 | remote 无 URL → 回退 + 提示真钥另验 |
| E2E-ADVOICE / E2E-ADMIC-* | 回归 |
| vitest voiceRemoteAsr | stub 回放无网络 |

## 质量试点（非 KPI）

- 方法：stub 事件 `t_ms` 记为合成首响应样本（40ms / 110ms）
- 样本数：回放 2 条 partial；非真人麦 / 非厂商 ASR
- 真钥 live 延迟采样另附证据

## 命令与结果

```text
# America/Vancouver 2026-11-23 ~00:21–00:22
cd web && npm run build
→ tsc --noEmit && vite build OK (dist assets written)

npm test -- src/desktop/voiceRemoteAsr.test.ts
→ Test Files 1 passed / Tests 1 passed

npm run test:map
→ Feature map valid: 33 features, 82 browser cases

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADASR|ADMIC|ADVOICE'
→ 7 passed (4.3s)  # ADVOICE + ADMIC-01～04 + ADASR-01～02

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 25 passed (15.8s)
```

## Mock→真差距

| 层 | 本卡 | 仍缺口 |
|---|---|---|
| ASR 合同 | `remote-stub` NDJSON 回放驱动 `/voice/partial` | 真钥 WS/HTTP live 端到端 |
| 回退 | `asr=remote` 无 URL → webspeech +「真钥另验」 | 有钥后的 live 质量采样 |
| 音频 | 默认不上送 PCM（与 egress 文档一致） | 若开通 live 须显式开启并过 DLP |
| 双真机 | 未做 | T-77 |

## 非本卡

- 存录音；双真机 T-77；付费厂商合同；弱化断言；改 orchestrator / 新语音表
