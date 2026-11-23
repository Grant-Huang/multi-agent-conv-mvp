// E2E-SMAT：同会话续问使用窗口外会话资料（conversation-memory 5.1，REQ-conversation-memory-01）。
// 真实链路：桌面 UI 委派任务 → B1 HTTP 建任务链接 → 任务完成回执被 12 条消息挤出窗口
// → UI 续问 → 回答文本含窗口外摘要（仅能经资料注入进入模型）。
// 夹具：harness.mjs 假模型仅在「续问含探针词 + 提示词带资料块」时回声摘要。
import { test, expect, type Page } from '@playwright/test';

async function send(page: Page, text: string) {
  await page.getByRole('textbox', { name: '输入消息' }).fill(text);
  const response = page.waitForResponse(
    (r) => r.url().endsWith('/messages') && r.request().method() === 'POST',
  );
  await page.getByRole('button', { name: '发送', exact: true }).click();
  const result = await response;
  expect(result.ok()).toBeTruthy();
  return result.json();
}

async function complete(page: Page, id: string) {
  await expect
    .poll(async () => (await (await page.request.get(`/api/tasks/${id}`)).json()).task.status, {
      timeout: 60_000,
    })
    .toBe('COMPLETED');
}

test('E2E-SMAT: 续问回答使用窗口外会话资料摘要', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('/');
  const discovery = page.getByRole('region', { name: '发现的助手' });
  if (await discovery.count()) {
    await discovery.getByRole('button', { name: '知道了', exact: true }).click();
  }
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await expect(page.locator('.msg')).toHaveCount(0);

  // 1. 委派任务（fake 模型：非「你好」开头 → delegate → general 写文件 → 审查通过）
  const out = await send(page, '会话资料验证任务：写一份验证报告');
  expect(out.task).toBeTruthy();
  await complete(page, out.task.id);

  // 2. 用生产的 B1 链接路径把任务绑进本会话（会话标题 = 首条消息截断）
  const convsBody = await (await page.request.get('/api/conversations')).json();
  const convs: Array<{ id: string; title?: string }> = Array.isArray(convsBody)
    ? convsBody
    : convsBody?.data ?? [];
  const conv = convs.find((c) => c.title?.includes('会话资料验证任务'));
  const convId = conv?.id ?? '';
  expect(
    convId,
    `应找到本会话；会话列表=${JSON.stringify(convsBody).slice(0, 400)}`,
  ).toBeTruthy();
  const link = await page.request.post(`/api/desktop/v1/tasks/${out.task.id}/links`, {
    data: {
      conversation_id: convId,
      client_operation_id: 'e2e-smat-op-1',
      source_message_ids: [],
      created_by: 'e2e',
    },
  });
  expect(link.ok(), await link.text()).toBeTruthy();

  // 3. 灌 12 条直接回答消息，把回执挤出近期窗口
  for (let i = 0; i < 12; i++) {
    const r = await page.request.post(`/api/conversations/${convId}/messages`, {
      data: { text: `你好，填充消息${i}` },
    });
    expect(r.ok()).toBeTruthy();
  }

  // 4. UI 续问（「你好」开头走直接回答；探针词触发 harness 回声）
  const reply = await send(page, '你好，会话资料验证：之前那个任务的结论是什么？');
  expect(reply.reply?.content).toBeTruthy();

  // 5. 回答必须包含窗口外摘要——它只能经会话资料注入进入模型
  expect(
    reply.reply.content.includes('验证报告已写入'),
    `回答应含窗口外回执摘要：${reply.reply.content}`,
  ).toBeTruthy();
  await expect(page.locator('.msg').last()).toContainText('验证报告已写入', { timeout: 15_000 });
});
