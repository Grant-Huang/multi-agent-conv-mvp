import { test, expect } from '@playwright/test';

// 体验地图 UX-05 的实时性与动作约束（#322 / INTENT-2026-021、023）。
// I3：桌面打开时注入提醒，不刷新，“为你关注”实时出现；
// I4：浮层“稍后”保留提醒（“为你关注”仍在）、“打开”直达关联任务、
//     “忽略”后浮层与“为你关注”一致移除（且明示会移除）。

test.beforeEach(async ({ page }) => {
  await page.request.post('/api/desktop/v1/snapshot/fixture/reset');
  // 前置：清空其他用例留在服务端的提醒（EX-02：提醒不随夹具重置清空），
  // 使本用例对浮层条数的断言只反映自己注入的提醒
  const cur = await (await page.request.get('/api/desktop/v1/attention')).json();
  for (const it of cur?.items ?? []) {
    await page.request.delete(`/api/desktop/v1/attention/${encodeURIComponent(it.id)}`);
  }
  // 前置：普通建议未被静音（与 E2E-UX05 相同的串扰防护）
  await page.request.post('/api/desktop/v1/attention/mute', { data: { mute_ordinary: false } });
});

test('E2E-UX05-I3I4: 提醒实时进“为你关注”；稍后不丢、打开直达任务、忽略两处一致移除', async ({ page }) => {
  await page.goto('/');
  const title = `建议：复核 SMT-03 证据 ${Date.now()}`;

  // 桌面已打开时注入提醒（不刷新页面）
  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'ordinary', title, body: '首件记录与上周对比缺一处复核', task_id: 'TASK-2026-018' },
  });

  // UX-05-I3（CH-U3）：“为你关注”实时出现该提醒（浮层轮询节奏内）
  const watch = page.getByRole('region', { name: '为你关注' }).getByRole('button').filter({ hasText: title });
  await expect(watch).toBeVisible({ timeout: 15_000 });

  // 浮层同时出现，展示原因与来源（关联任务）
  const toastItem = page.getByTestId('attn-item-ordinary').filter({ hasText: title });
  await expect(toastItem).toBeVisible({ timeout: 15_000 });
  await expect(toastItem).toContainText('普通建议');
  await expect(toastItem).toContainText('TASK-2026-018');

  // UX-05-I4（CH-U3）：稍后 → 浮层收起（不再显示该提醒），“为你关注”仍在
  await toastItem.getByTestId('attn-snooze').click();
  await expect(page.getByTestId('attn-root')).toHaveCount(0);
  await expect(watch).toBeVisible();

  // 打开 → 直达关联任务的工作区，返回桌面后浮层仍在（未被移除）
  const title2 = `建议：复核 SMT-03 证据 B ${Date.now()}`;
  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'ordinary', title: title2, body: '复核提示 B', task_id: 'TASK-2026-018' },
  });
  const toastItem2 = page.getByTestId('attn-item-ordinary').filter({ hasText: title2 });
  await expect(toastItem2).toBeVisible({ timeout: 15_000 });
  await toastItem2.getByTestId('attn-open').click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText('TASK-2026-018');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(toastItem2).toBeVisible();

  // 忽略 → 浮层与“为你关注”一致移除（忽略动作旁明示会移除）
  await expect(toastItem2.getByTestId('attn-dismiss-hint')).toContainText('为你关注');
  await toastItem2.getByTestId('attn-dismiss').click();
  await expect(page.getByTestId('attn-root')).toHaveCount(0);
  await expect(page.getByRole('region', { name: '为你关注' })).not.toContainText(title2);
});
