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

test('E2E-TICKER: 简短回复后接走马灯，历史从旧到新且不带步骤编号，正文在最后，运行中有指示', async ({ page }) => {
  const out = await send(page, '写验证报告');
  await complete(page, out.task.id);
  // 期望的进度文字：按时间顺序取非助手事件，相邻重复只保留一条（与界面的进度列表规则一致）
  const events = (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).events as Array<{ kind: string; message: string }>;
  const expected: string[] = [];
  for (const e of events) {
    if (e.kind === 'assistant_message' || !e.message.trim()) continue;
    if (expected.at(-1) !== e.message) expected.push(e.message);
  }
  expect(expected.length).toBeGreaterThan(1);

  // 走马灯挂在任务的第一条消息（简短回复）下面；最终正文在它之后，且不在走马灯里
  const ackBubble = page.locator('main .msg.assistant').first().locator('.bubble');
  const ticker = ackBubble.locator('.ticker');
  await expect(ticker).toHaveCount(1);
  const finalBubble = page.locator('main .msg.assistant').last().locator('.bubble');
  await expect(finalBubble.locator('.ticker')).toHaveCount(0);
  expect((await ticker.boundingBox())!.y).toBeLessThan((await finalBubble.boundingBox())!.y);

  // 收起时只显示最新一行
  const line = ticker.locator('.ticker-line');
  await expect(line.locator('.ticker-text')).toHaveText(expected.at(-1)!);

  // 展开：从旧到新，每行都是工作内容，没有"第 N 步"前缀
  await line.click();
  const items = await ticker.locator('.ticker-history li').allTextContents();
  expect(items).toEqual(expected);
  for (const t of items) expect(t).not.toMatch(/^第\s*\d+\s*步/);

  // 走马灯背景与正文一致；字体比正文小
  const tickerBg = await ticker.evaluate((el) => getComputedStyle(el).backgroundColor);
  const bodyBg = await ackBubble.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(tickerBg).toBe(bodyBg);
  const tickerFont = await line.locator('.ticker-text').evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  const bodyFont = await ackBubble.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  expect(tickerFont).toBeLessThan(bodyFont);

  // 完成后没有运行指示
  await expect(page.locator('main .ticker-spin')).toHaveCount(0);

  // 运行中：走马灯行最右侧有动画 SVG；取消后指示消失
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  const slow = await send(page, '慢验证任务');
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${slow.task.id}`)).json()).task.status).toBe('RUNNING');
  const spin = page.locator('main .ticker-spin');
  await expect(spin).toBeVisible();
  expect(await spin.evaluate((el) => getComputedStyle(el).animationName)).not.toBe('none');
  const row = (await page.locator('main .ticker-line').boundingBox())!;
  const mark = (await spin.boundingBox())!;
  expect(mark.x + mark.width).toBeGreaterThanOrEqual(row.x + row.width - 2);
  await page.locator('main').getByRole('button', { name: '取消任务', exact: true }).click();
  await page.locator('main').getByRole('button', { name: '确认取消', exact: true }).click();
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${slow.task.id}`)).json()).task.status).toBe('CANCELLED');
  await expect(page.locator('main .ticker-spin')).toHaveCount(0);
});

test('E2E-FAILCARD: 失败任务的执行历史从旧到新、无编号，失败原因并入历史末尾', async ({ page }) => {
  const out = await send(page, '失败验证任务');
  await expect.poll(async () => (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task.status).toBe('FAILED');
  const detail = await (await page.request.get(`/api/tasks/${out.task.id}`)).json();
  const reason: string = detail.task.result.errors.join('；');
  expect(reason.length).toBeGreaterThan(0);

  // 期望的历史：非助手事件按时间顺序、相邻重复只留一条；末尾的"任务结束：FAILED"换成失败原因
  const expected: string[] = [];
  for (const e of detail.events as Array<{ kind: string; message: string }>) {
    if (e.kind === 'assistant_message' || !e.message.trim()) continue;
    if (expected.at(-1) !== e.message) expected.push(e.message);
  }
  expected[expected.length - 1] = reason;

  // 走马灯挂在简短回复下，失败态；收起时那一行只显示历史的最新一条（即失败原因）
  const ticker = page.locator('main .msg.assistant').first().locator('.bubble .ticker');
  await expect(ticker).toHaveClass(/is-failed/);
  const line = ticker.locator('.ticker-line');
  await expect(line.locator('.ticker-text')).toHaveText(expected.at(-1)!);

  // 展开：从旧到新，无编号列表（UL 且不显示序号）
  await line.click();
  const items = await ticker.locator('.ticker-history li').allTextContents();
  expect(items).toEqual(expected);
  expect(await ticker.locator('.ticker-history').evaluate((el) => el.tagName)).toBe('UL');
  expect(await ticker.locator('.ticker-history').evaluate((el) => getComputedStyle(el).listStyleType)).toBe('none');

  // 不再有单独的错误段和"失败"状态行
  await expect(page.locator('main .task-reason')).toHaveCount(0);
  await expect(page.locator('main .task-status.s-failed')).toHaveCount(0);
});

test('E2E-ACK: 简短回复完整显示用户意图，不截断', async ({ page }) => {
  // 超过 80 字的意图，结尾带标记：截断时标记会丢失
  const goal = '写验证报告：' + '请覆盖中期选举对科技股的影响，并逐条列出依据来源；'.repeat(4) + '结尾标记ACK-END';
  expect(goal.length).toBeGreaterThan(80);
  const out = await send(page, goal);
  await complete(page, out.task.id);
  const ack = page.locator('main .msg.assistant').first().locator('.bubble');
  await expect(ack).toContainText('结尾标记ACK-END');
  await expect(ack).not.toContainText('…');
});

test('E2E-ASSIGN: 任务卡显示执行者', async ({ page }) => {
  const out = await send(page, '写执行者验证报告');
  await complete(page, out.task.id);
  const card = page.locator('.task-card').filter({ hasText: '写执行者验证报告' });
  // 标题行（不展开）就能看到是哪个 agent 在做
  await expect(card.locator('.task-head')).toContainText('General Agent');
  await card.locator('.task-head').click();
  const section = card.locator('section.section').filter({ has: page.getByRole('heading', { name: '执行者', exact: true }) });
  await expect(section).toContainText('General Agent');
  await expect(section).toContainText('主力');
  await expect(section).toContainText('本机');
});

test('E2E-APPROVEINFO: 待批准卡片说明交给谁、预计用什么、为什么要批', async ({ page }) => {
  const out = await send(page, '审批说明验证');
  expect(out.task.status).toBe('NEEDS_HUMAN');
  // 对话区的待批准卡片，批准前就能看到
  const info = page.locator('main .approval-info');
  await expect(info).toContainText('将交给');
  await expect(info).toContainText('General Agent');
  await expect(info).toContainText('write_file');
  await expect(info).toContainText('风险等级 High');
  await expect(page.locator('main').getByRole('button', { name: '批准执行', exact: true })).toBeVisible();
});
