import { test, expect, type Page } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

// 体验地图 UX-01（docs/experience-map/journeys/UX-01.md）的确定性约束。
// 只断言“当前已经成立”的约束（基线补录）；尚不成立的写在旅程的 known_deviations 里，
// 由产品意图审查跟踪，不在这里写成失败用例。
// 体验探针：统计本旅程的操作步数与整页跳转次数，作为附件输出，只记录趋势，不设门槛。

test.beforeEach(async ({ page }) => {
  await page.request.post('/api/desktop/v1/snapshot/fixture/reset');
  // 前置条件：SMT-03 在服务端没有其他会话留下的草稿。草稿上云（T-86）后，其他用例在
  // 别的浏览器会话写的草稿会留在服务端；有旧草稿时新输入可能被覆盖（间歇，见
  // INTENT-2026-008 / 约束 UX-01-I7，欠账跟踪）。这里只检验本旅程自己的约束。
  const url = `/api/desktop/v1/drafts/task/${SMT03_TASK_ID}`;
  const cur = await (await page.request.get(url)).json();
  const v = cur?.draft?.version ?? 0;
  if (v > 0) await page.request.delete(`${url}?sent_version=${v}`);
});

function probe(page: Page) {
  let navigations = 0;
  let steps = 0;
  page.on('framenavigated', f => {
    if (f === page.mainFrame()) navigations++;
  });
  return {
    step() { steps++; },
    snapshot() { return { steps, full_page_navigations: Math.max(0, navigations - 1) }; },
  };
}

test('E2E-UX01: 桌面发现 SMT-03 → 工作区 → 证据 → 专注 → 离开再返回，对话可达且草稿与位置恢复', async ({ page }, info) => {
  const p = probe(page);
  const workspaceInput = page.getByRole('textbox', { name: '工作区输入' });
  await page.goto('/');

  // UX-01-I1（CH-U1）：首页对话入口可用
  await expect(page.getByRole('textbox', { name: 'S1 输入' })).toBeEditable();
  // 发现：异常卡片在“我的工作”里，并标明“模拟”（CH-P8）
  const card = page.getByTestId('card-smt-03');
  await expect(card).toBeVisible();
  await expect(card).toContainText('模拟');

  await card.click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(workspaceInput).toBeEditable(); // UX-01-I1

  // UX-01-I3：打开证据、关闭证据后回到原工作区，对话输入仍可用
  await workspaceInput.fill('请对比上周同批次的首件检验记录'); p.step();
  await page.getByRole('button', { name: '证据', exact: true }).click(); p.step();
  await expect(workspaceInput).toBeEditable();
  await page.getByRole('button', { name: '关闭证据' }).click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');

  // UX-01-I2（CH-U2）：切换专注模式再回来，草稿不丢
  await page.getByRole('button', { name: '专注 S3' }).click(); p.step();
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');
  await page.getByRole('button', { name: '分屏 S2' }).click(); p.step();
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');

  // UX-01-I2（CH-U2）：打开设置再关闭，回到原处，草稿不丢
  await page.getByRole('button', { name: '设置与管理' }).click(); p.step();
  await page.keyboard.press('Escape'); p.step();
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');

  // UX-01-I2（CH-U2）：离开回桌面做别的事，再从卡片返回，草稿与任务恢复
  await page.getByRole('button', { name: '返回桌面', exact: true }).click(); p.step();
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await page.getByTestId('card-smt-03').click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');

  // UX-01-I4（CH-U2）：整个旅程不发生整页跳转
  const metrics = p.snapshot();
  expect(metrics.full_page_navigations).toBe(0);
  await info.attach('ux01-probe.json', { body: JSON.stringify({ journey: 'UX-01', ...metrics }, null, 2), contentType: 'application/json' });
});
