# AI Desktop A0/A1 证据

日期：2026-11-20（America/Vancouver）

## 范围

- A0：`docs/ai-desktop/TRACEABILITY.md`；Case `E2E-ADNAV`、`E2E-ADLAYOUT` 登记
- A1：默认桌面壳、三区、S2/S3 工作区、SMT-03 Mock、会话/运营模块化

## 命令

```bash
cd web && npm run test:map && npm run build
CARGO_TARGET_DIR=target cargo build --locked --bin agentnexus
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test features.spec.ts -g 'E2E-CONV|E2E-MOBILE'
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts -g 'E2E-OPSDESK|E2E-OPSOFF'
```

## 结果

| 套件 | 结果 |
|---|---|
| test:map | Feature map valid: 31 features, 37 browser cases |
| desktop E2E-ADNAV / E2E-ADLAYOUT | 2 passed |
| E2E-CONV / E2E-MOBILE | 2 passed（窄屏发现卡限高，避免挤掉输入条） |
| E2E-OPSDESK / E2E-OPSOFF | 2 passed |

## 已知局限（本 PR）

- S1 浮层、证据面板、完整 AD-* 尚未做（A2+）
- SMT-03 为前端 Mock，未接真实 TaskRecord
- 运营「任务/资源」子入口仍属 D3a
