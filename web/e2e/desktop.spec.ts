import { test, expect } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

// T-82：首页卡片来自真实快照；每个用例先播种 fixture（SMT-03/产品改进/关注卡），
// 断言本身与 A 阶段一致（不放松）。
test.beforeEach(async ({ page }) => {
  await page.request.post('/api/desktop/v1/snapshot/fixture/reset');
});

test.afterEach(async ({ page }, info) => {
  await page.screenshot({ path: info.outputPath('page.png'), fullPage: true });
});

test('E2E-ADNAV: 桌面首页多入口打开同一 SMT-03 并返回', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(page.getByRole('heading', { name: '我的 AI 桌面' })).toBeVisible();
  await expect(page.getByRole('region', { name: '我的工作' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Agent 工作' })).toBeVisible();
  await expect(page.getByRole('region', { name: '为你关注' })).toBeVisible();
  await expect(page.getByTestId('ops-entry-workbench')).toBeDisabled();
  await expect(page.getByRole('tab', { name: /运营桌面/ })).toHaveCount(0);

  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);

  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('desktop-home')).toBeVisible();

  await page.getByRole('button', { name: /SMT-03|TASK-2026-018/ }).first().click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADLAYOUT: S2与S3切换后刷新仍恢复布局', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');
  await expect(page.getByTestId('workspace-task-pane')).toBeVisible();

  await page.getByRole('button', { name: '专注 S3', exact: true }).click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's3');
  await expect(page.getByTestId('workspace-task-pane')).toHaveCount(0);

  await page.getByRole('button', { name: '分屏 S2', exact: true }).click();
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');

  await page.reload();
  await expect(page.getByTestId('workspace')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-layout', 's2');
});

test('E2E-ADDRAFT: 两任务草稿隔离且延迟发送不覆盖追加输入', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('草稿甲-SMT');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-product').click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText('TASK-2026-021');
  await page.getByTestId('workspace-input').fill('草稿乙-产品');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('草稿甲-SMT');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-product').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('草稿乙-产品');

  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('待发送正文');
  await page.getByTestId('workspace-send').click();
  await expect(page.getByTestId('workspace-reply')).toContainText('发送中');
  await page.getByTestId('workspace-input').fill('追加草稿保留');
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-product').click();
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('an.desktop.late.v1:TASK-2026-018')), { timeout: 5_000 })
    .toBeTruthy();
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await page.getByTestId('card-smt-03').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('追加草稿保留');
  await expect(page.getByTestId('workspace-late').first()).toContainText('迟到回执');
});

test('E2E-ADSCOPE: 独立话题与继续任务及关联纠正', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByRole('button', { name: '独立话题', exact: true }).click();
  await expect(page.getByTestId('scope-reason')).toContainText('独立话题');
  await expect(page.getByTestId('scope-task-id')).toHaveCount(0);

  await page.getByRole('button', { name: '继续当前任务', exact: true }).click();
  await expect(page.getByTestId('scope-task-id')).toContainText(SMT03_TASK_ID);

  await page.getByRole('button', { name: '纠正关联', exact: true }).click();
  await expect(page.getByTestId('scope-reason')).toContainText('独立话题');
  await expect(page.getByTestId('scope-audit')).toContainText('撤回关联');

  await page.getByRole('button', { name: '转为任务', exact: true }).click();
  await expect(page.getByTestId('scope-task-id')).toContainText(SMT03_TASK_ID);
  await expect(page.getByTestId('scope-audit')).toContainText('建立关联');
});

test('E2E-ADEVID: 首页查看证据并逐层返回含失败过期与空夹具', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.setItem('an.desktop.principal.v1', 'alice'));
  await page.getByTestId('home-view-evidence').click();
  await expect(page.getByTestId('workspace')).toBeVisible();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('evidence-panel')).toBeVisible();

  await page.getByTestId('evidence-item-ev-smt-qc-1').click();
  await expect(page.getByTestId('evidence-preview')).toBeVisible();
  await expect(page.getByTestId('evidence-object')).toContainText('质量批次 SMT-03');
  await expect(page.getByTestId('evidence-time')).toBeVisible();
  await expect(page.getByTestId('evidence-version')).toHaveText('v3');
  await expect(page.getByTestId('evidence-task')).toHaveText(SMT03_TASK_ID);
  await page.getByTestId('evidence-open-source').click();
  await expect(page.getByTestId('evidence-source')).toContainText('LOT=SMT-03-Q');
  await page.getByTestId('evidence-back').click();
  await expect(page.getByTestId('evidence-preview')).toBeVisible();
  await page.getByTestId('evidence-back').click();
  await expect(page.getByTestId('evidence-list')).toBeVisible();

  await page.getByTestId('evidence-item-ev-smt-fail').click();
  await expect(page.getByTestId('evidence-error')).toContainText('不可用');
  await page.getByTestId('evidence-back').click();
  await page.getByTestId('evidence-item-ev-smt-exp').click();
  await expect(page.getByTestId('evidence-expired')).toContainText('过期');
  await page.getByTestId('evidence-back').click();

  await page.getByTestId('evidence-demo-empty').click();
  await expect(page.getByTestId('evidence-empty')).toContainText('暂无证据');
  await page.getByTestId('evidence-back').click();
  await expect(page.getByTestId('evidence-panel')).toHaveCount(0);
  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('desktop-home')).toBeVisible();
});

test('E2E-ADSOURCES: 两角色证据 ACL 撤权后搜索与原文一致拒绝', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.setItem('an.desktop.principal.v1', 'alice'));
  await page.getByTestId('home-view-evidence').click();
  await expect(page.getByTestId('evidence-panel')).toBeVisible();
  await expect(page.getByTestId('evidence-mode-label')).toContainText('真实 ACL');

  await page.getByTestId('evidence-item-ev-smt-qc-1').click();
  await expect(page.getByTestId('evidence-summary')).toContainText('抽检');
  await page.getByTestId('evidence-open-source').click();
  await expect(page.getByTestId('evidence-source')).toContainText('LOT=SMT-03-Q');
  await page.getByTestId('evidence-back').click();

  await page.getByTestId('evidence-principal-bob').click();
  await expect(page.getByTestId('evidence-item-ev-smt-qc-1')).toHaveCount(0);
  await page.getByTestId('evidence-search-acl').click();
  await expect(page.getByTestId('evidence-search-hits')).toHaveText('none');

  await page.getByTestId('evidence-principal-alice').click();
  await expect(page.getByTestId('evidence-item-ev-smt-qc-1')).toBeVisible();
  await page.getByTestId('evidence-item-ev-smt-qc-1').click();
  await page.getByTestId('evidence-revoke').click();
  await expect(page.getByTestId('evidence-denied')).toContainText('无权');
  await page.getByTestId('evidence-back').click();
  await expect(page.getByTestId('evidence-item-ev-smt-qc-1')).toHaveCount(0);
  await page.getByTestId('evidence-search-acl').click();
  await expect(page.getByTestId('evidence-search-hits')).toHaveText('none');

  await page.getByTestId('evidence-principal-carol').click();
  await expect(page.getByTestId('evidence-item-ev-smt-qc-1')).toBeVisible();
  await page.getByTestId('evidence-item-ev-smt-qc-1').click();
  await page.getByTestId('evidence-open-source').click();
  await expect(page.getByTestId('evidence-source')).toContainText('LOT=SMT-03-Q');
});

test('E2E-ADHISTORY: 历史筛选固定与打开任务后恢复筛选', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('open-history').click();
  await expect(page.getByTestId('history-panel')).toBeVisible();
  await expect(page.getByTestId('history-reason-hist-smt')).toContainText('推荐');

  await page.getByTestId('history-filter').fill('产品改进');
  await expect(page.getByTestId('history-item-hist-prod')).toBeVisible();
  await expect(page.getByTestId('history-item-hist-smt')).toHaveCount(0);

  await page.getByTestId('history-filter').fill('');
  const smt = page.getByTestId('history-item-hist-smt');
  await smt.getByRole('button', { name: '固定', exact: true }).click();
  await expect(smt.getByRole('button', { name: '固定' })).toHaveAttribute('aria-pressed', 'true');

  await page.getByTestId('history-filter').fill('SMT');
  await smt.getByRole('button', { name: /SMT-03/ }).click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('history-panel')).toHaveCount(0);

  await page.getByTestId('open-history').click();
  await expect(page.getByTestId('history-filter')).toHaveValue('SMT');
  await expect(page.getByTestId('history-item-hist-smt')).toBeVisible();
});

test('E2E-ADPROF: 助理设置保存、记忆返回与冲突不覆盖草稿', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('设置中应保留的草稿');
  await page.getByTestId('workspace-open-settings').click();
  await expect(page.getByTestId('settings-panel')).toBeVisible();
  await expect(page.getByTestId('settings-assistant')).toBeVisible();

  await page.getByTestId('profile-name').fill('桌面测试助理');
  await page.getByTestId('profile-save').click();
  await expect(page.getByTestId('profile-saved')).toBeVisible();
  await expect(page.getByTestId('assistant-chip')).toContainText('桌面测试助理');

  await page.getByTestId('profile-open-memory').click();
  await expect(page.getByTestId('settings-memory')).toBeVisible();
  await expect(page.getByTestId('memory-crumb')).toContainText('记忆');
  await page.getByTestId('memory-back').click();
  await expect(page.getByTestId('settings-assistant')).toBeVisible();
  await expect(page.getByTestId('settings-crumb')).toContainText('我的 AI 助理');

  await page.getByTestId('profile-name').fill('冲突中的名称');
  await page.getByTestId('profile-inject-conflict').click();
  await page.getByTestId('profile-save').click();
  await expect(page.getByTestId('profile-save-error')).toContainText('版本冲突');
  await expect(page.getByTestId('profile-name')).toHaveValue('冲突中的名称');

  // UXFIX-O（#327）：捷径进入设置后一次返回即关闭设置回工作区
  await page.getByTestId('settings-back').click();
  await expect(page.getByTestId('settings-panel')).toHaveCount(0);
  await expect(page.getByTestId('workspace-input')).toHaveValue('设置中应保留的草稿');

  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('home-assistant-name')).toContainText('桌面测试助理');
});

test('E2E-ADMEM: 记忆列表来自服务并可打开详情', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('记忆页应保留的草稿');
  await page.getByTestId('workspace-open-settings').click();
  await expect(page.getByTestId('settings-assistant')).toBeVisible();

  await page.getByTestId('profile-open-memory').click();
  await expect(page.getByTestId('settings-memory')).toBeVisible();
  await expect(page.getByTestId('memory-list')).toBeVisible();
  await expect(page.getByTestId('memory-item-mem-1')).toContainText('质量报告偏好摘要格式');

  await page.getByTestId('memory-item-mem-1').click();
  await expect(page.getByTestId('memory-detail')).toBeVisible();
  await expect(page.getByTestId('memory-body')).toContainText('先给结论');
  await expect(page.getByTestId('memory-source')).toContainText('用户保存');
  await expect(page.getByTestId('memory-expires')).toContainText('2027');

  await page.getByTestId('memory-detail-back').click();
  await expect(page.getByTestId('memory-list')).toBeVisible();
  await page.getByTestId('memory-back').click();
  await expect(page.getByTestId('settings-assistant')).toBeVisible();

  // UXFIX-O（#327）：捷径进入设置后一次返回即回工作区
  await page.getByTestId('settings-back').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('记忆页应保留的草稿');
});

test('E2E-ADMEMW: 记忆纠错与撤回', async ({ page, request }) => {
  const reset = await request.post('/api/desktop/v1/memories/fixture/reset', { data: {} });
  expect(reset.ok()).toBeTruthy();

  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('纠错时应保留的草稿');
  await page.getByTestId('workspace-open-settings').click();
  await page.getByTestId('profile-open-memory').click();
  await expect(page.getByTestId('memory-list')).toBeVisible();

  await page.getByTestId('memory-item-mem-1').click();
  await expect(page.getByTestId('memory-detail')).toBeVisible();
  await page.getByTestId('memory-body-input').fill('纠错后的摘要格式说明');
  await page.getByTestId('memory-correct-save').click();
  await expect(page.getByTestId('memory-correct-saved')).toBeVisible();
  await expect(page.getByTestId('memory-body')).toContainText('纠错后的摘要格式说明');

  await page.getByTestId('memory-detail-back').click();
  await expect(page.getByTestId('memory-item-mem-1')).toBeVisible();
  // #354 夹具口径：种子只剩 mem-1，撤回流程改用 mem-1 验证
  await page.getByTestId('memory-item-mem-1').click();
  // UXFIX-T（#353）：撤回需确认
  await page.getByTestId('memory-withdraw').click();
  await expect(page.getByTestId('memory-withdraw-confirm')).toBeVisible();
  await page.getByTestId('memory-withdraw-confirm-yes').click();
  await expect(page.getByTestId('memory-list')).toBeVisible();
  await expect(page.getByTestId('memory-item-mem-1')).toHaveCount(0);

  await page.getByTestId('memory-back').click();
  // UXFIX-O（#327）：捷径进入设置后一次返回即回工作区
  await page.getByTestId('settings-back').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('纠错时应保留的草稿');
});

// UX-08-I6（CH-G2，INTENT-2026-031 客户端部分）：撤回需确认；详情显示类型与确认状态（中文）；
// 有效期可编辑（详情与列表显示新值）。撤销恢复与已撤回记录需服务端支持，另出方案（见 #353 评论）。
test('E2E-UX08-I6: 撤回需确认；详情显示类型与确认状态；有效期可编辑', async ({ page, request }) => {
  const reset = await request.post('/api/desktop/v1/memories/fixture/reset', { data: {} });
  expect(reset.ok()).toBeTruthy();

  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-input').fill('有效期测试草稿');
  await page.getByTestId('workspace-open-settings').click();
  await page.getByTestId('profile-open-memory').click();
  await page.getByTestId('memory-item-mem-1').click();
  await expect(page.getByTestId('memory-detail')).toBeVisible();

  // 类型与确认状态（中文）。夹具口径（#354）：种子 confirmed_at=None → 待确认，
  // source「用户保存」不虚报「已确认」
  await expect(page.getByTestId('memory-kind')).toContainText('事实');
  await expect(page.getByTestId('memory-confirm')).toContainText('待确认');

  // 有效期可编辑（详情与列表都显示新值）
  await page.getByTestId('memory-expires-input').fill('2028-01-01');
  await page.getByTestId('memory-expires-save').click();
  await expect(page.getByTestId('memory-expires')).toContainText('2028-01-01');
  await page.getByTestId('memory-detail-back').click();
  await expect(page.getByTestId('memory-list')).toContainText('2028-01-01');
  await page.getByTestId('memory-item-mem-1').click();

  // 撤回需确认：第一次点击不撤回
  await page.getByTestId('memory-withdraw').click();
  await expect(page.getByTestId('memory-withdraw-confirm')).toBeVisible();
  await expect(page.getByTestId('memory-withdraw-confirm')).toContainText('不再参与下一次回答');
  await expect(page.getByTestId('memory-list')).not.toBeVisible();

  // 确认后撤回：回到列表且条目消失
  await page.getByTestId('memory-withdraw-confirm-yes').click();
  await expect(page.getByTestId('memory-list')).toBeVisible();
  await expect(page.getByTestId('memory-item-mem-1')).toHaveCount(0);
  // 子层返回不变（memory→分类→关闭回工作区），草稿不变
  await page.getByTestId('memory-back').click();
  await page.getByTestId('settings-back').click();
  await expect(page.getByTestId('workspace-input')).toHaveValue('有效期测试草稿');
});

test('E2E-ADXR: 已读跨端服务权威与幂等', async ({ page, browser, request }) => {
  const reset = await request.post('/api/desktop/v1/assistant/messages/fixture/reset', {
    data: {},
  });
  expect(reset.ok()).toBeTruthy();

  await page.goto('/');
  await expect(page.getByTestId('asst-read-root')).toBeVisible();
  await expect(page.getByTestId('asst-read-unread')).toContainText(/未读 [2-9]/);
  await expect(page.getByTestId('asst-read-item-asst-msg-1')).toHaveAttribute('data-read', '0');

  await page.getByTestId('asst-read-mark-asst-msg-1').click();
  await expect(page.getByTestId('asst-read-item-asst-msg-1')).toHaveAttribute('data-read', '1');
  await expect(page.getByTestId('asst-read-flag-asst-msg-1')).toHaveText('已读');
  const unreadAfterA = await page.getByTestId('asst-read-unread').innerText();
  expect(unreadAfterA).toMatch(/未读 [1-9]/);
  expect(unreadAfterA).not.toMatch(/未读 0$/);

  const ctxB = await browser.newContext();
  const pageB = await ctxB.newPage();
  await pageB.goto('/');
  await expect(pageB.getByTestId('asst-read-root')).toBeVisible();
  await expect(pageB.getByTestId('asst-read-item-asst-msg-1')).toHaveAttribute('data-read', '1');
  await expect(pageB.getByTestId('asst-read-unread')).toHaveText(unreadAfterA);

  for (const id of ['asst-msg-2', 'asst-msg-3'] as const) {
    const mark = pageB.getByTestId(`asst-read-mark-${id}`);
    if (await mark.count()) {
      await mark.click();
      await expect(pageB.getByTestId(`asst-read-item-${id}`)).toHaveAttribute('data-read', '1');
    }
  }
  await expect(pageB.getByTestId('asst-read-unread')).toHaveText('未读 0');

  // T-88：刷新按钮收编，面板 5s 轮询自动对齐
  await expect(page.getByTestId('asst-read-unread')).toHaveText('未读 0', { timeout: 10_000 });
  await expect(page.getByTestId('asst-read-item-asst-msg-1')).toHaveAttribute('data-read', '1');

  const again = await request.post('/api/desktop/v1/assistant/messages/read', {
    data: { message_ids: ['asst-msg-1', 'asst-msg-2', 'asst-msg-3'] },
  });
  expect(again.ok()).toBeTruthy();
  const body = await again.json();
  expect(body.newly_marked).toBe(0);

  await expect(page.getByTestId('asst-read-unread')).toHaveText('未读 0', { timeout: 10_000 });
  await ctxB.close();
});

test('E2E-ADINPUT: 响应式设置入口、Escape、IME 与减少动态效果', async ({ page }) => {
  const widths = [360, 768, 1024, 1440] as const;
  for (const w of widths) {
    await page.setViewportSize({ width: w, height: 800 });
    await page.goto('/');
    const settingsBtn = page.getByTestId('open-settings');
    if (!(await settingsBtn.isVisible())) {
      await page.getByRole('button', { name: /会话/ }).first().click();
    }
    await expect(settingsBtn).toBeVisible();
  }

  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByTestId('open-settings').click();
  await expect(page.getByTestId('settings-panel')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('settings-panel')).toHaveCount(0);

  await page.getByTestId('card-smt-03').click();
  const input = page.getByTestId('workspace-input');
  await input.fill('选词中勿发送');
  await input.focus();
  await input.evaluate((el) => {
    el.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
  });
  await expect(input).toHaveAttribute('data-composing', '1');
  await input.evaluate((el) => {
    const ev = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(ev, 'isComposing', { get: () => true });
    el.dispatchEvent(ev);
  });
  await expect(page.getByTestId('workspace-reply')).toHaveCount(0);
  await expect(page.getByTestId('workspace-send')).toHaveText('发送');
  await input.evaluate((el) => {
    el.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
  });
  await expect(input).toHaveAttribute('data-composing', '0');

  const posts: string[] = [];
  page.on('request', (req) => {
    if (req.method() === 'POST' && req.url().includes('/messages')) posts.push(req.url());
  });
  await page.getByRole('button', { name: '＋ 新会话' }).click();
  const chatInput = page.getByRole('textbox').last();
  await chatInput.fill('ime');
  await chatInput.evaluate((el) => {
    el.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
  });
  await chatInput.evaluate((el) => {
    const ev = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(ev, 'isComposing', { get: () => true });
    el.dispatchEvent(ev);
  });
  await expect.poll(() => posts.length).toBe(0);
  await chatInput.evaluate((el) => {
    el.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
  });

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('data-reduced-motion', '1');
});

test('E2E-ADASK: S1 问答与委派不强制导航', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/');
  await expect(page.getByTestId('s1-ask-bar')).toBeVisible();
  // UXFIX-A（#304）：问答接真实会话，回答来自模型（不出现“问答（模拟…）”回显）。
  // 问句以“你好”开头走受控模型的 respond 路径，保证用例确定性。
  await page.getByTestId('s1-ask-input').fill('你好，SMT-03 概况如何');
  await page.getByTestId('s1-ask-send').click();
  await expect(page.getByTestId('s1-reply')).toBeVisible();
  await expect(page.getByTestId('s1-reply')).not.toContainText('问答（模拟');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(page.getByTestId('workspace')).toHaveCount(0);

  await page.getByTestId('s1-ask-input').fill('深入调查并整理证据链');
  await page.getByTestId('s1-delegate').click();
  await expect(page.getByTestId('s1-bg-chip')).toBeVisible();
  await expect(page.getByTestId('s1-bg-status')).toContainText(/提交中|已受理/);
  await expect
    .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
    .toContain('已受理');
  const taskId = (await page.getByTestId('s1-bg-task').innerText()).trim();
  expect(taskId).toMatch(/^task-/);
  await expect(page.getByTestId('desktop-home')).toBeVisible();

  await page.getByTestId('s1-ask-input').fill('你好，后台跑着时继续问');
  await page.getByTestId('s1-ask-send').click();
  await expect(page.getByTestId('s1-reply')).toBeVisible();
  await expect(page.getByTestId('s1-reply')).not.toContainText('问答（模拟');
  await expect(page.getByTestId('s1-bg-chip')).toBeVisible();

  await page.getByTestId('s1-bg-open').click();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
  await expect(page.locator('.task-card').filter({ hasText: '深入调查并整理证据链' })).toBeVisible();
});

test('E2E-HOMEACCEPT: 首页委派丢回执回读与重连原会话', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/');
  const goal = '首页可靠受理验证：整理 SMT 调查提纲';
  await page.getByTestId('s1-ask-input').fill(goal);
  await page.getByTestId('s1-delegate').click();
  await expect
    .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
    .toContain('已受理');
  const taskId = (await page.getByTestId('s1-bg-task').innerText()).trim();
  const convId = (await page.getByTestId('s1-bg-conv').innerText()).trim();
  expect(taskId).toMatch(/^task-/);
  expect(convId.length).toBeGreaterThan(3);

  // 同会话可查任务
  const listed = await (await page.request.get(`/api/tasks?conversation_id=${convId}`)).json();
  expect(listed.some((t: { id: string }) => t.id === taskId)).toBeTruthy();

  await page.getByTestId('s1-bg-open').click();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
  await expect(page.locator('.task-card').filter({ hasText: goal })).toBeVisible();

  // 重连：刷新后仍能回到原会话（标题来自委派正文前缀）
  await page.reload();
  await expect(page.getByRole('button', { name: /首页可靠受理验证|整理 SMT/ }).first()).toBeVisible({
    timeout: 15_000,
  });
  await page.getByRole('button', { name: /首页可靠受理验证|整理 SMT/ }).first().click();
  await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
  await expect(page.locator('.task-card').filter({ hasText: goal })).toBeVisible();
});

test('E2E-HOMEREV: 首页委派审查待验收后判定通过', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('/');
  const goal = '人工验收验证报告：从桌面首页委派';
  await page.getByTestId('s1-ask-input').fill(goal);
  await page.getByTestId('s1-delegate').click();
  await expect
    .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
    .toContain('已受理');
  const taskId = (await page.getByTestId('s1-bg-task').innerText()).trim();
  const convId = (await page.getByTestId('s1-bg-conv').innerText()).trim();
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('NEEDS_HUMAN');
  const before = (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task;
  expect(before.result).not.toBeNull();
  expect(before.conversation_id).toBe(convId);

  await page.getByTestId('s1-bg-open').click();
  await expect(page.locator('main')).toContainText('待验收');
  await page.locator('main').getByRole('button', { name: '判定通过', exact: true }).click();
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('COMPLETED');
  await expect(page.locator('main')).toContainText(/已完成|任务已完成/);

  // Composer 交付 durable 写入发起会话（绑定 task_id）
  await expect
    .poll(
      async () => {
        const msgs = await (await page.request.get(`/api/conversations/${convId}/messages`)).json();
        return msgs.some(
          (m: { role: string; task_id: string | null; content: string }) =>
            m.role === 'assistant' && m.task_id === taskId && m.content.trim().length > 0,
        );
      },
      { timeout: 60_000 },
    )
    .toBeTruthy();
  const done = (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task;
  const hasArtifact = (done.result?.artifacts?.length ?? 0) > 0;
  const hasEvidence = (done.result?.evidence?.length ?? 0) > 0;
  expect(hasArtifact || hasEvidence).toBeTruthy();
  await expect(page.locator('main .msg.assistant').last()).toBeVisible();

  // 重连后仍可断言交付与证据
  await page.reload();
  await expect(page.getByRole('button', { name: /人工验收验证报告|从桌面首页/ }).first()).toBeVisible({
    timeout: 15_000,
  });
  await page.getByRole('button', { name: /人工验收验证报告|从桌面首页/ }).first().click();
  await expect(page.locator('.task-card').filter({ hasText: goal })).toContainText(/已完成|COMPLETED|完成/);
  const afterReload = await (await page.request.get(`/api/conversations/${convId}/messages`)).json();
  expect(
    afterReload.some(
      (m: { role: string; task_id: string | null }) => m.role === 'assistant' && m.task_id === taskId,
    ),
  ).toBeTruthy();
});

test('E2E-HOMEREJ: 首页委派判定不通过返工后再失败交付', async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto('/');
  const goal = '人工验收失败路径：从桌面首页委派退回';
  await page.getByTestId('s1-ask-input').fill(goal);
  await page.getByTestId('s1-delegate').click();
  await expect
    .poll(async () => page.getByTestId('s1-bg-status').innerText(), { timeout: 60_000 })
    .toContain('已受理');
  const taskId = (await page.getByTestId('s1-bg-task').innerText()).trim();
  const convId = (await page.getByTestId('s1-bg-conv').innerText()).trim();
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('NEEDS_HUMAN');

  await page.getByTestId('s1-bg-open').click();
  await expect(page.locator('main')).toContainText('待验收');
  const know = page.getByRole('button', { name: '知道了', exact: true });
  if (await know.count()) await know.click();

  // ConfirmButton：先武装再确认（限定对话区 task-actions，避免歧义）
  const actions = page.locator('main .task-actions');
  const rejectOnce = async () => {
    await actions.getByRole('button', { name: '判定不通过', exact: true }).click();
    await actions.getByRole('button', { name: '确认退回', exact: true }).click();
  };
  await rejectOnce();

  // 首次 reject → 返工（受控模型可能极快回到待验收；快轮询捕获中间态，或看事件）
  let sawReworkPhase = false;
  const reworkDeadline = Date.now() + 60_000;
  while (Date.now() < reworkDeadline) {
    const detail = await (await page.request.get(`/api/tasks/${taskId}`)).json();
    expect(detail.task.conversation_id).toBe(convId);
    if (['QUEUED', 'RUNNING', 'VERIFYING'].includes(detail.task.status)) {
      sawReworkPhase = true;
      break;
    }
    const reworkEvt = (detail.events ?? []).some(
      (e: { message?: string }) =>
        typeof e.message === 'string' &&
        (e.message.includes('返工') || e.message.includes('人工验收未通过')),
    );
    if (reworkEvt && detail.task.status === 'NEEDS_HUMAN') {
      sawReworkPhase = true;
      break;
    }
    await page.waitForTimeout(50);
  }
  expect(sawReworkPhase).toBeTruthy();

  // 返工后再待验收（若已在待验收则立刻满足）
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task.status, {
      timeout: 90_000,
    })
    .toBe('NEEDS_HUMAN');
  await expect(page.locator('main')).toContainText('待验收');

  await rejectOnce();
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${taskId}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('FAILED');

  await expect
    .poll(
      async () => {
        const msgs = await (await page.request.get(`/api/conversations/${convId}/messages`)).json();
        return msgs.some(
          (m: { role: string; task_id: string | null; content: string }) =>
            m.role === 'assistant' && m.task_id === taskId && m.content.trim().length > 0,
        );
      },
      { timeout: 60_000 },
    )
    .toBeTruthy();

  await page.reload();
  await expect(page.getByRole('button', { name: /人工验收失败路径|从桌面首页委派退回/ }).first()).toBeVisible({
    timeout: 15_000,
  });
  await page.getByRole('button', { name: /人工验收失败路径|从桌面首页委派退回/ }).first().click();
  await expect(page.locator('.task-card').filter({ hasText: goal })).toContainText(/失败|FAILED/);
  const afterReload = await (await page.request.get(`/api/conversations/${convId}/messages`)).json();
  expect(
    afterReload.some(
      (m: { role: string; task_id: string | null }) => m.role === 'assistant' && m.task_id === taskId,
    ),
  ).toBeTruthy();
});

test('E2E-ADATTN: 输入中普通建议不抢焦点且可静音', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话' }).click();
  const input = page.getByRole('textbox', { name: '输入消息' });
  await input.focus();
  await input.fill('正在写草稿');
  await expect(input).toBeFocused();

  // T-88：注入改为 API（测试控制不再渲染在生产界面）；toast 经轮询出现
  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'ordinary', title: '建议：复核 SMT-03 证据', body: '可在稍后打开；不打断当前输入' },
  });
  await expect(page.getByTestId('attn-item-ordinary')).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId('attn-no-steal')).toBeVisible();
  await expect(input).toBeFocused();

  await page.getByTestId('attn-mute-toggle').click();
  await expect(page.getByTestId('attn-mute-toggle')).toHaveAttribute('aria-pressed', 'true');
  const before = await page.getByTestId('attn-item-ordinary').count();
  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'ordinary', title: '建议：复核 SMT-03 证据', body: '可在稍后打开；不打断当前输入' },
  });
  await expect(page.getByTestId('attn-item-ordinary')).toHaveCount(before);

  await page.request.post('/api/desktop/v1/attention', {
    data: { kind: 'urgent', title: '紧急：产线联锁', body: '安全联锁触发，请立即处理' },
  });
  await expect(page.getByTestId('attn-item-urgent')).toBeVisible({ timeout: 10_000 });
  await expect(input).toBeFocused();

  // #483 测试卫生：恢复静音设置为用例前状态，避免污染同服务后续用例
  await page.request.post('/api/desktop/v1/attention/mute', { data: { mute_ordinary: false } });
});

test('E2E-ADHOME: 首页整治后无测试控制与重复面板（T-88/UI16）', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  // 生产界面无测试注入控件（MOCK_REGISTRY 规则 2）
  await expect(page.getByTestId('attn-inject-ordinary')).toHaveCount(0);
  await expect(page.getByTestId('attn-inject-urgent')).toHaveCount(0);
  await expect(page.getByTestId('asst-read-reset')).toHaveCount(0);
  // 无配置类链接行与未启用横幅
  await expect(page.getByTestId('ops-module-off')).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'DES 仿真实验' })).toHaveCount(0);
  // A1 欠账：无旧顶栏页签
  await expect(page.locator('.main-tabs')).toHaveCount(0);
  // 侧栏分区齐全（任务/运营入口权限门控）
  await expect(page.getByTestId('open-tasks')).toBeVisible();
  await expect(page.getByTestId('ops-entry-workbench')).toBeDisabled();
  await expect(page.getByTestId('asst-read-root')).toBeVisible();
});

test('E2E-ADAPPR: 审批版本失效、拒绝与执行一次', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-appr').click();
  await expect(page.getByTestId('approval-mock-panel')).toBeVisible();
  await page.getByTestId('appr-reset').click();
  await expect(page.getByTestId('appr-revision')).toHaveText('1');

  await page.getByTestId('appr-approve').click();
  await expect(page.getByTestId('appr-status')).toHaveText('approved');
  await page.getByTestId('appr-params').fill('改排产 + 刀具补齐');
  await page.getByTestId('appr-bump').click();
  await expect(page.getByTestId('appr-revision')).toHaveText('2');
  await expect(page.getByTestId('appr-status')).toHaveText('pending');
  await page.getByTestId('appr-execute').click();
  await expect(page.getByTestId('appr-error')).toContainText('尚未批准');

  await page.getByTestId('appr-approve').click();
  await page.getByTestId('appr-execute').click();
  await expect(page.getByTestId('appr-exec-count')).toHaveText('1');
  await page.getByTestId('appr-execute').click();
  await expect(page.getByTestId('appr-exec-count')).toHaveText('1');

  await page.getByTestId('appr-reset').click();
  await page.getByTestId('appr-reject').click();
  await expect(page.getByTestId('appr-status')).toHaveText('rejected');
  await page.getByTestId('appr-execute').click();
  await expect(page.getByTestId('appr-error')).toContainText('拒绝或过期');

  await page.getByTestId('appr-reset').click();
  await page.getByTestId('appr-approve').click();
  await page.getByTestId('appr-execute-unknown').click();
  await expect(page.getByTestId('appr-receipt')).toHaveText('unknown');
  await page.getByTestId('appr-ack-unknown').click();
  await expect(page.getByTestId('appr-receipt')).toHaveText('applied');
  await expect(page.getByTestId('appr-exec-count')).toHaveText('1');
});

test('E2E-ADRESUME: 双任务断线重放不串写', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-resume').click();
  await expect(page.getByTestId('resume-mock-panel')).toBeVisible();
  await page.getByTestId('resume-seed').click();
  await expect(page.getByTestId('resume-status-TASK-2026-018')).toHaveText('running');
  await expect(page.getByTestId('resume-status-TASK-2026-021')).toHaveText('waiting_input');

  await page.getByTestId('resume-replay-a').click();
  await expect(page.getByTestId('resume-progress-TASK-2026-018')).toContainText('80%');
  await expect(page.getByTestId('resume-events-TASK-2026-018')).toHaveText('事件 2');
  await expect(page.getByTestId('resume-status-TASK-2026-021')).toHaveText('waiting_input');

  await page.getByTestId('resume-continue-b').click();
  await expect(page.getByTestId('resume-status-TASK-2026-021')).toHaveText('running');
  await expect(page.getByTestId('resume-progress-TASK-2026-021')).toContainText('Line-B');
  await expect(page.getByTestId('resume-status-TASK-2026-018')).toHaveText('running');
});

test('E2E-ADVOICE: 拒麦、部分转写、插话与断线改文字', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await expect(page.getByTestId('voice-mock-panel')).toBeVisible();
  await page.getByTestId('voice-reset').click();

  await page.getByTestId('voice-deny').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('denied');
  await expect(page.getByTestId('voice-fallback')).toContainText('文字');
  await page.getByTestId('voice-as-text').click();
  await expect(page.getByTestId('voice-final')).toContainText('改用文字继续');

  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-start').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('granted');
  await expect(page.getByTestId('voice-phase')).toHaveText('listening');
  await page.getByTestId('voice-inject-partial').click();
  await expect(page.getByTestId('voice-partial')).toContainText('SMT');
  await expect(page.getByTestId('voice-final')).toHaveText('—');
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).toContainText('SMT-03');
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('voice-partial')).toHaveText('—');

  await page.getByTestId('voice-play').click();
  await expect(page.getByTestId('voice-playing')).toHaveText('播放中');
  await expect(page.getByTestId('voice-phase')).toHaveText('speaking');
  await page.getByTestId('voice-barge').click();
  await expect(page.getByTestId('voice-playing')).toHaveText('已停');
  await expect(page.getByTestId('voice-phase')).toHaveText('listening');
  await expect(page.getByTestId('voice-final')).toContainText('SMT-03');

  await page.getByTestId('voice-offline').click();
  await expect(page.getByTestId('voice-phase')).toHaveText('offline');
  await expect(page.getByTestId('voice-fallback')).toBeVisible();
  await page.getByTestId('voice-as-text').click();
  await expect(page.getByTestId('voice-final')).toContainText('SMT-03');
});

async function openVoiceReal(page: import('@playwright/test').Page) {
  await page.addInitScript(() => {
    (window as unknown as { __AN_DESKTOP_VOICE_MEDIA__: string }).__AN_DESKTOP_VOICE_MEDIA__ =
      'real';
  });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await expect(page.getByTestId('voice-mock-panel')).toBeVisible();
  await expect(page.getByTestId('voice-media-mode')).toHaveText('media=real');
  await page.getByTestId('voice-reset').click();
}

test('E2E-ADMIC-01: 真机路径拒麦后可改文字', async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __AN_DESKTOP_VOICE_MEDIA__: string }).__AN_DESKTOP_VOICE_MEDIA__ =
      'real';
    const deny = () => Promise.reject(new DOMException('Permission denied', 'NotAllowedError'));
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: { getUserMedia: deny },
    });
  });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await expect(page.getByTestId('voice-media-mode')).toHaveText('media=real');
  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-open-mic').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('denied');
  await expect(page.getByTestId('voice-fallback')).toContainText('文字');
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-as-text').click();
  await expect(page.getByTestId('voice-final')).toContainText('改用文字继续');
});

test('E2E-ADMIC-02: 授权开麦、部分不关联、确认后关联', async ({ page }) => {
  await openVoiceReal(page);
  await page.getByTestId('voice-open-mic').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('granted');
  await expect(page.getByTestId('voice-phase')).toHaveText('listening');
  await page.getByTestId('voice-asr-partial').click();
  await expect(page.getByTestId('voice-partial')).toContainText('SMT');
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).toContainText('SMT');
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADMIC-03: 播报中插话停播报并保留最终转写', async ({ page }) => {
  await openVoiceReal(page);
  await page.getByTestId('voice-open-mic').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('granted');
  await page.getByTestId('voice-asr-partial').click();
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).not.toHaveText('—');
  const finalBefore = await page.getByTestId('voice-final').innerText();
  await page.getByTestId('voice-play').click();
  await expect(page.getByTestId('voice-playing')).toHaveText('播放中');
  await page.getByTestId('voice-barge').click();
  await expect(page.getByTestId('voice-playing')).toHaveText('已停');
  await expect(page.getByTestId('voice-phase')).toHaveText('listening');
  await expect(page.getByTestId('voice-final')).toHaveText(finalBefore);
});

test('E2E-ADMIC-04: offline 后改用文字保留最终转写', async ({ page }) => {
  await openVoiceReal(page);
  await page.getByTestId('voice-open-mic').click();
  await page.getByTestId('voice-asr-partial').click();
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).not.toHaveText('—');
  const finalBefore = await page.getByTestId('voice-final').innerText();
  await page.getByTestId('voice-offline').click();
  await expect(page.getByTestId('voice-phase')).toHaveText('offline');
  await expect(page.getByTestId('voice-fallback')).toBeVisible();
  await page.getByTestId('voice-as-text').click();
  await expect(page.getByTestId('voice-final')).toHaveText(finalBefore);
});

test('E2E-ADASR-01: remote-stub 自动 partial 并关联', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as {
      __AN_DESKTOP_VOICE_MEDIA__: string;
      __AN_DESKTOP_ASR_BACKEND__: string;
    };
    w.__AN_DESKTOP_VOICE_MEDIA__ = 'real';
    w.__AN_DESKTOP_ASR_BACKEND__ = 'remote-stub';
  });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await expect(page.getByTestId('voice-asr-backend')).toHaveText('asr=remote-stub');
  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-open-mic').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('granted');
  await expect(page.getByTestId('voice-asr-backend')).toHaveText('asr=remote-stub');
  await expect(page.getByTestId('voice-partial')).toContainText('远端', { timeout: 5_000 });
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).toContainText('远端');
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADASR-02: remote 无密钥回退并可继续', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as {
      __AN_DESKTOP_VOICE_MEDIA__: string;
      __AN_DESKTOP_ASR_BACKEND__: string;
      __AN_DESKTOP_ASR_URL__?: string;
    };
    w.__AN_DESKTOP_VOICE_MEDIA__ = 'real';
    w.__AN_DESKTOP_ASR_BACKEND__ = 'remote';
    delete w.__AN_DESKTOP_ASR_URL__;
  });
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-open-mic').click();
  await expect(page.getByTestId('voice-permission')).toHaveText('granted');
  await expect(page.getByTestId('voice-asr-note')).toContainText('真钥另验');
  await expect(page.getByTestId('voice-asr-backend')).toHaveText('asr=webspeech');
  await page.getByTestId('voice-asr-partial').click();
  await expect(page.getByTestId('voice-partial')).toContainText('SMT');
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADHAND: 跨端草稿冲突与未授权转写不共享', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-handoff').click();
  await expect(page.getByTestId('handoff-mock-panel')).toBeVisible();
  await page.getByTestId('handoff-reset').click();

  await expect(page.getByTestId('handoff-task')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('handoff-run')).toHaveText('run-smt-03-1');
  await expect(page.getByTestId('handoff-anchor')).toHaveText('msg-anchor-1');
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('未授权不共享');
  await expect(page.getByTestId('handoff-peer-voice')).toHaveText('未授权不共享');

  await page.getByTestId('handoff-phone').fill('手机侧草稿 A');
  await page.getByTestId('handoff-desktop').fill('桌面侧草稿 B');
  await expect(page.getByTestId('handoff-conflict')).toHaveText('有');
  await page.getByTestId('handoff-choose-phone').click();
  await expect(page.getByTestId('handoff-conflict')).toHaveText('无');
  await expect(page.getByTestId('handoff-chosen')).toHaveText('phone');
  await expect(page.getByTestId('handoff-phone')).toHaveValue('手机侧草稿 A');
  await expect(page.getByTestId('handoff-desktop')).toHaveValue('手机侧草稿 A');

  await page.getByTestId('handoff-voice-share').click();
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('共享中');
  await page.getByTestId('handoff-voice-share').click();
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('未授权不共享');
});

test('E2E-ADD01-01: 双 context 手机发起关联；桌面未授权不共享', async ({
  page,
  browser,
}) => {
  // 端 A = 手机：语音发起并关联
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await page.getByTestId('voice-reset').click();
  const t0 = Date.now();
  await page.getByTestId('voice-start').click();
  await expect(page.getByTestId('voice-phase')).toHaveText('listening');
  await page.getByTestId('voice-inject-partial').click();
  await expect(page.getByTestId('voice-partial')).toContainText('SMT');
  const firstPartialMs = Date.now() - t0;
  expect(firstPartialMs).toBeGreaterThanOrEqual(0);
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-final')).toContainText('SMT-03');
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);

  await page.getByTestId('workspace-open-handoff').click();
  await page.getByTestId('handoff-reset').click();
  await expect(page.getByTestId('handoff-task')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('handoff-run')).toHaveText('run-smt-03-1');
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('未授权不共享');

  // 端 B = 桌面：同 Task/Run；对端转写门闩
  const ctxB = await browser.newContext();
  const pageB = await ctxB.newPage();
  await pageB.goto('/');
  await pageB.getByTestId('card-smt-03').click();
  await pageB.getByTestId('workspace-open-handoff').click();
  await expect(pageB.getByTestId('handoff-mock-panel')).toBeVisible();
  await pageB.getByTestId('handoff-refresh').click();
  await expect(pageB.getByTestId('handoff-task')).toHaveText(SMT03_TASK_ID);
  await expect(pageB.getByTestId('handoff-run')).toHaveText('run-smt-03-1');
  await expect(pageB.getByTestId('handoff-anchor')).toHaveText('msg-anchor-1');
  await expect(pageB.getByTestId('handoff-voice-shared')).toHaveText('未授权不共享');
  await expect(pageB.getByTestId('handoff-peer-voice')).toHaveText('未授权不共享');
  await ctxB.close();
});

test('E2E-ADD01-02: 授权共享后桌面可见转写并可续写', async ({ page, browser }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-start').click();
  await page.getByTestId('voice-inject-partial').click();
  await page.getByTestId('voice-commit').click();
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
  const finalText = await page.getByTestId('voice-final').innerText();

  await page.getByTestId('workspace-open-handoff').click();
  await page.getByTestId('handoff-reset').click();
  await page.getByTestId('handoff-voice-share').click();
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('共享中');
  await expect(page.getByTestId('handoff-peer-voice')).toContainText('SMT');

  const ctxB = await browser.newContext();
  const pageB = await ctxB.newPage();
  await pageB.goto('/');
  await pageB.getByTestId('card-smt-03').click();
  await pageB.getByTestId('workspace-open-handoff').click();
  // #351 / #335 范式：以服务端可见性为准轮询（每次点击刷新后断言），不放松最终断言、
  // 不放宽为 any——共享状态与对端转写文本在服务端确认后必须完全一致。
  await expect.poll(async () => {
    await pageB.getByTestId('handoff-refresh').click();
    return pageB.getByTestId('handoff-voice-shared').innerText().catch(() => '');
  }, { timeout: 15_000 }).toBe('共享中');
  await expect.poll(async () => {
    await pageB.getByTestId('handoff-refresh').click();
    return pageB.getByTestId('handoff-peer-voice').innerText().catch(() => '');
  }, { timeout: 15_000 }).toBe(finalText);
  await expect(pageB.getByTestId('handoff-peer-link')).toHaveText(SMT03_TASK_ID);

  await pageB.getByTestId('handoff-desktop').fill('桌面续写草稿');
  await expect(pageB.getByTestId('handoff-desktop')).toHaveValue('桌面续写草稿');

  // 桌面打开语音只刷新、不重置
  await pageB.getByTestId('workspace-open-voice').click();
  await pageB.getByTestId('voice-refresh').click();
  await expect(pageB.getByTestId('voice-final')).toHaveText(finalText);
  await expect(pageB.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
  await ctxB.close();
});

test('E2E-ADD01-03: 先不关联再纠正；共享后对端可见', async ({ page, browser }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-voice').click();
  await page.getByTestId('voice-reset').click();
  await page.getByTestId('voice-start').click();
  await page.getByTestId('voice-inject-partial').click();
  await page.getByTestId('voice-commit-nolink').click();
  await expect(page.getByTestId('voice-final')).toContainText('SMT-03');
  await expect(page.getByTestId('voice-link')).toHaveText('未关联');
  await page.getByTestId('voice-correct-link').click();
  await expect(page.getByTestId('voice-link')).toHaveText(SMT03_TASK_ID);
  const finalText = await page.getByTestId('voice-final').innerText();

  await page.getByTestId('workspace-open-handoff').click();
  await page.getByTestId('handoff-reset').click();
  await page.getByTestId('handoff-voice-share').click();
  await expect(page.getByTestId('handoff-voice-shared')).toHaveText('共享中');

  const ctxB = await browser.newContext();
  const pageB = await ctxB.newPage();
  await pageB.goto('/');
  await pageB.getByTestId('card-smt-03').click();
  await pageB.getByTestId('workspace-open-handoff').click();
  await pageB.getByTestId('handoff-refresh').click();
  await expect(pageB.getByTestId('handoff-peer-voice')).toHaveText(finalText);
  await expect(pageB.getByTestId('handoff-peer-link')).toHaveText(SMT03_TASK_ID);
  await ctxB.close();
});

test('E2E-ADACCEPT: 丢回执回读与同操作 ID 幂等冲突', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('card-smt-03').click();
  await page.getByTestId('workspace-open-accept').click();
  await expect(page.getByTestId('accept-mock-panel')).toBeVisible();
  await page.getByTestId('accept-reset').click();

  await page.getByTestId('accept-mode-legacy').click();
  await expect(page.getByTestId('accept-mode')).toHaveText('现有');
  await page.getByTestId('accept-drop-receipt').click();
  await expect(page.getByTestId('accept-phase')).toHaveText('unknown');
  await expect(page.getByTestId('accept-pending-hint')).toBeVisible();
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');
  await page.getByTestId('accept-retry').click();
  await expect(page.getByTestId('accept-error')).toContainText('先回读');
  await page.getByTestId('accept-readback').click();
  await expect(page.getByTestId('accept-phase')).toHaveText('accepted');
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');

  await page.getByTestId('accept-reset').click();
  await page.getByTestId('accept-mode-reliable').click();
  await expect(page.getByTestId('accept-mode')).toHaveText('可靠');
  await page.getByTestId('accept-drop-receipt').click();
  await expect(page.getByTestId('accept-phase')).toHaveText('unknown');
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');
  await page.getByTestId('accept-retry').click();
  await expect(page.getByTestId('accept-phase')).toHaveText('accepted');
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');
  await page.getByTestId('accept-retry').click();
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');

  await page.getByTestId('accept-change-text').click();
  await page.getByTestId('accept-retry').click();
  await expect(page.getByTestId('accept-phase')).toHaveText('conflict');
  await expect(page.getByTestId('accept-error')).toContainText('409');
  await expect(page.getByTestId('accept-dispatch-count')).toHaveText('1');
  await expect(page.getByTestId('accept-task')).toHaveText(SMT03_TASK_ID);
});
