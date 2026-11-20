# 运营桌面 D1-3（前端桌面）：Red 证据

日期：2026-11-19。

冻结用例 `web/e2e/ops.spec.ts` 在 D0b/D1-2 完成后仍失败在桌面页签（业务断言，不是环境问题）：

- E2E-OPSDESK / E2E-OPSDRAWER：`openDesk` → `getByRole('tab', { name: /运营桌面/ })` 找不到页签。
- E2E-OPSNOISE：频道部分通过后，最后一步点击“运营桌面”失败（见 `docs/testing/evidence/ops-desk-d0b.md`）。

本卡实现“运营桌面”页签、看板、抽屉后应转绿。
