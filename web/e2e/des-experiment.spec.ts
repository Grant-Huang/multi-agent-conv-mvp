import { test, expect } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

/**
 * DES 实验 / 运营入口拆分（DES-UI-01～06 → E2E-DES*）。
 * 断言已冻结（M0-0）。M0-2：E2E-DESTRIAL Green；M0-3：运营频道投影打通 DESSTART/OPS 等。
 * 独立实例（18792 + world_mock），避免与默认 harness 用例间状态串扰。
 * 定位契约见 docs/feature-map/features/world-adapter.md。
 */
const root = fileURLToPath(new URL('../../', import.meta.url));
const base = 'http://127.0.0.1:18792';
let dist = '';
let work = '';
let child: ChildProcess | undefined;
let logs = '';

test.beforeAll(async () => {
  test.setTimeout(300_000);
  execFileSync('cargo', ['build', '--locked', '--bin', 'agentnexus'], {
    cwd: root,
    env: { ...process.env, CARGO_TARGET_DIR: 'target' },
    stdio: 'pipe',
  });
  work = mkdtempSync(path.join(tmpdir(), 'des-browser-'));
  dist = path.join(work, 'dist');
  execFileSync('npx', ['vite', 'build', '--outDir', dist, '--emptyOutDir'], {
    cwd: path.join(root, 'web'),
    stdio: 'pipe',
  });
});

async function startServer(name: string) {
  const dir = mkdtempSync(path.join(work, `${name}-`));
  const workspace = path.join(dir, 'workspace');
  mkdirSync(workspace, { recursive: true });
  const config = path.join(dir, 'config.toml');
  writeFileSync(
    config,
    [
      '[server]',
      'bind="127.0.0.1:18792"',
      `db_path=${JSON.stringify(path.join(dir, 'app.db'))}`,
      `static_dir=${JSON.stringify(dist)}`,
      '[llm]',
      'base_url="http://127.0.0.1:1/v1"',
      'model="unused"',
      'max_retries=0',
      'timeout_secs=2',
      '[agents.general]',
      `workspace_dir=${JSON.stringify(workspace)}`,
      'websearch_command=""',
      '[agents.coding]',
      'enabled=false',
      '[acp]',
      'auto_detect=false',
      '[nodes]',
      'discovery=false',
      '[mom]',
      'world_mock=true',
      '[logging]',
      `dir=${JSON.stringify(path.join(dir, 'logs'))}`,
    ].join('\n'),
  );
  logs = '';
  child = spawn(path.join(root, 'target/debug/agentnexus'), [config], {
    cwd: root,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout?.on('data', (c) => {
    logs += c.toString();
  });
  child.stderr?.on('data', (c) => {
    logs += c.toString();
  });
  const handle = child;
  await expect
    .poll(
      async () => {
        if (handle.exitCode !== null) throw Error(logs);
        try {
          return (await fetch(base + '/api/health')).status;
        } catch {
          return 0;
        }
      },
      { timeout: 30_000 },
    )
    .toBe(200);
}

async function stopServer() {
  const handle = child;
  child = undefined;
  if (handle && handle.exitCode === null) {
    await new Promise<void>((resolve) => {
      const t = setTimeout(() => handle.kill('SIGKILL'), 7000);
      handle.once('exit', () => {
        clearTimeout(t);
        resolve();
      });
      handle.kill('SIGINT');
    });
  }
  const out = path.join(root, 'web/test-results/des');
  mkdirSync(out, { recursive: true });
  writeFileSync(path.join(out, 'server.log'), logs);
}

test.beforeEach(async () => {
  await startServer(test.info().title.slice(0, 16).replace(/\W/g, ''));
});
test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
  await stopServer();
});

test('E2E-DESTRIAL: 配置试跑不向运营频道投递', async ({ page }) => {
  await page.goto(base + '/');
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
  await page.goto(base + '/');
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
  await page.goto(base + '/');
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
  await page.goto(base + '/');
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
  await page.goto(base + '/');
  await page.getByRole('link', { name: '运营频道' }).click();
  await expect(page.getByRole('link', { name: 'DES 仿真实验' })).toHaveCount(0);
  const denied = await request.post(base + '/api/world/approve', {
    data: { run_id: 'run-b', approval_from_run: 'run-a', plan_revision: 1 },
  });
  expect(denied.status(), await denied.text()).toBeGreaterThanOrEqual(400);
  const leak = await request.get(base + '/api/world/runs/run-a');
  expect([401, 403, 404]).toContain(leak.status());
});

test('E2E-DESLOOP: Mock 双角色正式运行闭环', async ({ page }) => {
  await page.goto(base + '/');
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
