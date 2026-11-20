import { test, expect } from '@playwright/test';

/**
 * World Adapter M0-4：三入口工作台 + 事项页（E2E-WA-01 → E2E-WABIND）。
 * 定位契约见 docs/feature-map/features/world-adapter.md。
 */
test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
});

test('E2E-WABIND: 三入口打开同一事项且世界隔离', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByRole('link', { name: 'MOM 工作台', exact: true }).click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'MOM 工作台' })).toBeVisible();

  const worldSelect = page.getByLabel('选择世界');
  await expect(worldSelect).toBeVisible();
  await worldSelect.selectOption({ label: '华东精加工 · 模拟' });
  await expect(page.getByLabel('当前世界标识')).toContainText('world-demo-east');
  await expect(page.getByLabel('数据来源')).toHaveText('模拟');

  await page.getByRole('button', { name: '看板', exact: true }).click();
  await page.getByRole('button', { name: '事项 插单 A · CNC-XX 振动' }).click();
  const detail = page.getByTestId('mom-case-page');
  await expect(detail).toBeVisible();
  await expect(detail).toHaveAttribute('data-case-id', 'case-insert-a');
  await expect(detail).toContainText('case_id=');
  await expect(detail.locator('code').first()).toHaveText('case-insert-a');
  await expect(detail).toContainText('world-demo-east');

  await page.getByRole('button', { name: '任务', exact: true }).click();
  await expect(page.getByRole('button', { name: '任务', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(detail).toHaveAttribute('data-case-id', 'case-insert-a');
  await page.getByRole('button', { name: '事项 插单 A · CNC-XX 振动' }).click();
  await expect(detail.locator('code').first()).toHaveText('case-insert-a');

  await page.getByRole('button', { name: '资源', exact: true }).click();
  await expect(page.getByRole('button', { name: '资源', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(detail).toHaveAttribute('data-case-id', 'case-insert-a');
  await page.getByRole('button', { name: '事项 插单 A · CNC-XX 振动' }).click();
  await expect(detail.locator('code').first()).toHaveText('case-insert-a');
  await expect(detail).toContainText('资源缺口：机、料');

  await page.getByRole('button', { name: '← 返回桌面', exact: true }).click();
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByRole('link', { name: 'MOM 工作台', exact: true }).click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await expect(worldSelect).toHaveValue('world-demo-east');
  await expect(page.getByRole('button', { name: '资源', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );

  await worldSelect.selectOption({ label: '刀具磨损演练 · DES' });
  await expect(page.getByLabel('当前世界标识')).toContainText('world-des-toolwear');
  await expect(page.getByLabel('数据来源')).toHaveText('DES');
  await expect(page.getByTestId('mom-case-page')).toHaveCount(0);
  await expect(
    page.getByRole('button', { name: '事项 插单 A · CNC-XX 振动' }),
  ).toHaveCount(0);
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await page.getByRole('button', { name: '事项 DES 刀具磨损演练事项' }).click();
  await expect(page.getByTestId('mom-case-page')).toHaveAttribute(
    'data-case-id',
    'case-des-tool',
  );
  await expect(page.getByTestId('mom-case-page')).toContainText('world-des-toolwear');
});
