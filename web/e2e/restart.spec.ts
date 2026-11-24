// E2E-RESTART：运行中重启明确失败 + 待验收重启后判定通过（WP7 / G08 / GS6 重启分支）。
// 用例见 docs/testing/CASES.md；端口 18793（自管后端，不复用 harness 的 18787）。
import { test, expect, type APIRequestContext, type Page } from '@playwright/test';
import { startRestartEnv } from './helpers/restartServer';

async function statusOf(request: APIRequestContext, base: string, taskId: string): Promise<string> {
  const r = await (await request.get(`${base}/api/tasks/${taskId}`)).json();
  return (r.task?.status as string) ?? '';
}

async function msgs(request: APIRequestContext, base: string, convId: string) {
  return (await (await request.get(`${base}/api/conversations/${convId}/messages`)).json()) as {
    role: string;
    task_id: string | null;
    content: string;
  }[];
}

/** 回到桌面首页；关掉可能存在的发现卡，并经侧栏「我的桌面」按钮回首页（T-88 拆顶栏页签；视图偏好会恢复上次停留） */
async function gotoHome(page: Page, base: string): Promise<void> {
  await page.goto(`${base}/`);
  const dismiss = page.getByRole('button', { name: '知道了' });
  if (await dismiss.isVisible().catch(() => false)) {
    await dismiss.click();
  }
  await page.getByRole('button', { name: '我的桌面', exact: true }).click();
  await page.getByTestId('s1-ask-input').waitFor({ state: 'visible', timeout: 15_000 });
}

test('E2E-RESTART: 运行中重启明确失败，待验收重启后判定通过', async ({ page }) => {
  test.setTimeout(240_000);
  const env = await startRestartEnv();
  const base = env.base;
  try {
    await gotoHome(page, base);

    // A：慢验证任务 → 进入 RUNNING 后重启服务
    const goalA = '慢验证任务：整理调查提纲';
    await page.getByTestId('s1-ask-input').fill(goalA);
    await page.getByTestId('s1-delegate').click();
    await expect
      .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
      .toContain('已受理');
    const taskA = (await page.getByTestId('s1-bg-task').innerText()).trim();
    const convA = (await page.getByTestId('s1-bg-conv').innerText()).trim();
    await expect.poll(async () => statusOf(page.request, base, taskA), { timeout: 60_000 }).toBe('RUNNING');
    await page.getByTestId('s1-bg-open').click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();

    // 执行中重启：A 应明确失败并写含原因的失败交付
    await env.restart();
    await expect.poll(async () => statusOf(page.request, base, taskA), { timeout: 60_000 }).toBe('FAILED');
    await expect
      .poll(
        async () =>
          (await msgs(page.request, base, convA)).some(
            (m) =>
              m.role === 'assistant' &&
              m.task_id === taskA &&
              m.content.includes('任务因服务重启中断，可重新委派'),
          ),
        { timeout: 30_000 },
      )
      .toBeTruthy();

    // 重连回原会话：任务卡与失败交付可见
    await gotoHome(page, base);
    await expect(page.getByRole('button', { name: /慢验证任务|整理调查提纲/ }).first()).toBeVisible({
      timeout: 15_000,
    });
    await page.getByRole('button', { name: /慢验证任务|整理调查提纲/ }).first().click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    await expect(page.locator('.task-card').filter({ hasText: goalA })).toContainText(/失败|FAILED/);
    await expect(page.locator('main')).toContainText('任务因服务重启中断，可重新委派');

    // B：人工验收任务 → NEEDS_HUMAN 后重启，判定通过仍走通（先回桌面首页再委派）
    await gotoHome(page, base);
    const goalB = '人工验收验证：重启后判定';
    await page.getByTestId('s1-ask-input').fill(goalB);
    await page.getByTestId('s1-delegate').click();
    await expect
      .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
      .toContain('已受理');
    const taskB = (await page.getByTestId('s1-bg-task').innerText()).trim();
    const convB = (await page.getByTestId('s1-bg-conv').innerText()).trim();
    await expect.poll(async () => statusOf(page.request, base, taskB), { timeout: 60_000 }).toBe('NEEDS_HUMAN');

    await env.restart();
    await expect
      .poll(async () => statusOf(page.request, base, taskB), { timeout: 60_000 })
      .toBe('NEEDS_HUMAN');
    await expect
      .poll(async () => statusOf(page.request, base, taskA), { timeout: 10_000 })
      .toBe('FAILED');
    await page.getByRole('button', { name: /人工验收验证|重启后判定/ }).first().click();
    await expect(page.locator('main')).toContainText('待验收');
    await page.locator('main').getByRole('button', { name: '判定通过', exact: true }).click();
    await expect
      .poll(async () => statusOf(page.request, base, taskB), { timeout: 60_000 })
      .toBe('COMPLETED');
    await expect
      .poll(
        async () =>
          (await msgs(page.request, base, convB)).some(
            (m) => m.role === 'assistant' && m.task_id === taskB && m.content.trim().length > 0,
          ),
        { timeout: 60_000 },
      )
      .toBeTruthy();
  } finally {
    await env.close();
  }
});
