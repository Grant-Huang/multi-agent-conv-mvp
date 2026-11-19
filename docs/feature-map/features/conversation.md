# conversation / navigation：会话与导航

用户和系统说话的主通道：创建会话、发消息、直接回答或委派、刷新后保持、窄屏抽屉。

## Sub-features

- 新建会话、切换会话、刷新后回到上次会话（`localStorage` 键 `an.currentConversation`）
- 发送消息（Enter 发送，Shift+Enter 换行），乐观显示用户消息
- 前台决策四选一：respond（直接回答）/ clarify（追问）/ delegate（委派后台）/ follow_up（跟进已有任务）
- 流式回答；`<think>…</think>` 内容不显示给用户
- 空会话“试试这些”四条示例
- 长时间等待提示（≥20 秒）“模型可能正在处理其他请求，请稍候。”
- 窄屏：左栏变抽屉，“☰ 会话”打开，选择/新建后自动关闭
- 简短回复完整显示用户意图，不截断（E2E-ACK）

## How to get to it (user POV)

打开 `/` → 左栏“＋ 新会话” → 中间输入框“输入消息”输入 → 点“发送”或 Enter。应用是单页，**没有** `/tasks`、`/settings` 等路径。

## Driving it with Playwright / HTTP

```ts
await page.goto('/');
await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
await page.getByRole('textbox', { name: '输入消息' }).fill('你好验证');
const resp = page.waitForResponse(r => r.url().endsWith('/messages') && r.request().method() === 'POST');
await page.getByRole('button', { name: '发送', exact: true }).click();
expect((await resp).ok()).toBeTruthy();
await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
```

切换会话：`page.getByRole('complementary', { name: '会话列表' }).getByRole('button', { name: '<会话标题>', exact: true }).click()`。
窄屏：`page.setViewportSize({ width: 390, height: 800 })` 后点 `getByRole('button', { name: '☰ 会话' })`。
HTTP：`POST /api/conversations`，`POST /api/conversations/{id}/messages` body `{"text": "..."}`，`GET .../messages`。

## Verify（判据）

- 刷新前后消息 ID 与内容一致；另一会话里看不到本会话消息。
- 助手消息不含 `<think>` 内容（页面无“私有思考”之类字样）。
- 窄屏：创建/选择会话后抽屉关闭，输入框可用。
- 简短回复里包含完整的用户目标文本。

## Gotchas

- 乐观消息 ID 形如 `local-<时间戳>`，发送完成后被服务端消息替换；断言“消息 ID 一致”要在刷新后做。
- 消息列表和任务列表是两次请求，SSE 事件触发后才刷新；断言前用 `expect.poll` 等待而非固定 sleep。
- 模型小导致“简单问题被委派”或“该委派却直接回答”属于路由质量问题，不是功能故障：到“路由日志”查原始输出。
- 受控模型下回答是固定文本；真实模型的多问题质量需单独验收。
- 开发者模式、发现卡已读标记保存在 `localStorage`：复用 context 会让“首次打开”类用例失败。

## Backed by

源码：`web/src/App.tsx`、`web/src/components/ChatView.tsx`、`crates/runtime/src/conversation.rs`
用例：E2E-CONV、E2E-MOBILE、E2E-ACK；RT-CONV-01/02（补充要求路由，见 [tasks.md](tasks.md)）
验证状态：自动化已覆盖（受控模型）；真实模型质量未验收。
