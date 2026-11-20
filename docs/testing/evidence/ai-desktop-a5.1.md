# AI Desktop A5.1 证据（S1 / Attention）

日期：2026-11-20（America/Vancouver）

## 范围

- S1 桌面问答与委派：不强制进工作区；提交中 ≠ 已受理；可手动打开任务（`E2E-ADASK`）
- Attention Mock：输入中普通建议不抢焦点；静音后 ordinary 不投递；urgent 仍可见（`E2E-ADATTN`）

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts -g 'ADASK|ADATTN'
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 52 browser cases |
| ADASK / ADATTN | 2 passed |
