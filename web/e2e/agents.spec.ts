// AG.3「助手」页签与卡片的浏览器用例（定义见 docs/testing/CASES.md「助手页签与卡片」）。
// 测试服务（e2e/harness.mjs）里：claude-agent-acp 已安装（指向模拟 ACP agent，握手真实）；
// zcode-acp-server 未安装；config.toml 手写了 hand；自动发现关闭；没有内置 coding agent。
// 用例共享同一个服务、按顺序执行（后面的用例依赖前面的状态）。
import { test, expect, type Page, type Locator } from '@playwright/test';

test.describe.configure({ mode: 'serial' });

/** 打开「助手」页签，返回某个 agent 的一行 */
async function openAgents(page: Page) {
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
  const card = page.getByRole('region', { name: '发现的助手' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('Claude Code');
  await card.getByRole('button', { name: '知道了', exact: true }).click();
  await expect(card).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('tab', { name: '助手' })).toBeVisible();
  await expect(page.getByRole('region', { name: '发现的助手' })).toHaveCount(0);
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
