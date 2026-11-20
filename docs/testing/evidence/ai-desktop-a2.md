# AI Desktop A2 证据（草稿 / 作用域）

日期：2026-11-20（America/Vancouver）

## 范围

- 工作区草稿按 Task 本地隔离；模拟延迟发送；卸载后迟到回执入 sessionStorage
- 上下文作用域：自动 / 继续 / 独立；转为任务与纠正关联审计
- 会话迟到刷新：`refreshConversation` 校验 `currentRef` 不覆盖他会话

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 31 features, 39 browser cases |
| E2E-ADNAV / ADLAYOUT / ADDRAFT / ADSCOPE | 4 passed |
