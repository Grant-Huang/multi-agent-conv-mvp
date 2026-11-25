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
  // 首页拥挤度探针（CH-U4、CH-U5，只记录趋势）：可见可交互控件数与技术词命中数。
  // 校准基线：T-88 之前 37 / 主区 21 / 技术词 11；之后 21 / 6 / 1（见 docs/review/product-intent/）。
  const homeClutter = await page.evaluate(() => {
    const visible = (e: Element) => (e as HTMLElement).offsetParent !== null;
    const sel = 'button,a[href],input,textarea,select,[role=tab]';
    const main = document.querySelector('main');
    const terms = ['fixture', 'Mock', 'ACP', 'ACL', 'O-Agent', '夹具', '注入', '重置', '未启用', '页签'];
    const text = document.body.innerText;
    return {
      home_controls: [...document.querySelectorAll(sel)].filter(visible).length,
      home_main_controls: main ? [...main.querySelectorAll(sel)].filter(visible).length : 0,
      home_tech_terms: terms.reduce((n, t) => n + (text.split(t).length - 1), 0),
    };
  });
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
  await expect(workspaceInput).toBeEditable(); // UX-01-I1：专注模式下对话输入仍可用
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');
  await page.getByRole('button', { name: '分屏 S2' }).click(); p.step();
  await expect(workspaceInput).toHaveValue('请对比上周同批次的首件检验记录');

  // UX-01-I2（CH-U2）：打开设置再关闭，回到原处，草稿不丢
  await page.getByRole('button', { name: '设置与管理' }).click(); p.step();
  await page.keyboard.press('Escape'); p.step();
  await expect(workspaceInput).toBeEditable(); // UX-01-I1
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
  await info.attach('ux01-probe.json', { body: JSON.stringify({ journey: 'UX-01', ...metrics, ...homeClutter }, null, 2), contentType: 'application/json' });
});

// 体验地图 UX-02（docs/experience-map/journeys/UX-02.md）：做事中途临时提问，不被带离当前工作。
// 只断言当前成立的约束 UX-02-I1；草稿保留（I2）、得到回答（I3）、助理入口可对话（I4）为欠账。
test('E2E-UX02: 任务中切到独立话题临时提问再切回，不离开工作区且任务与作用域恢复', async ({ page }) => {
  const p = probe(page);
  await page.goto('/');
  await page.getByTestId('card-smt-03').click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  const input = page.getByRole('textbox', { name: '工作区输入' });

  await page.getByRole('button', { name: '独立话题' }).click(); p.step();
  await expect(page.getByRole('button', { name: '独立话题' })).toHaveAttribute('aria-pressed', 'true');
  await input.fill('8D 报告里的 D4 指什么？'); p.step();
  await page.getByRole('button', { name: '发送', exact: true }).click(); p.step();

  // UX-02-I1：仍在同一任务的工作区，没有被带走
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(input).toBeEditable();
  await page.getByRole('button', { name: '继续当前任务' }).click(); p.step();
  await expect(page.getByRole('button', { name: '继续当前任务' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  expect(p.snapshot().full_page_navigations).toBe(0);
});

// 体验地图 UX-03（docs/experience-map/journeys/UX-03.md）：一句话委派复杂任务，后台运行时继续做别的事。
// 只断言当前成立的约束 UX-03-I1、I2、I3；从卡片看到真实结果（I4）、完成时主动告知（I5）、
// 不显示内部编号（I6）为欠账，见 2026-11-25 UX-03 基线审查。
test('E2E-UX03: 首页一句话委派后留在桌面，后台运行时可去做别的任务，完成后在“我的工作”看到已完成', async ({ page }, info) => {
  test.setTimeout(120_000);
  const p = probe(page);
  // 用唯一的目标文字区分本次委派（服务端任务不随夹具重置清空，EX-02）
  const goal = `汇总本周 Line-A 质量异常并起草 8D 初稿 ${Date.now()}`;
  await page.goto('/');
  const s1 = page.getByRole('textbox', { name: 'S1 输入' });

  // UX-03-I1（CH-U2、CH-U1）：委派后不离开桌面，受理状态可见，首页输入立即可用
  await s1.fill(goal); p.step();
  await page.getByRole('button', { name: '委派任务' }).click(); p.step();
  await expect(page.getByRole('status').filter({ hasText: '已受理' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(s1).toBeEditable();
  await expect(s1).toHaveValue('');

  // UX-03-I2（CH-U2）：后台运行时可以打开另一个任务做事，再回到桌面
  await page.getByTestId('card-smt-03').click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByRole('textbox', { name: '工作区输入' })).toBeEditable();
  await page.getByRole('button', { name: '返回桌面', exact: true }).click(); p.step();
  await expect(page.getByTestId('desktop-home')).toBeVisible();

  // UX-03-I3（CH-U3）：不用守着，委派的任务出现在“我的工作”并最终显示已完成
  const card = page.getByRole('region', { name: '我的工作' }).getByRole('button').filter({ hasText: goal });
  await expect(card).toContainText('已完成', { timeout: 60_000 });

  const metrics = p.snapshot();
  expect(metrics.full_page_navigations).toBe(0);
  await info.attach('ux03-probe.json', { body: JSON.stringify({ journey: 'UX-03', ...metrics }, null, 2), contentType: 'application/json' });
});
