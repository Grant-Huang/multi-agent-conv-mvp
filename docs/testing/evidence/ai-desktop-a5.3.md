# AI Desktop A5.3 证据（语音 / 跨端续接）

日期：2026-11-20（America/Vancouver）

## 范围

- 语音 Mock：拒麦权限、部分/最终转写、插话停播、断线改文字（`E2E-ADVOICE`）
- 跨端续接 Mock：同一 Task/Run/阅读锚点、双端草稿冲突可选、未授权转写不共享（`E2E-ADHAND`）

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts -g 'ADVOICE|ADHAND'
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 54 browser cases |
| ADVOICE / ADHAND | 2 passed |
| build | ok |
