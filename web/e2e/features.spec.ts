import { test, expect, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

async function send(page: Page, text: string) {
  await page.getByRole('textbox', { name: '输入消息' }).fill(text);
  const response = page.waitForResponse(r => r.url().endsWith('/messages') && r.request().method() === 'POST');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const result = await response;
  expect(result.ok()).toBeTruthy();
  return result.json();
}
async function complete(page: Page, id: string) {
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${id}`)).json()).task.status).toBe('COMPLETED');
  return (await (await page.request.get(`/api/tasks/${id}`)).json()).task;
}

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.locator('.msg')).toHaveCount(0);
});
test.afterEach(async ({ page }, info) => { await page.screenshot({ path: info.outputPath('page.png'), fullPage: true }); });

test('E2E-CONV: 会话刷新和隔离', async ({ page }) => {
  const out = await send(page, '你好验证');
  await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
  await expect(page.locator('main')).not.toContainText('私有思考');
  await page.reload();
  await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
  const messages = await (await page.request.get(`/api/conversations/${out.reply.conversation_id}/messages`)).json();
  expect(messages.at(-1).id).toBe(out.reply.id);
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.locator('.msg')).toHaveCount(0);
  await page.getByRole('complementary', { name: '会话列表' }).getByRole('button', { name: '你好验证', exact: true }).click();
  await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
});

test('E2E-FILE: 交付与真实文件下载', async ({ page }) => {
  const out = await send(page, '写验证报告');
  const task = await complete(page, out.task.id);
  expect(task.review.verdict).toBe('pass');
  const artifact = task.result.artifacts.find((a: {kind: string}) => a.kind === 'file');
  await expect(page.locator('main').getByRole('button', { name: '下载', exact: true }).first()).toBeVisible();
  const downloaded = page.waitForEvent('download');
  await page.locator('main').getByRole('button', { name: '下载', exact: true }).first().click();
  const file = await downloaded;
  expect(await readFile((await file.path())!, 'utf8')).toBe(artifact.content);
  expect(file.suggestedFilename()).toBe(artifact.name.split('/').at(-1));
  const runtime = JSON.parse(await readFile('test-results/runtime/state.json', 'utf8'));
  expect(await readFile(`${runtime.workspace}/${artifact.name}`, 'utf8')).toBe(artifact.content);
});

test('E2E-APPROVE: 审批先于执行', async ({ page }) => {
  const out = await send(page, '审批验证报告');
  expect(out.task.status).toBe('NEEDS_HUMAN');
  const before = (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task;
  expect(before.result).toBeNull();
  await page.locator('main').getByRole('button', { name: '批准执行', exact: true }).click();
  const task = await complete(page, out.task.id);
  expect(task.result.artifacts.length).toBeGreaterThan(0);
});

test('E2E-CANCEL: 两步确认取消', async ({ page }) => {
  const out = await send(page, '慢验证任务');
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task.status).toBe('RUNNING');
  await page.locator('main').getByRole('button', { name: '取消任务', exact: true }).click();
  expect((await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task.status).toBe('RUNNING');
  await page.locator('main').getByRole('button', { name: '确认取消', exact: true }).click();
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task.status).toBe('CANCELLED');
});

test('E2E-DIAG: 开发者模式中的诊断与路由日志', async ({ page }) => {
  await send(page, '你好诊断验证');
  await expect(page.getByRole('tab', { name: '诊断', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '开发者模式：关', exact: true }).click();
  await page.getByRole('tab', { name: '诊断', exact: true }).click();
  await expect(page.getByRole('heading', { name: '后端', exact: true })).toBeVisible();
  await page.getByRole('tab', { name: '路由日志', exact: true }).click();
  const log = page.locator('.task-card.log').filter({ hasText: '你好诊断验证' }).first();
  await expect(log).toBeVisible();
  await log.locator('summary').click();
  await expect(log.locator('pre').first()).toContainText('respond');
});

test('E2E-MOBILE: 窄屏会话抽屉', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '☰ 会话', exact: true }).click();
  await expect(page.getByRole('complementary', { name: '会话列表' })).toBeVisible();
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.locator('.nav-backdrop')).toHaveCount(0);
  await send(page, '你好移动验证');
  await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
});
