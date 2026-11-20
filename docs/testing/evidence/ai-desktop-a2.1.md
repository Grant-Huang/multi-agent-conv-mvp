# AI Desktop A2.1 证据（阅读锚点 / 执行卡展开）

日期：2026-11-20（America/Vancouver）

## 范围

- 运营频道：按 Conversation 持久化首条可见 `Message.id` + 偏移；离开后再进恢复
- 工作区：执行卡展开按 Task 本地持久化
- 用例：`E2E-ADREAD`（`ops.spec.ts`）

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts -g ADREAD
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 50 browser cases |
| ops E2E-ADREAD | 1 passed |
