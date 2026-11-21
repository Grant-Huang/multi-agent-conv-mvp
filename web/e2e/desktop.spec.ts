import { test, expect } from '@playwright/test';
import { SMT03_TASK_ID } from '../src/desktop/mockData';

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
  await expect(page.getByTestId('ops-module-off')).toContainText('运营模块未启用');
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

  await page.getByTestId('settings-back').click();
  await page.getByTestId('settings-back').click();
  await expect(page.getByTestId('settings-panel')).toHaveCount(0);
  await expect(page.getByTestId('workspace-input')).toHaveValue('设置中应保留的草稿');

  await page.getByRole('button', { name: '返回桌面', exact: true }).click();
  await expect(page.getByTestId('home-assistant-name')).toContainText('桌面测试助理');
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
  await page.goto('/');
  await expect(page.getByTestId('s1-ask-bar')).toBeVisible();
  await page.getByTestId('s1-ask-input').fill('SMT-03 概况如何');
  await page.getByTestId('s1-ask-send').click();
  await expect(page.getByTestId('s1-reply')).toContainText('问答');
  await expect(page.getByTestId('desktop-home')).toBeVisible();
  await expect(page.getByTestId('workspace')).toHaveCount(0);

  await page.getByTestId('s1-ask-input').fill('深入调查并整理证据链');
  await page.getByTestId('s1-delegate').click();
  await expect(page.getByTestId('s1-bg-chip')).toBeVisible();
  await expect(page.getByTestId('s1-bg-status')).toContainText(/提交中|已受理/);
  await expect.poll(async () => page.getByTestId('s1-bg-status').innerText()).toContain('已受理');
  await expect(page.getByTestId('s1-bg-task')).toHaveText(SMT03_TASK_ID);
  await expect(page.getByTestId('desktop-home')).toBeVisible();

  await page.getByTestId('s1-ask-input').fill('后台跑着时继续问');
  await page.getByTestId('s1-ask-send').click();
  await expect(page.getByTestId('s1-reply')).toContainText('继续问');
  await expect(page.getByTestId('s1-bg-chip')).toBeVisible();

  await page.getByTestId('s1-bg-open').click();
  await expect(page.getByTestId('workspace-task-id')).toHaveText(SMT03_TASK_ID);
});

test('E2E-ADATTN: 输入中普通建议不抢焦点且可静音', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '＋ 新会话' }).click();
  const input = page.getByRole('textbox', { name: '输入消息' });
  await input.focus();
  await input.fill('正在写草稿');
  await expect(input).toBeFocused();

  await page.getByTestId('attn-inject-ordinary').click();
  await expect(page.getByTestId('attn-item-ordinary')).toBeVisible();
  await expect(page.getByTestId('attn-no-steal')).toBeVisible();
  await expect(input).toBeFocused();

  await page.getByTestId('attn-mute-toggle').click();
  await expect(page.getByTestId('attn-mute-toggle')).toHaveAttribute('aria-pressed', 'true');
  const before = await page.getByTestId('attn-item-ordinary').count();
  await page.getByTestId('attn-inject-ordinary').click();
  await expect(page.getByTestId('attn-item-ordinary')).toHaveCount(before);

  await page.getByTestId('attn-inject-urgent').click();
  await expect(page.getByTestId('attn-item-urgent')).toBeVisible();
  await expect(input).toBeFocused();
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
