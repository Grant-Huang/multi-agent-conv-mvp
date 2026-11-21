import { test, expect } from '@playwright/test';

/**
 * World Adapter M2 远程 DES 写（WA-03/04/05/08 → E2E-WAEXEC / WARECV / WADYN / WALOOP）。
 * 断言已冻结（M2-1 Red）；Green 接线后期望通过。
 * 定位契约见 docs/feature-map/features/world-adapter.md、docs/mom/WA_M2_TASKS.md。
 */
test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true }).catch(() => {});
});

test('E2E-WAEXEC: 远程 DES 授权并发与取消守卫', async ({ page, request }) => {
  const bind = await request.post('/api/world/des/bind', {
    data: { factory: 'plant-a', scenario: 's1', seed: 1, request_id: 'e2e-waexec' },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();
  expect(world.world_id).toBeTruthy();

  const propose = await request.post('/api/world/des/propose', {
    data: {
      world_id: world.world_id,
      action_type: 'eam.maintenance',
      target: { type: 'equipment', id: 'CNC-1' },
      parameters: { priority: 'high' },
      plan_revision: 'rev-1',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(propose.status(), 'DES Propose 合同应挂载').toBe(200);
  const proposal = await propose.json();
  expect(proposal.proposal_id).toBeTruthy();
  expect(proposal.plan_revision).toBe('rev-1');

  const execBody = {
    world_id: world.world_id,
    proposal_id: proposal.proposal_id,
    approval_ref: 'ops-approve-1',
    idempotency_key: 'e2e-waexec-idem',
    plan_revision: 'rev-1',
  };
  const first = await request.post('/api/world/des/execute', {
    data: execBody,
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(first.status()).toBe(200);
  const firstBody = await first.json();
  const second = await request.post('/api/world/des/execute', {
    data: execBody,
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(second.status()).toBe(200);
  const secondBody = await second.json();
  expect(secondBody.execution_id).toBe(firstBody.execution_id);

  const stale = await request.post('/api/world/des/execute', {
    data: { ...execBody, plan_revision: 'rev-stale', idempotency_key: 'e2e-waexec-stale' },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect([400, 409, 422]).toContain(stale.status());
  const staleBody = await stale.json();
  expect(String(staleBody.code || staleBody.error || '')).toMatch(
    /APPROVAL_INVALID|PRECONDITION_FAILED/,
  );

  await page.goto('/');
  await page.getByRole('link', { name: '运营频道' }).click();
  const card = page.getByTestId('ops-case-card').filter({ hasText: firstBody.execution_id });
  await expect(card).toBeVisible();
  await expect(card.getByTestId('execution-evidence')).toContainText(firstBody.execution_id);
});

test('E2E-WARECV: 丢回执恢复与工厂重启不误投', async ({ request }) => {
  const bind = await request.post('/api/world/des/bind', {
    data: { factory: 'plant-a', scenario: 's1', seed: 2, request_id: 'e2e-warecv' },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();

  const propose = await request.post('/api/world/des/propose', {
    data: {
      world_id: world.world_id,
      action_type: 'eam.maintenance',
      target: { type: 'equipment', id: 'CNC-1' },
      parameters: { priority: 'high' },
      plan_revision: 'rev-1',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(propose.status()).toBe(200);
  const proposal = await propose.json();

  const exec = await request.post('/api/world/des/execute', {
    data: {
      world_id: world.world_id,
      proposal_id: proposal.proposal_id,
      approval_ref: 'ops-approve-recv',
      idempotency_key: 'e2e-warecv-idem',
      plan_revision: 'rev-1',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(exec.status()).toBe(200);
  const execution = await exec.json();
  expect(execution.execution_id).toBeTruthy();

  const lose = await request.post('/api/world/des/test/lose-receipt', {
    data: { world_id: world.world_id, execution_id: execution.execution_id },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(lose.status()).toBe(200);

  const unknown = await request.post('/api/world/des/confirm', {
    data: { world_id: world.world_id, execution_id: execution.execution_id },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(unknown.status()).toBe(200);
  const unknownBody = await unknown.json();
  expect(['unknown', 'pending']).toContain(unknownBody.status);

  const recover = await request.post('/api/world/des/test/restore-receipt', {
    data: { world_id: world.world_id, execution_id: execution.execution_id },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(recover.status()).toBe(200);

  const confirmed = await request.post('/api/world/des/confirm', {
    data: { world_id: world.world_id, execution_id: execution.execution_id },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(confirmed.status()).toBe(200);
  const confirmedBody = await confirmed.json();
  expect(confirmedBody.execution_id).toBe(execution.execution_id);
  expect(['applied', 'confirmed']).toContain(confirmedBody.status);

  const restart = await request.post('/api/world/des/test/factory-restart', {
    data: { world_id: world.world_id },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(restart.status()).toBe(200);

  const afterRestart = await request.post('/api/world/des/execute', {
    data: {
      world_id: world.world_id,
      proposal_id: proposal.proposal_id,
      approval_ref: 'ops-approve-recv',
      idempotency_key: 'e2e-warecv-after-restart',
      plan_revision: 'rev-1',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(afterRestart.status()).toBeGreaterThanOrEqual(400);
  const afterBody = await afterRestart.json();
  expect(String(afterBody.code || afterBody.error || afterBody.message || '')).toMatch(
    /FACTORY|INSTANCE|SESSION|STALE|FAILED/i,
  );
});

test('E2E-WADYN: 干预后前缀不变未来改变且无标准答案', async ({ page, request }) => {
  const bind = await request.post('/api/world/des/bind', {
    data: { factory: 'plant-a', scenario: 'dynamic', seed: 3, request_id: 'e2e-wadyn' },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();

  await request.post('/api/world/des/experiment/step', {
    data: { world_id: world.world_id, minutes: 30 },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  const before = await request.post('/api/world/des/observe', {
    data: { world_id: world.world_id },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(before.status()).toBe(200);
  const beforeBody = await before.json();
  const prefixIds = (beforeBody.events || []).map((e: { event_id: string }) => e.event_id);

  const propose = await request.post('/api/world/des/propose', {
    data: {
      world_id: world.world_id,
      action_type: 'eam.maintenance',
      target: { type: 'equipment', id: 'CNC-1' },
      parameters: { priority: 'high' },
      plan_revision: 'rev-dyn',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(propose.status()).toBe(200);
  const proposal = await propose.json();
  const exec = await request.post('/api/world/des/execute', {
    data: {
      world_id: world.world_id,
      proposal_id: proposal.proposal_id,
      approval_ref: 'ops-approve-dyn',
      idempotency_key: 'e2e-wadyn-idem',
      plan_revision: 'rev-dyn',
    },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(exec.status()).toBe(200);

  await request.post('/api/world/des/experiment/step', {
    data: { world_id: world.world_id, minutes: 30 },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  const after = await request.post('/api/world/des/observe', {
    data: { world_id: world.world_id },
    headers: { 'X-WA-Role': 'operator' },
  });
  expect(after.status()).toBe(200);
  const afterBody = await after.json();
  const afterIds = (afterBody.events || []).map((e: { event_id: string }) => e.event_id);
  for (const id of prefixIds) {
    expect(afterIds).toContain(id);
  }
  expect(afterIds.length).toBeGreaterThan(prefixIds.length);

  await page.goto('/');
  await page.getByRole('link', { name: 'MOM 工作台', exact: true }).click();
  await page.getByLabel('选择世界').selectOption({ label: /DES/ });
  await page.getByRole('button', { name: '看板', exact: true }).click();
  await page.getByRole('button', { name: /^事项 / }).first().click();
  const detail = page.getByTestId('mom-case-page');
  await expect(detail.getByTestId('ground-truth')).toHaveCount(0);
  await expect(detail).not.toContainText(/标准答案|ground_truth/i);
});

test('E2E-WALOOP: 双角色远程闭环同一 execution_id', async ({ browser, request }) => {
  const bind = await request.post('/api/world/des/bind', {
    data: { factory: 'plant-a', scenario: 's1', seed: 4, request_id: 'e2e-waloop' },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(bind.status()).toBe(200);
  const world = await bind.json();

  const official = await request.post('/api/world/des/experiment/official', {
    data: {
      world_id: world.world_id,
      target_channel: 'ops-channel',
      start_key: 'e2e-waloop',
    },
    headers: { 'X-WA-Role': 'experimenter' },
  });
  expect(official.status()).toBe(200);

  const experimenter = await browser.newPage();
  const operator = await browser.newPage();
  await experimenter.goto('/');
  await experimenter.getByRole('link', { name: 'DES 仿真实验' }).click();
  await expect(experimenter.getByTestId('adapter-mode')).toHaveAttribute('data-mode', 'des');

  await operator.goto('/');
  await operator.getByRole('link', { name: '运营频道' }).click();
  const card = operator.getByTestId('ops-case-card').first();
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: '查看方案', exact: true }).click();
  await card.getByRole('button', { name: '批准', exact: true }).click();
  await card.getByRole('button', { name: '查看执行', exact: true }).click();
  const execId = await card.getByTestId('execution-id').innerText();
  expect(execId).toBeTruthy();

  await experimenter.getByRole('button', { name: '单步推进', exact: true }).click();
  await operator.getByRole('button', { name: '查证结果', exact: true }).click();
  await expect(card.getByTestId('execution-evidence')).toContainText(execId);
  await expect(card.getByTestId('execution-evidence')).toHaveAttribute(
    'data-provenance',
    'des',
  );

  await experimenter.close();
  await operator.close();
});
