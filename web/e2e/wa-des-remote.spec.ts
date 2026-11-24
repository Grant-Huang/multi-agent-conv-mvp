import { test, expect } from '@playwright/test';
import { prepareWaMockArtifacts, startWaMockServer, type WaMockHandle } from './helpers/waMockServer';


/**
 * World Adapter M1 远程 DES 读（WA-01/02/07 → E2E-WADES / WAREAD / WAOBS）。
 * 断言已冻结（M1-1）；M1 Green 接线后期望通过。
 * 定位契约见 docs/feature-map/features/world-adapter.md、docs/mom/WA_M1_TASKS.md。
 */
test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true }).catch(() => {});
});


let server: WaMockHandle | undefined;
function base(): string {
  if (!server) throw new Error('mock server not started');
  return server.base;
}

test.beforeAll(async () => {
  test.setTimeout(300_000);
  prepareWaMockArtifacts();
});

test.beforeEach(async () => {
  server = await startWaMockServer(
    test.info().title.slice(0, 16).replace(/\W/g, ''),
  );
});

test.afterEach(async () => {
  if (server) {
    await server.stop();
    server = undefined;
  }
});

test('E2E-WADES: 远程 DES 世界绑定与跨世界隔离', async ({ page, request }) => {
  // 合同：远程 DES 读适配可用（相对 Mock 前缀）
  const bind = await request.post(base() + '/api/world/des/bind', {
    data: { factory: 'plant-a', scenario: 's1', seed: 1, request_id: 'e2e-wades' },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(bind.status()).toBe(200);
  const body = await bind.json();
  expect(body.provenance).toBe('des');
  expect(body.world_id).toBeTruthy();

  await page.goto(base() + '/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  const worldSelect = page.getByLabel('选择世界');
  await worldSelect.selectOption({ label: '刀具磨损演练 · DES' });
  await expect(page.getByLabel('数据来源')).toHaveText(/DES/);
  await expect(page.getByTestId('mom-workbench')).toHaveAttribute(
    'data-provenance',
    'des',
  );

  await page.getByRole('button', { name: '看板', exact: true }).click();
  const firstCase = page.getByRole('button', { name: /^事项 / }).first();
  await firstCase.click();
  const detail = page.getByTestId('mom-case-page');
  await expect(detail).toBeVisible();
  const caseId = await detail.getAttribute('data-case-id');
  expect(caseId).toBeTruthy();

  // T-88：看板/任务/资源是工作台自身入口（nav「工作台入口」），与侧栏「任务」同名无关
  const entryTabs = page.getByRole('navigation', { name: '工作台入口' });
  await entryTabs.getByRole('button', { name: '任务', exact: true }).click();
  await expect(detail).toHaveAttribute('data-case-id', caseId!);
  await entryTabs.getByRole('button', { name: '资源', exact: true }).click();
  await expect(detail).toHaveAttribute('data-case-id', caseId!);

  await worldSelect.selectOption({ index: 0 });
  await expect(page.getByRole('button', { name: new RegExp(caseId!) })).toHaveCount(0);
});

test('E2E-WAREAD: 断线后续读不重复且水位可辨', async ({ page }) => {
  await page.goto(base() + '/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '刀具磨损演练 · DES' });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await expect(page.getByTestId('event-watermark')).toBeVisible();
  const before = await page.getByTestId('applied-event-ids').innerText();

  await page.getByRole('button', { name: '重新读取', exact: true }).click();
  await expect(page.getByTestId('event-watermark')).toBeVisible();
  await expect(page.getByTestId('cursor-expired-banner')).toHaveCount(0);
  const after = await page.getByTestId('applied-event-ids').innerText();
  expect(after.split(',').filter(Boolean).sort().join(',')).toEqual(
    before.split(',').filter(Boolean).sort().join(','),
  );

  await page.getByTestId('inject-expired-cursor').click();
  await page.getByRole('button', { name: '重新读取', exact: true }).click();
  await expect(page.getByTestId('cursor-expired-banner')).toBeVisible();
});

test('E2E-WAOBS: 远程推进后可见正式观测且无标准答案', async ({ page }) => {
  await page.goto(base() + '/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('ops-entry-des').click();
  await expect(page.getByTestId('des-experiment')).toBeVisible();
  await page.getByRole('tab', { name: '正式运行' }).click();
  await expect(page.getByTestId('adapter-mode')).toHaveAttribute('data-mode', 'des');
  await page.getByRole('button', { name: '单步推进', exact: true }).click();
  await page.getByRole('button', { name: '← 返回桌面' }).click();
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '刀具磨损演练 · DES' });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await page.getByRole('button', { name: /^事项 / }).first().click();
  const detail = page.getByTestId('mom-case-page');
  await expect(detail.getByTestId('formal-observation')).toBeVisible();
  await expect(detail.getByTestId('formal-observation')).toContainText(
    /振动|Cpk|设备|观测/i,
  );
  await expect(detail.getByTestId('ground-truth')).toHaveCount(0);
  await expect(detail).not.toContainText(/标准答案|ground_truth/i);
});
