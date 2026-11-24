import { test, expect } from '@playwright/test';
import {
  prepareWaMockArtifacts,
  startWaMockServer,
  type WaMockHandle,
} from './helpers/waMockServer';

/**
 * M0-5 集成：DES Mock 实例上把工作台事项接到运营频道（WA-05 适用切片）。
 * 与 des-experiment / mom-workbench 共用 harness 约定。
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

test('E2E-WACHANNEL: 工作台事项前往运营频道', async ({ page }) => {
  if (!server) throw new Error('mock server not started');
  await page.goto(server.base + '/');
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '华东精加工 · 模拟' });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await page.getByRole('button', { name: '事项 插单 A · CNC-XX 振动' }).click();
  const detail = page.getByTestId('mom-case-page');
  await expect(detail).toHaveAttribute('data-case-id', 'case-insert-a');
  await detail.getByRole('button', { name: '前往运营频道', exact: true }).click();
  await expect(
    page.getByRole('navigation', { name: '最近工作' }).getByRole('button', {
      name: /运营频道/,
    }),
  ).toBeVisible();
  // 已离开工作台，进入频道会话主区
  await expect(page.getByTestId('mom-workbench')).toHaveCount(0);
});
