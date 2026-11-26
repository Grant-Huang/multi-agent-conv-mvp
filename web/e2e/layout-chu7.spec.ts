import { test, expect, type Page } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

// 体验宪章 CH-U7（#321 / INTENT-2026-020）：桌面本身不滚动；各栏按需滚动；
// 对话输入框浮动，不随内容滚动。验收宽度 360 / 768 / 1024 / 1440。
// 断言口径：页面 scrollHeight 等于视口（无页面级滚动）；把内容区滚到底和顶，
// 对话输入框位置不变且在视口内；用较长内容（展开执行、多条消息）构造溢出。

const SIZES: Array<{ width: number; height: number }> = [
  { width: 360, height: 844 },
  { width: 768, height: 600 },
  { width: 1024, height: 700 },
  { width: 1440, height: 900 },
];

test.beforeEach(async ({ page }) => {
  await page.request.post('/api/desktop/v1/snapshot/fixture/reset');
});

async function pageMetrics(page: Page) {
  return page.evaluate(() => ({
    scrollHeight: document.documentElement.scrollHeight,
    innerHeight: window.innerHeight,
  }));
}

async function inputY(page: Page, testId: string) {
  const box = await page.getByTestId(testId).boundingBox();
  return box?.y ?? null;
}

async function chatInputY(page: Page) {
  const box = await page.locator('.composer textarea').boundingBox();
  return box?.y ?? null;
}

test('E2E-ADLAYOUT-CHU7: 页面不滚动；首页/工作区/会话页输入框不随内容滚动且始终在视口内', async ({ page }, info) => {
  test.setTimeout(150_000);
  const rows: Array<Record<string, unknown>> = [];
  for (const size of SIZES) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto('/');

    // CH-U7 约束 1：整个窗口（页面）不出现滚动
    const home = await pageMetrics(page);
    expect(home.scrollHeight, `页面级滚动 @${size.width}`).toBeLessThanOrEqual(home.innerHeight);

    // 首页：S1 输入框固定——把首页栏内容滚到底再滚到顶，输入框位置不变且在视口内
    const s1Before = await inputY(page, 's1-ask-input');
    await page.evaluate(() => {
      const el = document.querySelector('.desktop-home-scroll') ?? document.querySelector('.desktop-home');
      if (el) { el.scrollTop = el.scrollHeight; }
    });
    await page.waitForTimeout(100);
    const s1Mid = await inputY(page, 's1-ask-input');
    await page.evaluate(() => {
      const el = document.querySelector('.desktop-home-scroll') ?? document.querySelector('.desktop-home');
      if (el) { el.scrollTop = 0; }
    });
    await page.waitForTimeout(100);
    const s1After = await inputY(page, 's1-ask-input');
    expect(s1Before).not.toBeNull();
    expect(s1Mid, `首页输入框随内容滚走 @${size.width}`).toBe(s1Before);
    expect(s1After, `首页输入框滚顶后未复位 @${size.width}`).toBe(s1Before);
    expect(s1Before!, `首页输入框不在视口内 @${size.width}`).toBeLessThanOrEqual(size.height);

    // 工作区：展开执行构造更高内容，输入框固定。
    // 窄屏 S2 双栏水平挤压下指针点击可能被裁剪（INTENT-2026-005 / #308 的窄屏问题，不在本
    // issue 范围）；展开执行用键盘激活（原生可聚焦控件的一等路径），不影响本约束的测量。
    await page.getByTestId('card-smt-03').click();
    await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
    const execToggle = page.getByTestId('workspace-exec-toggle');
    await expect(execToggle).toBeVisible();
    // 展开「执行」只是本用例构造溢出的手段（被测约束是输入框位置）。
    // 工作区打开后短时间内会整体重挂载一次（窄屏下更快），派发过早会落在被替换的旧节点上，
    // 因此重试到展开成功；窄屏 S2 双栏水平挤压使指针事件不可达（INTENT-2026-005 / #308，
    // 不在本 issue 范围），与 desktop.spec.ts ADINPUT 一致用合成 click 事件。
    await expect(async () => {
      await execToggle.dispatchEvent('click');
      await expect(execToggle).toHaveAttribute('aria-expanded', 'true');
    }).toPass({ timeout: 15_000 });
    const wsBefore = await inputY(page, 'workspace-input');
    await page.evaluate(() => {
      const el = document.querySelector('.workspace-session-scroll') ?? document.querySelector('.workspace-session');
      if (el) { el.scrollTop = el.scrollHeight; }
    });
    await page.waitForTimeout(100);
    const wsMid = await inputY(page, 'workspace-input');
    await page.evaluate(() => {
      const el = document.querySelector('.workspace-session-scroll') ?? document.querySelector('.workspace-session');
      if (el) { el.scrollTop = 0; }
    });
    await page.waitForTimeout(100);
    const wsAfter = await inputY(page, 'workspace-input');
    expect(wsBefore).not.toBeNull();
    expect(wsMid, `工作区输入框随内容滚走 @${size.width}`).toBe(wsBefore);
    expect(wsAfter, `工作区输入框滚顶后未复位 @${size.width}`).toBe(wsBefore);
    expect(wsBefore!, `工作区输入框不在视口内 @${size.width}`).toBeLessThanOrEqual(size.height);

    // 返回桌面，继续下一个宽度
    await page.getByRole('button', { name: '返回桌面', exact: true }).click();
    rows.push({ width: size.width, s1Before, s1Mid, wsBefore, wsMid });
  }

  // 会话页：多条消息构造溢出，消息区单独滚动，输入框固定（当前已成立，作为回归护栏）。
  // 会话页在 1024 宽验证（360 宽的移动端导航覆盖层会拦截指针，属 #308 窄屏范围）。
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话' }).click();
  const chatInput = page.getByRole('textbox', { name: '输入消息' });
  await chatInput.fill('你好，验证一下会话页布局');
  await chatInput.press('Enter');
  await expect(page.locator('.messages .msg.assistant').first()).toBeVisible({ timeout: 60_000 });
  const chatBefore = await chatInputY(page);
  await page.evaluate(() => {
    const el = document.querySelector('.messages');
    if (el) { el.scrollTop = 0; }
  });
  await page.waitForTimeout(100);
  const chatAfter = await chatInputY(page);
  expect(chatBefore).not.toBeNull();
  expect(chatAfter, `会话页输入框随内容滚动 @360`).toBe(chatBefore);

  await info.attach('layout-chu7.json', { body: JSON.stringify(rows, null, 2), contentType: 'application/json' });
});
