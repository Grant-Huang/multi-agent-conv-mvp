// WP3 / GS2 / S05：改代码全链路（接入 → coding → git diff → 审查 → 完成）
// 夹具：harness.mjs 临时 git workdir + an-mock-acp（写 hello.txt）
import { test, expect, type Page, type Locator } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';

async function send(page: Page, text: string) {
  await page.getByRole('textbox', { name: '输入消息' }).fill(text);
  const response = page.waitForResponse(
    (r) => r.url().endsWith('/messages') && r.request().method() === 'POST',
  );
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const result = await response;
  expect(result.ok()).toBeTruthy();
  return result.json();
}

async function complete(page: Page, id: string) {
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${id}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('COMPLETED');
  return (await (await page.request.get(`/api/tasks/${id}`)).json()).task;
}

async function openAgents(page: Page) {
  // T-88：助手管理从首页右栏页签迁到「设置 → 连接与能力」（入口路径适配，断言不变）
  await page.getByTestId('open-settings').click();
  await page.getByTestId('settings-cat-connect').click();
  return page.getByRole('region', { name: '助手列表' });
}

function row(list: Locator, id: string) {
  return list.locator(`[data-agent-id="${id}"]`);
}

async function ensureClaudeConnected(page: Page) {
  const list = await openAgents(page);
  const claude = row(list, 'claude');
  await expect(claude).toBeVisible();
  const text = await claude.innerText();
  if (text.includes('已接入')) return;
  if (text.includes('已停用')) {
    await claude.getByRole('button', { name: '恢复启用' }).click();
    const card = page.getByRole('region', { name: '确认修改' });
    await expect(card).toBeVisible();
    await card.getByRole('button', { name: '确认', exact: true }).click();
    await expect(page.getByRole('region', { name: '修改结果' })).toBeVisible();
  }
  if (!(await claude.innerText()).includes('已接入')) {
    await claude.getByRole('button', { name: '接入' }).click();
    const card = page.getByRole('region', { name: '确认修改' });
    await expect(card).toBeVisible();
    await card.getByRole('button', { name: '确认', exact: true }).click();
    await expect(page.getByRole('region', { name: '修改结果' })).toContainText('已完成');
  }
  await expect(claude).toContainText('已接入');
  // T-88：设置面板为对话框，Esc 关闭后再回会话流（ADINPUT 同语义）
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('settings-panel')).toHaveCount(0);
}

test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
});

test('E2E-CODEFLOW: 接入后改代码产出 git diff 并审查通过', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/');
  // 关掉发现卡（若有）以免挡操作
  const discovery = page.getByRole('region', { name: '发现的助手' });
  if (await discovery.count()) {
    await discovery.getByRole('button', { name: '知道了', exact: true }).click();
  }
  await ensureClaudeConnected(page);

  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.locator('.msg')).toHaveCount(0);
  const out = await send(page, '改代码验证：在仓库里写一个 hello.txt');
  expect(out.task).toBeTruthy();
  expect(out.task.role).toBe('coding');

  const task = await complete(page, out.task.id);
  expect(task.status).toBe('COMPLETED');
  expect(task.assignment?.agent_id).toBe('claude');
  expect(task.review?.verdict).toBe('pass');

  const evidence = task.result?.evidence ?? [];
  const artifacts = task.result?.artifacts ?? [];
  const hasGitStat = evidence.some((e: { kind: string }) => e.kind === 'git_diff_stat');
  const hasDiffArtifact = artifacts.some(
    (a: { kind: string; content?: string; name?: string }) =>
      a.kind === 'diff' && (a.content?.includes('hello.txt') || a.name?.includes('git')),
  );
  const hasAcpDiff = artifacts.some(
    (a: { kind: string; content?: string }) =>
      a.kind === 'diff' && (a.content?.includes('hello') || a.content?.includes('hello.txt')),
  );
  expect(
    hasGitStat || hasDiffArtifact || hasAcpDiff,
    `应有 git/diff 证据：evidence=${JSON.stringify(evidence)} artifacts=${JSON.stringify(artifacts.map((a: { kind: string; name?: string }) => ({ kind: a.kind, name: a.name })))}`,
  ).toBeTruthy();

  // 工作目录是 git 仓；hello.txt 已写出；用户暂存区无额外 staged 改动（diff --cached 空）
  const runtime = JSON.parse(await readFile('test-results/runtime/state.json', 'utf8'));
  expect(runtime.codeRepo).toBeTruthy();
  const hello = await readFile(`${runtime.codeRepo}/hello.txt`, 'utf8');
  expect(hello).toContain('hello from mock ACP agent');
  const cached = execFileSync('git', ['diff', '--cached'], {
    cwd: runtime.codeRepo,
    encoding: 'utf8',
  });
  expect(cached.trim()).toBe('');

  // 右栏可能停在「助手」；切到任务再断言执行者
  await page.getByRole('tab', { name: /任务/ }).click();
  const card = page.locator('.task-card').filter({ hasText: '改代码验证' });
  await expect(card.locator('.task-head')).toContainText(/claude|Claude/i);
});

test('E2E-FEEDBACK: 任务卡「派错了」可保存反馈', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/');
  const discovery = page.getByRole('region', { name: '发现的助手' });
  if (await discovery.count()) {
    await discovery.getByRole('button', { name: '知道了', exact: true }).click();
  }
  await ensureClaudeConnected(page);
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  const out = await send(page, '改代码验证：反馈链路用');
  await complete(page, out.task.id);

  // T-88 适配欠账（第 1 线定稿后一步适配）：「派错了」所在的任务卡在旧壳位于右栏「任务」页签，
  // 新壳中该卡既不在右栏事件流、也不在会话内联（详见 PR #270 第 4 线留言）。
  // 在落点定稿前保留原入口步骤（此处会失败，属已知适配欠账，不属断言放松）。
  await page.getByRole('tab', { name: /任务/ }).click();
  const card = page.locator('.task-card').filter({ hasText: '改代码验证：反馈链路用' });
  await card.locator('.task-head').click();
  await card.getByRole('button', { name: '派错了', exact: true }).click();
  await card.locator('select').first().selectOption('respond');
  await card.getByRole('button', { name: '保存', exact: true }).click();
  await expect(card).toContainText('已记下');
});
