import { test, expect } from '@playwright/test';

/**
 * DES 实验 / 运营入口拆分（DES-UI-01～06 → E2E-DES*）。
 * 断言已冻结（M0-0）。M0-2：E2E-DESTRIAL Green（实验页 + world_mock）。
 * E2E-DESSTART 及后续仍依赖 M0-3 运营频道投影等；不得弱化断言。
 * 定位契约见 docs/feature-map/features/world-adapter.md。
 */
test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
});

test('E2E-DESTRIAL: 配置试跑不向运营频道投递', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await expect(page.getByRole('heading', { name: 'DES 仿真实验' })).toBeVisible();
  await page.getByRole('tab', { name: '配置' }).click();
  await page.getByLabel('场景时长').fill('-1');
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText(/非法|无效|约束/);
  await page.getByLabel('场景时长').fill('480');
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await page.getByRole('button', { name: '试跑', exact: true }).click();
  await expect(page.getByTestId('trial-run-id')).toBeVisible();
  await expect(page.getByTestId('ops-delivery-count')).toHaveText('0');
});

test('E2E-DESSTART: 正式启动幂等且与试跑 ID 不同', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await page.getByRole('tab', { name: '正式运行' }).click();
  await expect(page.getByTestId('config-version')).toBeVisible();
  await expect(page.getByTestId('target-channel')).toBeVisible();
  await page.getByRole('button', { name: '启动正式运行', exact: true }).click();
  const officialId = await page.getByTestId('official-run-id').innerText();
  await page.getByRole('button', { name: '启动正式运行', exact: true }).click();
  await expect(page.getByTestId('official-run-id')).toHaveText(officialId);
  const trialId = await page.getByTestId('last-trial-run-id').innerText();
  expect(trialId).not.toEqual(officialId);
  await page.getByRole('link', { name: '运营频道' }).click();
  await expect(page.getByTestId('ops-case-card').filter({ hasText: officialId })).toHaveCount(1);
});

test('E2E-DESOPS: 运营员在桌面内批准并查看执行', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: '运营频道' }).click();
  const card = page.getByTestId('ops-case-card').first();
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: '查看方案', exact: true }).click();
  await expect(page.getByTestId('plan-revision')).toBeVisible();
  await card.getByRole('button', { name: '批准', exact: true }).click();
  await expect(card).toContainText(/supervisor|已批准/);
  await card.getByRole('button', { name: '查看执行', exact: true }).click();
  await expect(page.getByTestId('execution-receipt')).toBeVisible();
  // 批准只在频道卡上；桌面看板若可见则不得有批准按钮
  const desk = page.getByTestId('ops-desk');
  if (await desk.count()) {
    await expect(desk.getByRole('button', { name: '批准' })).toHaveCount(0);
  }
});

test('E2E-DESPAUSE: 暂停等待区别于失败且推进后回执正确', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await page.getByRole('tab', { name: '正式运行' }).click();
  await page.getByRole('button', { name: '暂停', exact: true }).click();
  await expect(page.getByTestId('run-status')).toHaveAttribute('data-status', 'paused');
  await page.getByRole('link', { name: '运营频道' }).click();
  await expect(page.getByTestId('ops-case-card').first()).toContainText(/等待仿真|已下发/);
  await expect(page.getByTestId('ops-case-card').first()).not.toContainText(/失败/);
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await page.getByRole('button', { name: '单步推进', exact: true }).click();
  await page.getByRole('link', { name: '运营频道' }).click();
  await page.reload();
  await expect(page.getByTestId('execution-receipt')).toBeVisible();
  await expect(page.getByTestId('ops-case-card')).toHaveCount(1);
});

test('E2E-DESISO: 跨运行审批被拒绝且无泄露', async ({ page, request }) => {
  await page.goto('/');
  await page.getByRole('link', { name: '运营频道' }).click();
  await expect(page.getByRole('link', { name: 'DES 仿真实验' })).toHaveCount(0);
  const denied = await request.post('/api/world/approve', {
    data: { run_id: 'run-b', approval_from_run: 'run-a', plan_revision: 1 },
  });
  expect(denied.status(), await denied.text()).toBeGreaterThanOrEqual(400);
  const leak = await request.get('/api/world/runs/run-a');
  expect([401, 403, 404]).toContain(leak.status());
});

test('E2E-DESLOOP: Mock 双角色正式运行闭环', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await page.getByRole('button', { name: '启动正式运行', exact: true }).click();
  const runId = await page.getByTestId('official-run-id').innerText();
  await page.getByRole('link', { name: '运营频道' }).click();
  const card = page.getByTestId('ops-case-card').filter({ hasText: runId });
  await card.getByRole('button', { name: '批准', exact: true }).click();
  await page.getByRole('link', { name: 'DES 仿真实验' }).click();
  await page.getByRole('button', { name: '单步推进', exact: true }).click();
  await page.getByRole('link', { name: '运营频道' }).click();
  await card.getByRole('button', { name: '查看核验证据', exact: true }).click();
  await expect(page.getByTestId('verify-evidence')).toBeVisible();
  await expect(page.getByTestId('eval-ground-truth')).toHaveCount(0);
});
