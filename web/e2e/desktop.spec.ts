import { test, expect } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
});

test('E2E-ADNAV: 桌面首页多入口打开同一 SMT-03 并返回', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(page.getByRole('heading', { name: '我的 AI 桌面' })).toBeVisible();
  await expect(page.getByRole('region', { name: '我的工作' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Agent 工作' })).toBeVisible();
  await expect(page.getByRole('region', { name: '为你关注' })).toBeVisible();
  await expect(page.getByTestId('ops-module-off')).toContainText('运营模块未启用');
  await expect(page.getByRole('tab', { name: /运营桌面/ })).toHaveCount(0);

  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);

  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('desktop-home')).toBeVisible();

  await page.getByRole('button', { name: /SMT-03|TASK-2026-018/ }).first().click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADLAYOUT: S2与S3切换后刷新仍恢复布局', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');
  await expect(page.getByTestId('workspace-task-pane')).toBeVisible();

  await page.getByRole('button', { name: '专注 S3', exact: true }).click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's3');
  await expect(page.getByTestId('workspace-task-pane')).toHaveCount(0);

  await page.getByRole('button', { name: '分屏 S2', exact: true }).click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');

  await page.reload();
  await expect(page.getByTestId('workspace')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');
});
