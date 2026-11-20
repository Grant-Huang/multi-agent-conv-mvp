import { test, expect, type Page, type APIRequestContext } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// 业务内省 S6（E2E-INTRO*）。独立实例 18792，开启 [introspection] 并用 fixture 造数。
const root = fileURLToPath(new URL('../../', import.meta.url));
const on = 'http://127.0.0.1:18792';
let dist = '';
let work = '';
let child: ChildProcess | undefined;
let logs = '';
let pushThreshold = 4.0;

test.beforeAll(async () => {
  test.setTimeout(300_000);
  execFileSync('cargo', ['build', '--locked', '--bin', 'agentnexus'], {
    cwd: root,
    env: { ...process.env, CARGO_TARGET_DIR: 'target' },
    stdio: 'pipe',
  });
  work = mkdtempSync(path.join(tmpdir(), 'intro-browser-'));
  dist = path.join(work, 'dist');
  execFileSync('npx', ['vite', 'build', '--outDir', dist, '--emptyOutDir'], {
    cwd: path.join(root, 'web'),
    stdio: 'pipe',
  });
});

async function startServer(name: string, threshold = 4.0) {
  pushThreshold = threshold;
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
      '[introspection]',
      'enabled=true',
      'min_samples=1',
      `push_threshold=${pushThreshold}`,
      'daily_cap=10',
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
          return (await fetch(on + '/api/health')).status;
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
  const out = path.join(root, 'web/test-results/intro');
  mkdirSync(out, { recursive: true });
  writeFileSync(path.join(out, 'server.log'), logs);
}

test.beforeEach(async () => {
  const title = test.info().title;
  const th = title.includes('INTROLABEL') ? 5.0 : 4.0;
  await startServer(title.slice(0, 12).replace(/\W/g, ''), th);
});
test.afterEach(async () => {
  await stopServer();
});

async function runFixture(request: APIRequestContext, fixture: string) {
  const r = await request.post(on + '/api/introspection/run', { data: { fixture } });
  expect(r.ok(), await r.text()).toBeTruthy();
  const j = await r.json();
  expect(j.status).toBe('done');
}

async function openApp(page: Page) {
  await page.goto(on + '/');
  await expect(page.getByRole('button', { name: '＋ 新会话' })).toBeVisible();
}

test('E2E-INTROVIEW: 内省页查看与否决', async ({ page, request }) => {
  await runFixture(request, 'adoption_push');
  await openApp(page);
  await page.getByRole('tab', { name: /内省/ }).click();
  await expect(page.getByTestId('intro-panel')).toBeVisible();
  const item = page.locator('.intro-item').first();
  await expect(item).toBeVisible();
  await item.click();
  await expect(page.getByTestId('intro-detail')).toBeVisible();
  await page.getByTestId('intro-reject').click();
  await expect(page.locator('.intro-item')).toHaveCount(0);
  // 规则未变：再跑仍可产出（幂等同一 id）；否决记录仍在详情接口
  const list = await (await request.get(on + '/api/introspection/findings?include_rejected=true')).json();
  expect(list.findings.length).toBeGreaterThan(0);
  expect(list.findings.some((f: { verdict: string | null }) => f.verdict === 'rejected')).toBeTruthy();
});

test('E2E-INTROLABEL: 未推送原因与标注', async ({ page, request }) => {
  await runFixture(request, 'adoption_hold');
  await openApp(page);
  await page.getByRole('tab', { name: /内省/ }).click();
  const item = page.locator('.intro-item').first();
  await item.click();
  await expect(page.getByTestId('intro-gate-reason')).toContainText('不推送');
  await page.getByTestId('intro-label-push').click();
  const unread = await (await request.get(on + '/api/introspection/unread')).json();
  expect(unread.unread).toBe(0);
});

test('E2E-INTROPUSH: 主窗口内省消息', async ({ page, request }) => {
  await runFixture(request, 'adoption_push');
  const unread = await (await request.get(on + '/api/introspection/unread')).json();
  expect(unread.unread).toBeGreaterThanOrEqual(1);
  const mainId = unread.conversation_id as string;
  await openApp(page);
  await page.getByRole('button', { name: '主窗口' }).click();
  await expect(page.getByTestId('intro-msg').first()).toBeVisible();
  await expect(page.getByTestId('intro-card').first()).toBeVisible();
  // 无任务卡
  const tasks = await (await request.get(on + `/api/tasks?conversation_id=${mainId}`)).json();
  expect(Array.isArray(tasks) ? tasks.length : 0).toBe(0);
  // 其他会话无内省消息
  await page.getByRole('button', { name: '＋ 新会话' }).click();
  await expect(page.getByTestId('intro-msg')).toHaveCount(0);
});

test('E2E-INTROUNREAD: 主窗口角标清零', async ({ page, request }) => {
  await runFixture(request, 'adoption_push');
  await openApp(page);
  await expect(page.getByTestId('intro-unread')).toHaveText(/^[1-9]/);
  await page.getByRole('button', { name: '主窗口' }).click();
  await expect(page.getByTestId('intro-unread')).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('button', { name: '＋ 新会话' })).toBeVisible();
  // 刷新后仍为 0（未再次点开也不应回升）
  await expect.poll(async () => {
    const u = await (await request.get(on + '/api/introspection/unread')).json();
    return u.unread;
  }).toBe(0);
  // 再推一条（新 fixture 模板需不同 scope——用 hold 阈值不够；再 run 同 fixture 不重推）
  // 用第二次 boot 前：换用 API 再写——同 finding 不重推，故用 adoption_hold 在阈值 4 下也会推（单案例得分 4）
  const r = await request.post(on + '/api/introspection/run', { data: { fixture: 'adoption_hold' } });
  expect(r.ok(), await r.text()).toBeTruthy();
  await expect.poll(async () => {
    const u = await (await request.get(on + '/api/introspection/unread')).json();
    return u.unread;
  }).toBeGreaterThanOrEqual(1);
  await page.reload();
  await expect(page.getByTestId('intro-unread')).toHaveText(/^[1-9]/);
});
