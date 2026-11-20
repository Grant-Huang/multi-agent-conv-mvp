# 运营桌面 D1-3（前端桌面）：证据

日期：2026-11-19。

## Red
见 `ops-desk-d1-3-red.md`：实现前 E2E-OPSDESK / OPSDRAWER / OPSNOISE 失败在“运营桌面”页签（D0b 起一直如此）。

## Green
```text
$ cd web && PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts
  ✓ E2E-OPSDESK
  ✓ E2E-OPSDRAWER
  ✓ E2E-OPSCHANNEL
  ✓ E2E-OPSNOISE
  ✓ E2E-OPSOFF
  5 passed
```

实现：`OpsDesk.tsx`（看板+抽屉）、`api.opsDesk/opsDeskCase/opsOpenInChannel`、`App.tsx` 主视图页签（仅 `opsUnread !== null` 时渲染）、样式。桌面无批准按钮。
