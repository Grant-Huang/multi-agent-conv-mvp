import { test, expect } from '@playwright/test';
import { prepareWaMockArtifacts, startWaMockServer, type WaMockHandle } from './helpers/waMockServer';


/**
 * World Adapter M3 生产只读（WA-02/07 → E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY）。
 * 断言已冻结（M3-1 Red）；Green 接线后期望通过。
 * 定位契约见 docs/feature-map/features/world-adapter.md、docs/mom/WA_M3_TASKS.md。
 * 路由前缀 `/api/world/prod`（相对 Mock/DES）；写仍 Mock/DES。
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

test('E2E-WAPROD: 生产世界只读与 provenance', async ({ page, request }) => {
  const bind = await request.post(base() + '/api/world/prod/bind', {
    data: {
      site_id: 'plant-east',
      authority: 'mes-stub',
      request_id: 'e2e-waprod',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(bind.status(), '生产 Bind 合同应挂载').toBe(200);
  const body = await bind.json();
  expect(body.provenance).toBe('production');
  expect(body.world_id).toBeTruthy();
  expect(body.protocol_version).toBe(1);
  expect(body.capabilities?.actions === false || body.capabilities?.actions === undefined).toBeTruthy();

  await page.goto(base() + '/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  const worldSelect = page.getByLabel('选择世界');
  await worldSelect.selectOption({ label: '华东精加工 · 生产' });
  await expect(page.getByLabel('数据来源')).toHaveText(/正式|生产/);
  await expect(page.getByTestId('mom-workbench')).toHaveAttribute(
    'data-provenance',
    'production',
  );

  await page.getByRole('button', { name: '看板', exact: true }).click();
  await expect(page.getByTestId('event-watermark')).toBeVisible();
  await expect(page.getByTestId('formal-observation').or(page.getByTestId('mom-case-page'))).toBeVisible();

  // 写入口关闭或明确指向隔离世界
  const writeEntry = page.getByRole('button', { name: /Propose|Execute|创建维修|执行已批准/i });
  if ((await writeEntry.count()) > 0) {
    await expect(writeEntry.first()).toBeDisabled();
  }
});

test('E2E-WAPISO: 跨站点读拒绝且不泄露', async ({ page, request }) => {
  const bindA = await request.post(base() + '/api/world/prod/bind', {
    data: { site_id: 'plant-east', authority: 'mes-stub', request_id: 'e2e-wapiso-a' },
    headers: { 'X-WA-Role': 'operator', 'X-WA-Site': 'plant-east' },
  });
  expect(bindA.status()).toBe(200);
  const worldA = await bindA.json();

  const cross = await request.post(base() + '/api/world/prod/observe', {
    data: { world_id: worldA.world_id, request_id: 'e2e-wapiso-cross' },
    headers: { 'X-WA-Role': 'operator', 'X-WA-Site': 'plant-west' },
  });
  expect([401, 403, 404]).toContain(cross.status());
  const crossBody = await cross.json().catch(() => ({}));
  expect(String(crossBody.code || crossBody.error || '')).toMatch(/ACCESS_DENIED|FORBIDDEN|UNAUTHORIZED/i);

  await page.goto(base() + '/');
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '华东精加工 · 生产' });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await expect(page.getByText(/plant-west|西厂|跨站/i)).toHaveCount(0);
});

test('E2E-WAPREAD: 断线续读不重复且水位可辨', async ({ page, request }) => {
  const bind = await request.post(base() + '/api/world/prod/bind', {
    data: { site_id: 'plant-east', authority: 'mes-stub', request_id: 'e2e-wapread' },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();

  const obs = await request.post(base() + '/api/world/prod/observe', {
    data: { world_id: world.world_id, request_id: 'e2e-wapread-obs' },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(obs.status()).toBe(200);
  const snap = await obs.json();
  expect(snap.as_of).toBeTruthy();
  expect(snap.cursor).toBeTruthy();

  await page.goto(base() + '/');
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '华东精加工 · 生产' });
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

test('E2E-WAPDELAY: 多源延迟 as_of 可辨', async ({ page, request }) => {
  const bind = await request.post(base() + '/api/world/prod/bind', {
    data: {
      site_id: 'plant-east',
      authority: 'mes-stub',
      request_id: 'e2e-wapdelay',
      sources: ['mes', 'erp', 'gateway'],
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();

  const delay = await request.post(base() + '/api/world/prod/test/inject-delay', {
    data: {
      world_id: world.world_id,
      source: 'erp',
      delay_ms: 5000,
      request_id: 'e2e-wapdelay-inj',
    },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(delay.status(), '延迟注入测试控件应挂载').toBe(200);

  const obs = await request.post(base() + '/api/world/prod/observe', {
    data: { world_id: world.world_id, request_id: 'e2e-wapdelay-obs' },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(obs.status()).toBe(200);
  const snap = await obs.json();
  expect(snap.as_of).toBeTruthy();
  expect(snap.consistency === 'per_source' || Array.isArray(snap.objects)).toBeTruthy();

  await page.goto(base() + '/');
  await page.getByTestId('ops-entry-workbench').click();
  await expect(page.getByTestId('mom-workbench')).toBeVisible();
  await page.getByLabel('选择世界').selectOption({ label: '华东精加工 · 生产' });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await expect(page.getByTestId('event-watermark')).toBeVisible();
  await expect(page.getByTestId('source-as-of').or(page.getByLabel(/截至|as_of|水位/i))).toBeVisible();
});
