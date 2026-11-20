# 运营桌面 D2（卡片批准）：Red 证据

日期：2026-11-20。

实现前（D1-3 完成后）：
- 运营频道 `OpsCase` 卡无「批准」按钮；冻结的 E2E-OPSAPPROVE 在 `getByRole('button', { name: '批准' })` 失败。
- `POST /api/mom/desk/cases/{id}/approve` 不存在 → 404（RT-OPSDESK-11 业务断言失败）。

公开类型变更：`ChatCard::OpsCase` 增加 `scope` / `plan_revision`（决定 60）。
