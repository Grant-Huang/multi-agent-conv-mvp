// AG.3「助手」页签与卡片的浏览器用例（定义见 docs/testing/CASES.md「助手页签与卡片」）。
// 测试服务（e2e/harness.mjs）里：claude-agent-acp 已安装（指向模拟 ACP agent，握手真实）；
// zcode-acp-server 未安装；config.toml 手写了 hand；自动发现关闭；没有内置 coding agent。
// 用例共享同一个服务、按顺序执行（后面的用例依赖前面的状态）。
import { test, expect, type Page, type Locator } from '@playwright/test';

test.describe.configure({ mode: 'serial' });

/** 打开「助手」页签，返回某个 agent 的一行 */
async function openAgents(page: Page) {
  // T-88：助手管理页签在会话视图右栏；先从侧栏进入会话
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await page.getByRole('tab', { name: '助手' }).click();
  return page.getByRole('region', { name: '助手列表' });
}
function row(list: Locator, id: string) {
  return list.locator(`[data-agent-id="${id}"]`);
}
/** 在确认卡上点"确认"，返回结果卡 */
async function confirm(page: Page) {
  const card = page.getByRole('region', { name: '确认修改' });
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: '确认', exact: true }).click();
  const result = page.getByRole('region', { name: '修改结果' });
  await expect(result).toBeVisible();
  return result;
}

test.afterEach(async ({ page }, info) => { await page.screenshot({ path: info.outputPath('page.png'), fullPage: true }); });

test('E2E-AGFIRST: 首次打开显示发现卡，关掉后不再出现', async ({ page }) => {
  await page.goto('/');
  // T-88：发现卡从首页移到设置-连接与能力（断言不变，入口路径适配）
  await page.getByTestId('open-settings').click();
  await page.getByTestId('settings-cat-connect').click();
  const card = page.getByRole('region', { name: '发现的助手' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('Claude Code');
  await card.getByRole('button', { name: '知道了', exact: true }).click();
  await expect(card).toHaveCount(0);
  await page.reload();
  await page.getByTestId('open-settings').click();
  await page.getByTestId('settings-cat-connect').click();
  await expect(page.getByRole('region', { name: '发现的助手' })).toHaveCount(0);
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.getByRole('tab', { name: '助手' })).toBeVisible();
});

test('E2E-AGLIST: 助手页签列出状态、来源和用途', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await expect(row(list, 'claude')).toContainText('已安装，未接入');
  await expect(row(list, 'claude')).toContainText('编码助手');
  await expect(row(list, 'zcode')).toContainText('未安装');
  await expect(row(list, 'hand')).toContainText('在 config.toml 中配置');
  await expect(row(list, 'general')).toContainText('内置');
});

test('E2E-AGTOML: config.toml 里的 agent 不能在这里停用', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  const hand = row(list, 'hand');
  await expect(hand).toContainText('在 config.toml 中配置');
  await expect(hand.getByRole('button', { name: '停用' })).toHaveCount(0);
});

test('E2E-AGGUIDE: 未安装的显示安装步骤，重新检测给出大白话结果', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await row(list, 'zcode').getByRole('button', { name: '看安装步骤' }).click();
  const guide = page.getByRole('region', { name: '安装步骤' });
  await expect(guide).toContainText('npm i -g zcode-acp-server');
  await expect(guide.getByRole('button', { name: '复制' }).first()).toBeVisible();
  await guide.getByRole('button', { name: '我装好了，重新检测' }).click();
  await expect(guide).toContainText('还没有安装');
});

test('E2E-AGCONNECT: 接入已安装的预设：确认卡 → 结果卡', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await row(list, 'claude').getByRole('button', { name: '接入' }).click();
  const card = page.getByRole('region', { name: '确认修改' });
  await expect(card).toContainText('接入「Claude Code」');
  await expect(card).toContainText('claude-agent-acp');
  const result = await confirm(page);
  await expect(result).toContainText('已完成');
  await expect(result).toContainText('已连通');
  await expect(row(list, 'claude')).toContainText('已接入');
  const agents = await (await page.request.get('/api/agents')).json();
  expect(agents.some((a: { id: string }) => a.id === 'claude')).toBeTruthy();
});

test('E2E-AGUNDO: 结果卡上撤销后恢复原状', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await expect(row(list, 'claude')).toContainText('已接入');
  await page.getByRole('button', { name: '撤销上一次修改' }).click();
  const result = await confirm(page);
  await expect(result).toContainText('已完成');
  await expect(row(list, 'claude')).toContainText('已安装，未接入');
});

test('E2E-AGDISABLE: 停用最后一个写代码的助手后，对话用大白话回复', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await row(list, 'claude').getByRole('button', { name: '接入' }).click();
  await confirm(page);
  await row(list, 'claude').getByRole('button', { name: '停用' }).click();
  const card = page.getByRole('region', { name: '确认修改' });
  await expect(card).toContainText('没有执行者');
  await confirm(page);
  await expect(row(list, 'claude')).toContainText('已停用');

  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await page.getByRole('textbox', { name: '输入消息' }).fill('改代码验证：修复编译错误');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const reply = page.locator('.msg.assistant').last();
  await expect(reply).toContainText('现在没有能写代码的助手');
  const text = (await reply.innerText()).trim();
  expect(text.startsWith('[')).toBeFalsy();
  expect(text).toMatch(/（技术信息：E_ROUTE_NO_AGENT）$/);
});

test('E2E-AGCHAT: 在对话里接入助手：确认卡 → 结果卡，刷新后仍在', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await page.getByRole('textbox', { name: '输入消息' }).fill('把 Claude Code 接进来验证');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const main = page.locator('main');
  const card = main.getByRole('region', { name: '确认修改' });
  await expect(card).toContainText('接入「Claude Code」');
  await card.getByRole('button', { name: '确认', exact: true }).click();
  await expect(main.getByRole('region', { name: '修改结果' })).toContainText('已连通');
  await page.reload();
  await expect(main.getByRole('region', { name: '修改结果' })).toBeVisible();
  await expect(main.getByRole('region', { name: '确认修改' })).toContainText('已确认');
});

test('E2E-AGCHOICE: 没说是哪个助手时给选择卡，点选后出确认卡', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await page.getByRole('textbox', { name: '输入消息' }).fill('帮我接个写代码的助手验证');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const main = page.locator('main');
  const choice = main.getByRole('region', { name: '选择助手' });
  await expect(choice).toBeVisible();
  await choice.getByRole('button', { name: /Cursor CLI/ }).click();
  await expect(main.getByRole('region', { name: '确认修改' })).toContainText('接入「Cursor CLI」');
});

test('E2E-AGDIAG: 未安装的助手点"排查"，看到大白话结论和安装步骤', async ({ page }) => {
  await page.goto('/');
  const list = await openAgents(page);
  await row(list, 'zcode').getByRole('button', { name: '排查' }).click();
  const report = page.getByRole('region', { name: '排查结果' });
  await expect(report).toContainText('还没有安装');
  await expect(report).toContainText('npm i -g zcode-acp-server');
  await expect(report.getByRole('button', { name: '复制' }).first()).toBeVisible();
  await expect(report).toContainText('体检');
});

test('E2E-AGHELP: 诊断包可复制；选帮手后看到完整预览，同意后看到还原过的回复', async ({ page, request }) => {
  // 帮手 hand 先握手一次，成为"已连通"
  const probe = await request.post('/api/agents/probe', { data: { id: 'hand' } });
  expect(probe.ok()).toBeTruthy();
  await page.goto('/');
  const list = await openAgents(page);
  await row(list, 'broken').getByRole('button', { name: '排查' }).click();
  const report = page.getByRole('region', { name: '排查结果' });
  // 第 3 层：诊断包经过 DLP（模拟服务把客户名换成占位符），可以复制
  const pkg = report.getByRole('region', { name: '诊断包' });
  await expect(pkg).toContainText('[CUSTOMER_001]');
  await expect(pkg).not.toContainText('ACME');
  await expect(pkg.getByRole('button', { name: '复制' })).toBeVisible();
  // 第 4 层：选帮手 → 看到将发送的完整内容和目的地 → 同意这一次
  await report.getByLabel('帮手').selectOption('hand');
  await report.getByRole('button', { name: '生成预览' }).click();
  const preview = page.getByRole('region', { name: '外发预览' });
  await expect(preview).toContainText('将发给「hand」');
  await expect(preview).toContainText('[CUSTOMER_001]');
  await expect(preview).not.toContainText('ACME');
  await preview.getByRole('button', { name: '同意发送这一次' }).click();
  const reply = page.getByRole('region', { name: '帮手的回复' });
  await expect(reply).toContainText('ACME 的配置文件');
});
