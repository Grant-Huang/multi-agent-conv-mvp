# AI Desktop A3 证据（证据面板 / 历史）

日期：2026-11-20（America/Vancouver）

## 范围

- 首页「查看证据」打开工作区 + 侧栏证据栈（列表 → 预览 → 来源）；Esc/返回上层
- 失败、过期、空列表明确状态（Mock）
- 历史：筛选、固定/关注/隐藏偏好持久化；打开任务后筛选恢复

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 31 features, 41 browser cases |
| desktop.spec（含 ADEVID/ADHISTORY） | 6 passed |
