# approval：人工审批与审查验收

两类等待共用 `NEEDS_HUMAN`，用 `result` 区分（WP2 / 契约 T1）：

| 等待原因 | 判定 | 文案 | 动作 |
|---|---|---|---|
| 待批准 | `NEEDS_HUMAN` 且无 result | 待批准 | `POST .../approve` → 排队执行 |
| 待验收 | `NEEDS_HUMAN` 且有 result | 待验收 | `POST .../review-verdict` accept/reject |

## Sub-features

- **执行前批准**：Router 风险 ≥ `[orchestrator].approval_risk`（默认 high），或 Policy 规则要求；对话内与任务卡内有「批准执行」；`ApprovalInfo` 说明将交给谁、预计工具、为什么要批
- **审查后验收**：Reviewer `needs_human`（或审查失败）后停在待验收；「判定通过」→ COMPLETED；「判定不通过」→ 有额度返工否则 FAILED
- 两种等待均可取消（两步确认）

## How to get to it (user POV)

- 批准：发高风险请求 → 「待批准」→ 「批准执行」
- 验收：任务经审查需人工 → 「待验收」→ 「判定通过」/「判定不通过」

## Driving it with Playwright / HTTP

```ts
// 待批准
const out = await send(page, '审批验证：部署到生产');
expect(out.task.status).toBe('NEEDS_HUMAN');
expect((await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task.result).toBeNull();
await page.locator('main').getByRole('button', { name: '批准执行', exact: true }).click();

// 待验收（E2E-REV；受控模型：目标含「人工验收」→ review needs_human）
const rev = await send(page, '人工验收验证报告');
await expect.poll(async () => (await (await page.request.get(`/api/tasks/${rev.task.id}`)).json()).task.status).toBe('NEEDS_HUMAN');
await page.locator('main').getByRole('button', { name: '判定通过', exact: true }).click();
```

HTTP：`POST /api/tasks/{id}/approve`；`POST /api/tasks/{id}/review-verdict` body `{"verdict":"accept"|"reject","note":"..."}`。

## Verify（判据）

- 批准前：`status === NEEDS_HUMAN`，`result` 为空；点击后才 RUNNING/COMPLETED。
- 验收前：`status === NEEDS_HUMAN`，`result` 非空；文案「待验收」；判定通过后 COMPLETED 且有交付。
- RT-REV-01～04：accept/reject/用尽/终态 409。

## Gotchas

- 「预计工具」是 Router **预估**，不是硬限制。
- 待验收任务调 `/approve` 会 409（SQL 要求无 result）；待批准调 `/review-verdict` 同样 409。
- 批准 / 验收 / 取消都经 `App.taskAction`，失败显示在顶部红色错误栏。

## Backed by

源码：`policy.rs`、`orchestrator.rs`、`store.rs`、`ChatView.tsx`、`TaskPanel.tsx`、`labels.ts`、`ApprovalInfo.tsx`
用例：E2E-APPROVE、E2E-APPROVEINFO、E2E-REV；RT-ASSIGN-05、RT-REV-01～04
验证状态：自动化已覆盖。准入接口（`/api/admission`）的审批见 [routing-and-admission.md](routing-and-admission.md)。
