# AI Desktop A5.2 证据（审批版本 / Run 恢复）

日期：2026-11-20（America/Vancouver）

## 范围

- 动作审批 Mock：方案版本绑定、改参失效、拒绝/超时、未知回执查证、执行副作用一次（`E2E-ADAPPR`）
- Run 恢复 Mock：双任务并行、断线重放去重、waiting_input 续跑不串写（`E2E-ADRESUME`）

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts -g 'ADAPPR|ADRESUME'
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 54 browser cases |
| ADAPPR / ADRESUME | 2 passed |
