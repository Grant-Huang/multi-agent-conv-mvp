# 证据：T-77 双设备 D01（双 browser context）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：#232 后按 MIC_REAL_PLAN §7 T-77
- **范围**：双会话/双 browser context 落地 D01（手机发起 → 纠正关联 → 桌面续接）；对端转写受 `voice_shared` 门闩；**未改** orchestrator/store 表；不扩远端 ASR 真钥
- **第二物理机**：本卡以双 context 合同落地；**第二物理机另验**

## Case

| Case | 场景 |
|---|---|
| E2E-ADD01-01 | 双 context：手机发起关联；桌面未授权不共享 |
| E2E-ADD01-02 | 授权共享后桌面可见转写并可续写草稿 |
| E2E-ADD01-03 | 先不关联再纠正关联；共享后对端可见 |
| E2E-ADHAND / ADVOICE | 回归 |

## D01 质量试点（非 KPI）

| 样本 | 方法 | 说明 |
|---|---|---|
| 首响应 | 端 A `voice-start` → 首次 `voice-partial` 可见的墙钟差 | fixture 注入；ADD01-01 记录 ≥0ms |
| 插话停止 | `voice-play` → `voice-barge` 至 `playing=已停` | ADVOICE 契约回归 |
| 任务识别 | finalize 后 `linked_task_id` = SMT-03；纠正路径 ADD01-03 | 合同路径样本 |
| 误打断 | 本卡不采真麦误触发 | 第二物理机另验 |

## 命令与结果

```text
# America/Vancouver 2026-11-23
cd web && npm run build
→ tsc --noEmit && vite build OK

npm run test:map
→ Feature map valid: 33 features, 87 browser cases

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADD01|ADHAND|ADVOICE'
→ 5 passed (5.0s)

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 28 passed (18.7s)
```


## Mock→真差距

| 层 | 本卡 | 仍缺口 |
|---|---|---|
| 双设备 | 双 Playwright browser context | 第二物理机（手机+桌面） |
| 转写共享 | 前端对端视图按 `voice_shared` 门闩 | 服务端按设备 ACL 脱敏（另卡，若需） |
| 媒体 | fixture 语音路径 | 真麦双端 |

## 非本卡

- 远端 ASR 真钥合同；存录音；改 orchestrator/store；记忆持久表；阅读锚点另线
