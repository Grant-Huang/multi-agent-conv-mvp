# AI Desktop A5.4 证据（受理回执）

日期：2026-11-20（America/Vancouver）

## 范围

- 受理 Mock：丢回执待确认、现有模式须回读、可靠模式原操作 ID 重试幂等、同 ID 改正文 409（`E2E-ADACCEPT`）

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts -g 'ADACCEPT'
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 53 browser cases |
| ADACCEPT | 1 passed |
| desktop.spec 全量 | 11 passed |
| build | ok |
