import { test, expect, type Page } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

// 体验地图 UX-01（docs/experience-map/journeys/UX-01.md）的确定性约束。
// 只断言“当前已经成立”的约束（基线补录）；尚不成立的写在旅程的 known_deviations 里，
// 由产品意图审查跟踪，不在这里写成失败用例。
// 体验探针：统计本旅程的操作步数与整页跳转次数，作为附件输出，只记录趋势，不设门槛。

test.beforeEach(async ({ page }) => {
  await page.request.post('/api/desktop/v1/snapshot/fixture/reset');
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

// UX-01-I7（CH-U2，INTENT-2026-008）：服务端已有其他会话留下的旧草稿时，本会话的新输入
// （尚未同步）离开再返回后不被覆盖，且跨会话冲突有提示。旧草稿与冲突经路由拦截确定性注入，
// 不依赖真实服务端状态（模拟另一会话持续占用，PUT 一律 409）。
test('E2E-UX01-I7: 服务端旧草稿不覆盖本会话新输入，冲突有提示', async ({ page }) => {
  const OTHER = '其他会话的旧草稿：对比上周检验记录';
  const MINE = '本次的新调查内容';
  const draftJson = JSON.stringify({
    draft: { text: OTHER, version: 7, scope: 'continue', continue_task_id: null, updated_at: '' },
  });
  await page.route(`**/api/desktop/v1/drafts/task/${SMT03_TASK_ID}*`, async (route) => {
    if (route.request().method() === 'GET') {
      await new Promise((r) => setTimeout(r, 300));
      await route.fulfill({ status: 200, contentType: 'application/json', body: draftJson });
      return;
    }
    await route.fulfill({ status: 409, contentType: 'application/json', body: draftJson });
  });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  const input = page.getByRole('textbox', { name: '工作区输入' });
  await input.fill(MINE); // 在远端快照返回前输入：快照不可能包含本次输入
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-smt-03').click();
  await expect(input).toHaveValue(MINE, { timeout: 10_000 });
  await expect(page.getByTestId('draft-conflict')).toBeVisible();
  await expect(page.getByTestId('draft-conflict')).toContainText('其他设备');
});

// UX-01-I8（CH-U2，INTENT-2026-011）：工作区已发送的消息与受理记录，离开再返回后仍在原位。
test('E2E-UX01-I8: 发送记录与受理回执离开再返回后仍在', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  const input = page.getByRole('textbox', { name: '工作区输入' });
  const sent = '请调取本周首件检验记录';
  await input.fill(sent);
  await page.getByTestId('workspace-send').click();
  await expect(page.getByTestId('workspace-reply')).toContainText('已受理（模拟）', { timeout: 10_000 });
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-smt-03').click();
  const log = page.getByTestId('workspace-sent-log');
  await expect(log).toBeVisible();
  await expect(log).toContainText(sent);
  await expect(log).toContainText('已受理（模拟）');
});

// UX-01-I5 首页部分（CH-U1，INTENT-2026-001）：首页问答接真实会话，浮层显示助手回答，
// 不离开桌面、首页卡片仍在；会话接口不可达时显示明确不可用说明，不静默回退模拟文本。
test('E2E-UX01-I5: 首页问答得到真实回答且留在桌面；会话接口不可达时明确说明不可用', async ({ page }) => {
  const p = probe(page);
  await page.goto('/');
  const s1 = page.getByRole('textbox', { name: 'S1 输入' });
  await s1.fill(`你好，验证一下首页问答 ${Date.now()}`); p.step();
  await page.getByTestId('s1-ask-send').click(); p.step();
  const reply = page.getByTestId('s1-reply');
  await expect(reply).toBeVisible({ timeout: 60_000 });
  await expect(reply).not.toContainText('问答（模拟'); // 不得静默回退模拟文本
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(page.getByTestId('card-smt-03')).toBeVisible();
  expect(p.snapshot().full_page_navigations).toBe(0);

  // 会话消息接口不可达：明确不可用说明（CH-P3），不出现模拟文本
  await page.route('**/api/conversations/*/messages', (route) => {
    if (route.request().method() === 'POST') return route.abort();
    return route.continue();
  });
  await s1.fill('你好，这条应当提示不可用'); p.step();
  await page.getByTestId('s1-ask-send').click(); p.step();
  await expect(reply).toContainText('不可用', { timeout: 15_000 });
  await expect(reply).not.toContainText('问答（模拟');
});

// 体验地图 UX-02（docs/experience-map/journeys/UX-02.md）：做事中途临时提问，不被带离当前工作。
// 只断言当前成立的约束 UX-02-I1；草稿保留（I2）、得到回答（I3）、助理入口可对话（I4）为欠账。
test('E2E-UX02: 任务中切到独立话题临时提问再切回，不离开工作区且任务与作用域恢复', async ({ page }) => {
  const p = probe(page);
  await page.goto('/');
  await page.getByTestId('card-smt-03').click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  const input = page.getByRole('textbox', { name: '工作区输入' });

  // UX-02-I2（CH-U2）：先写任务草稿，再切独立话题临时提问
  const taskDraft = '调查草稿：对比首件检验记录';
  await input.fill(taskDraft); p.step();
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
  // UX-02-I2：回到任务后原任务草稿仍在（临时提问不得覆盖任务草稿）
  await expect(input).toHaveValue(taskDraft);
  expect(p.snapshot().full_page_navigations).toBe(0);
});

// UX-02-I4（CH-U1、CH-U5，INTENT-2026-012）：左栏“我的助理”打开可直接对话的助理面板
// （顶部对话输入 + 最近消息），“调整助理”进设置；面板不出现内部术语 Soul / Taste；
// 关闭恢复下层界面，工作区任务与草稿不变。
test('E2E-UX02-I4: 我的助理打开可对话面板，关闭后草稿与任务不变且无内部术语', async ({ page }) => {
  const p = probe(page);
  const draft = '调查草稿：对比首件检验记录';
  await page.goto('/');
  await page.getByTestId('card-smt-03').click(); p.step();
  const input = page.getByRole('textbox', { name: '工作区输入' });
  await input.fill(draft); p.step();

  // 打开助理面板：有可编辑的对话输入与最近消息区
  await page.getByTestId('assistant-chip').click(); p.step();
  const panelInput = page.getByTestId('assistant-panel-input');
  await expect(panelInput).toBeVisible();
  await panelInput.fill('帮我把这份调查草稿整理成三个要点'); p.step();
  await expect(panelInput).toBeEditable();
  // 不暴露内部术语
  await expect(page.getByTestId('assistant-panel')).not.toContainText('Soul');
  await expect(page.getByTestId('assistant-panel')).not.toContainText('Taste');

  // “调整助理”进设置中心对应分类（Soul/Taste 的表单在设置里也用用户语言）
  await page.getByTestId('assistant-panel-settings').click(); p.step();
  await expect(page.getByTestId('settings-panel')).toBeVisible();
  await expect(page.getByTestId('settings-panel')).not.toContainText('Soul');
  await expect(page.getByTestId('settings-panel')).not.toContainText('Taste');
  await page.keyboard.press('Escape'); p.step();

  // 关闭面板恢复下层：任务与草稿不变
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(input).toHaveValue(draft);
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

  // UX-03-I4（CH-P8）：从卡片打开委派的真实任务，工作区显示该任务自己的目标、状态与产物，
  // 不出现夹具的模拟内容；工作区状态词与卡片一致
  const cardSubtitle = await card.locator('.desktop-card-sub').innerText();
  expect(cardSubtitle).toBe('已完成');
  await card.click(); p.step();
  const pane = page.getByTestId('workspace-task-pane');
  await expect(page.getByTestId('workspace-task-status')).toHaveText('已完成');
  await expect(page.getByTestId('workspace-task-goal')).toContainText(goal);
  await expect(page.getByTestId('workspace-task-artifacts')).toContainText('verification.md');
  await expect(pane).not.toContainText('质量批次 SMT-03');
  await expect(pane).not.toContainText('桌面首页 Mock');
  await expect(pane).not.toContainText('执行中（模拟）');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();

  const metrics = p.snapshot();
  expect(metrics.full_page_navigations).toBe(0);
  await info.attach('ux03-probe.json', { body: JSON.stringify({ journey: 'UX-03', ...metrics }, null, 2), contentType: 'application/json' });
});

// UX-03-I5（CH-U3，INTENT-2026-015）：任务完成后助理消息告知（普通优先级、带“查看结果”），
// 受理提示同步为完成状态、不再显示“后台处理中”。
test('E2E-UX03-I5: 任务完成后助理消息告知并可查看结果，受理提示同步', async ({ page }) => {
  test.setTimeout(120_000);
  const goal = `汇总本周 Line-A 质量异常并起草 8D 初稿 ${Date.now()}`;
  await page.goto('/');
  const s1 = page.getByRole('textbox', { name: 'S1 输入' });
  await s1.fill(goal);
  await page.getByRole('button', { name: '委派任务' }).click();
  await expect(page.getByRole('status').filter({ hasText: '已受理' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByTestId('s1-bg-status')).toContainText('后台处理中');

  // 去做别的事
  await page.getByTestId('card-smt-03').click();
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();

  // 完成后：受理提示不再“后台处理中”
  const chip = page.getByTestId('s1-bg-status');
  await expect(chip).not.toContainText('后台处理中', { timeout: 90_000 });
  // 助理消息出现该任务的完成通知，带“查看结果”
  const notice = page.getByTestId('task-notice').filter({ hasText: goal });
  await expect(notice).toBeVisible({ timeout: 30_000 });
  await expect(notice).toContainText('已完成');
  // 点“查看结果”直达该任务的工作区（非夹具任务）
  await notice.getByTestId('task-notice-open').click();
  await expect(page.getByTestId('workspace-task-id')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).not.toHaveText(SMT03_TASK_ID);
});

// 体验地图 UX-04（docs/experience-map/journeys/UX-04.md）：在对话里处理任务，查看任务详情时不丢对话现场。
// 只断言当前成立的约束 UX-04-I1；从对话进入工作区 / 专注模式（I2）、离开再返回草稿仍在（I3）、
// 对话页不暴露内部页签与原始工具参数（I4）为欠账，见 2026-11-25 UX-04 基线审查。
test('E2E-UX04: 对话中委派的任务完成后，打开任务详情不影响对话输入与草稿，且不整页跳转', async ({ page }) => {
  test.setTimeout(120_000);
  const p = probe(page);
  // 用唯一文字区分本次会话（服务端会话不随夹具重置清空，EX-02）
  const goal = `写一份关于本地小模型选型的简短报告，保存成 Markdown ${Date.now()}`;
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话' }).click(); p.step();
  const input = page.getByRole('textbox', { name: '输入消息' });
  await input.fill(goal); p.step();
  await input.press('Enter'); p.step();
  await expect(page.getByRole('button', { name: /任务结束：COMPLETED/ })).toBeVisible({ timeout: 60_000 });

  // UX-04-I1（CH-U2、CH-U1）：写了一半的追问，打开右侧任务详情后，对话输入仍可用、草稿不变
  await input.fill('再补一段关于功耗的对比'); p.step();
  const taskEntry = page.getByRole('complementary', { name: '任务与状态' }).getByRole('button').filter({ hasText: goal });
  await taskEntry.click(); p.step();
  await expect(taskEntry).toHaveAttribute('aria-expanded', 'true');
  await expect(input).toBeEditable();
  await expect(input).toHaveValue('再补一段关于功耗的对比');
  expect(p.snapshot().full_page_navigations).toBe(0);
});

// 体验地图 UX-05（docs/experience-map/journeys/UX-05.md）：主动提醒 → 稍后处理 → 回来处理。
// 只断言当前成立的约束 UX-05-I1、I2；“为你关注”实时出现（I3）、“稍后”不丢提醒（I4）、
// 助理消息可直达对应工作（I5）为欠账，见 2026-11-25 UX-05 基线审查。
// 提醒经测试 API 注入（夹具，EX-02），用唯一标题区分。
test('E2E-UX05: 提醒不抢输入焦点；稍后回到桌面可从“为你关注”直达任务，返回后提醒仍在', async ({ page }) => {
  const p = probe(page);
  const title = `建议：复核 SMT-03 证据 ${Date.now()}`;
  // 前置条件：普通建议未被静音。E2E-ADATTN 会打开“静音普通建议”且不恢复，同一服务上先跑它时
  // 本用例注入的普通提醒会被吞掉（测试间状态串扰，不是产品问题）。
  await page.request.post('/api/desktop/v1/attention/mute', { data: { mute_ordinary: false } });
  await page.goto('/');
  const s1 = page.getByRole('textbox', { name: 'S1 输入' });
  await s1.fill('正在写的问题'); p.step();
  await s1.focus();
  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'ordinary', title, body: '首件记录与上周对比缺一处复核', task_id: SMT03_TASK_ID },
  });

  // UX-05-I1（CH-U3、CH-U2）：提醒以浮层出现，不抢正在输入的焦点、不清空输入
  await expect(page.getByTestId('attn-item-ordinary').filter({ hasText: title })).toBeVisible({ timeout: 15_000 });
  await expect(s1).toBeFocused();
  await expect(s1).toHaveValue('正在写的问题');

  // 稍后回来：重新打开桌面（当前“为你关注”不随新提醒实时更新，见 INTENT-2026-023 / UX-05-I3 欠账）
  await page.reload();
  const navBefore = p.snapshot().full_page_navigations;

  // UX-05-I2（CH-U3、CH-U2）：从“为你关注”直达关联任务；处理完返回桌面，提醒仍在原处
  const watch = page.getByRole('region', { name: '为你关注' }).getByRole('button').filter({ hasText: title });
  await expect(watch).toBeVisible();
  await watch.click(); p.step();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await page.getByRole('button', { name: '返回桌面', exact: true }).click(); p.step();
  await expect(watch).toBeVisible();
  // 处理过程中不发生整页跳转（只计 reload 之后）
  expect(p.snapshot().full_page_navigations - navBefore).toBe(0);
});

// 体验地图 UX-06（docs/experience-map/journeys/UX-06.md）：从工作中直达对应设置，改完回到原处。
// 只断言当前成立的约束 UX-06-I1～I4；“一次返回即回原工作”（I5）、从提醒直达对应设置（I6）为欠账，
// 见 2026-11-25 UX-06 基线审查。改过的助理名称在用例末尾恢复，避免影响其他用例。
test('E2E-UX06: 工作区“调整助理”直达助理设置，改名生效，记忆子层返回原分类，关闭后回到原工作且草稿不变', async ({ page }) => {
  const p = probe(page);
  const draft = '调查草稿：对比首件记录';
  const newName = `助理${Date.now() % 100000}`;
  await page.goto('/');
  await page.getByTestId('card-smt-03').click(); p.step();
  const input = page.getByRole('textbox', { name: '工作区输入' });
  await input.fill(draft); p.step();

  // UX-06-I1（CH-U4）：场景内捷径直达对应分类，而不是设置首页
  await page.getByRole('button', { name: '调整助理' }).click(); p.step();
  await expect(page.getByTestId('settings-crumb')).toContainText('我的 AI 助理');
  const nameBox = page.getByTestId('profile-name');
  const original = await nameBox.inputValue();

  // UX-06-I2（CH-U4）：保存后改动在桌面各处生效
  await nameBox.fill(newName); p.step();
  await page.getByTestId('profile-save').click(); p.step();

  // UX-06-I4（CH-U2）：从分类进入子层（记忆），返回回到原分类
  await page.getByRole('button', { name: '查看记忆' }).click(); p.step();
  await page.getByTestId('memory-back').click(); p.step();
  await expect(page.getByTestId('settings-crumb')).toContainText('我的 AI 助理');

  // UX-06-I3（CH-U2）：关闭设置回到原工作区，任务与草稿不变（当前需要两次返回，见 I5 欠账）
  await page.keyboard.press('Escape'); p.step();
  await page.keyboard.press('Escape'); p.step();
  await expect(page.getByTestId('settings-panel')).toHaveCount(0);
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(input).toHaveValue(draft);
  await page.getByRole('button', { name: '返回桌面', exact: true }).click(); p.step();
  await expect(page.getByTestId('home-assistant-name')).toContainText(newName);
  expect(p.snapshot().full_page_navigations).toBe(0);

  // 清理：恢复原名称
  await page.getByTestId('card-smt-03').click();
  await page.getByRole('button', { name: '调整助理' }).click();
  await page.getByTestId('profile-name').fill(original);
  await page.getByTestId('profile-save').click();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('home-assistant-name')).toContainText(original);
});

// UX-01-I6（CH-U1，INTENT-2026-005）：窄屏（390 宽）下工作区的对话输入在首屏可见——
// 窄屏默认单栏（任务信息折叠），S2 双栏挤压不再把输入挤出首屏。
test('E2E-UX01-I6: 窄屏下工作区对话输入在首屏可见', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  const box = await page.getByTestId('workspace-input').boundingBox();
  expect(box).not.toBeNull();
  // 首屏内完整可见：纵向不被挤出视口，横向可用宽度足够（窄屏单栏，不被任务栏挤压）
  expect(box!.y, '输入框应在首屏内').toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height, '输入框不应被挤出首屏').toBeLessThanOrEqual(844);
  expect(box!.width, '输入框可用宽度不应被 S2 双栏挤压').toBeGreaterThanOrEqual(200);
  // 任务信息栏不与会话栏并排挤压（窄屏单栏：任务栏在输入框上方）
  const paneBox = await page.getByTestId('workspace-task-pane').boundingBox();
  if (paneBox) {
    expect(paneBox.y + paneBox.height, '任务信息栏应在输入框上方（窄屏单栏）').toBeLessThanOrEqual(box!.y + 1);
  }
});

// UX-03-I7（CH-U1，INTENT-2026-016）：输入为空时不代用户委派默认任务——
// “委派任务”不可用，不产生任何任务；填写后恢复可用。
test('E2E-UX03-I7: 输入为空时委派不可用且不产生默认任务', async ({ page }) => {
  await page.goto('/');
  const s1 = page.getByRole('textbox', { name: 'S1 输入' });
  const btn = page.getByRole('button', { name: '委派任务' });
  await expect(btn).toBeDisabled();
  await expect(btn).toHaveAttribute('title', '先说说要做什么');
  await btn.click({ force: true }).catch(() => {});
  await page.waitForTimeout(300);
  await expect(page.getByTestId('s1-bg-chip')).toHaveCount(0);
  await expect(page.getByRole('status').filter({ hasText: '已受理' })).toHaveCount(0);
  // 填写后恢复可用
  await s1.fill('检查 SMT-03 首件记录');
  await expect(btn).toBeEnabled();
});

// UXFIX-F（#309，INTENT-2026-010，CH-U4）：设置中心不出现“（占位）”；
// 未实现的分组显示“即将提供”且不可进入（已在 MOCK_REGISTRY 登记）。
test('E2E-ADSETGROUPS: 设置中心无占位字样，未实现分组显示即将提供且不可进入', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('open-settings').click();
  const panel = page.getByTestId('settings-panel');
  await expect(panel).toBeVisible();
  await expect(panel).not.toContainText('（占位）');
  await expect(panel).toContainText('即将提供');
  for (const id of ['desktop', 'notify', 'device', 'privacy']) {
    await expect(page.getByTestId(`settings-cat-${id}`)).toBeDisabled();
  }
  // 可用分组（我的 AI 助理 / 连接与能力）不受影响
  await expect(page.getByTestId('settings-cat-assistant')).toBeEnabled();
  await expect(page.getByTestId('settings-cat-connect')).toBeEnabled();
});
