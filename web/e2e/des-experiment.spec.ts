import { test, expect } from '@playwright/test';
import {
  prepareWaMockArtifacts,
  startWaMockServer,
  type WaMockHandle,
} from './helpers/waMockServer';

/**
 * DES 实验 / 运营入口拆分（DES-UI-01～06 → E2E-DES*）。
 * 断言已冻结（M0-0）。M0-3：六条 Green；M0-5：共用 Mock harness + DESLOOP 双浏览器上下文。
 * 独立实例（18792 + world_mock），避免与默认 harness 用例间状态串扰。
 * 定位契约见 docs/feature-map/features/world-adapter.md。
 */
let server: WaMockHandle | undefined;

test.beforeAll(async () => {
  test.setTimeout(300_000);
  prepareWaMockArtifacts();
});

test.beforeEach(async () => {
  server = await startWaMockServer(
    test.info().title.slice(0, 16).replace(/\W/g, ''),
  );
});

test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true }).catch(() => {});
  if (server) {
    await server.stop();
    server = undefined;
  }
});

function base(): string {
  if (!server) throw new Error('mock server not started');
  return server.base;
}

test('E2E-DESTRIAL: 配置试跑不向运营频道投递', async ({ page }) => {
  await page.goto(base() + '/');
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
  await page.goto(base() + '/');
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
  await page.goto(base() + '/');
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
  await page.goto(base() + '/');
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
  await page.goto(base() + '/');
  await page.getByRole('link', { name: '运营频道' }).click();
  await expect(page.getByRole('link', { name: 'DES 仿真实验' })).toHaveCount(0);
  const denied = await request.post(base() + '/api/world/approve', {
    data: { run_id: 'run-b', approval_from_run: 'run-a', plan_revision: 1 },
  });
  expect(denied.status(), await denied.text()).toBeGreaterThanOrEqual(400);
  const leak = await request.get(base() + '/api/world/runs/run-a');
  expect([401, 403, 404]).toContain(leak.status());
});

test('E2E-DESLOOP: Mock 双角色正式运行闭环', async ({ browser }, info) => {
  // M0-5：实验员 / 运营员分浏览器上下文；Then 断言与 M0-0 冻结一致。
  test.setTimeout(90_000);
  const experimenter = await browser.newContext();
  const operator = await browser.newContext();
  const expPage = await experimenter.newPage();
  const opsPage = await operator.newPage();
  try {
    await expPage.goto(base() + '/');
    await expPage.getByRole('link', { name: 'DES 仿真实验' }).click();
    await expPage.getByRole('tab', { name: '正式运行' }).click();
    await expPage.getByRole('button', { name: '启动正式运行', exact: true }).click();
    const runId = (await expPage.getByTestId('official-run-id').innerText()).trim();
    expect(runId.length).toBeGreaterThan(0);

    await opsPage.goto(base() + '/');
    await opsPage.getByRole('link', { name: '运营频道' }).click();
    const card = opsPage.getByTestId('ops-case-card').filter({ hasText: runId });
    await expect(card).toBeVisible({ timeout: 20_000 });
    await expect(card.getByRole('button', { name: '批准', exact: true })).toBeVisible({
      timeout: 20_000,
    });
    await card.getByRole('button', { name: '批准', exact: true }).click();
    await expect(card).toContainText(/supervisor|已批准/);

    await expPage.getByRole('button', { name: '单步推进', exact: true }).click();

    // 运营员保持频道上下文；reload 后若落回桌面则走侧栏入口
    await opsPage.reload();
    const sideOps = opsPage
      .getByRole('complementary', { name: '会话列表' })
      .getByRole('button', { name: /运营频道/ });
    if (await opsPage.getByTestId('ops-case-card').count() === 0) {
      if (await opsPage.getByRole('link', { name: '运营频道' }).count()) {
        await opsPage.getByRole('link', { name: '运营频道' }).click();
      } else {
        await sideOps.click();
      }
    }
    const cardAfter = opsPage.getByTestId('ops-case-card').filter({ hasText: runId });
    await expect(cardAfter).toBeVisible({ timeout: 20_000 });
    await cardAfter.getByRole('button', { name: '查看核验证据', exact: true }).click();
    await expect(opsPage.getByTestId('verify-evidence')).toBeVisible();
    await expect(opsPage.getByTestId('eval-ground-truth')).toHaveCount(0);
  } finally {
    await expPage.screenshot({ path: info.outputPath('experimenter.png'), fullPage: true }).catch(() => {});
    await opsPage.screenshot({ path: info.outputPath('operator.png'), fullPage: true }).catch(() => {});
    await experimenter.close().catch(() => {});
    await operator.close().catch(() => {});
  }
});
