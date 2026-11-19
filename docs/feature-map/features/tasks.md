# tasks / assignment / cancellation：任务、执行者与取消

委派后的后台任务：状态、走马灯进度、产物下载、执行者归属、取消、失败卡片。

## Sub-features

- 任务状态流转：CREATED → QUEUED → RUNNING → VERIFYING → COMPLETED / FAILED / NEEDS_HUMAN / CANCELLED（显示名见 `web/src/labels.ts`）
- 对话区状态行：排队中（第 N 位）/ 通用·编码执行中 / 审查中 / 已完成…
- **走马灯**：挂在任务第一条消息（简短回复）之下；收起=最新一行，展开=从旧到新，无步骤编号，运行中行尾有转圈（`aria-label="执行中"`）；最终正文在走马灯之后
- 产物：文件列表，“查看”“下载”；下载字节与 artifact、磁盘文件一致
- 失败卡片：执行历史从旧到新，失败原因并入末尾
- **assignment**：卡片标题行显示执行者；展开“执行者”节含 agent、主力/备份 N、排队时长、返工轮次；进入 RUNNING 时已写入
- **cancellation**：两步确认取消（3 秒内二次点击）
- 补充要求：先过 Router/Policy，再取消旧任务并创建替代任务

## How to get to it (user POV)

在对话里提出需要“做”的事（写文件、算数、写报告、改代码）→ 对话区出现状态行 + 走马灯 → 右栏“任务”页签看卡片，点开展开。

## Driving it with Playwright / HTTP

```ts
const out = await send(page, '写报告并保存');              // 见 conversation.md 的 send()
const id = out.task.id;                                    // POST /messages 响应的 task 字段
await expect.poll(async () =>
  (await (await page.request.get(`/api/tasks/${id}`)).json()).task.status).toBe('COMPLETED');
await page.locator('main').getByRole('button', { name: '下载', exact: true }).first().click(); // 配合 waitForEvent('download')
```

取消：`main` 下点“取消任务”→ 再点“确认取消”（必须在 3 秒内）。
任务卡片：`page.getByRole('tab', { name: '任务' })`；“执行者”节：`card.locator('section.section').filter({ has: page.getByRole('heading', { name: '执行者', exact: true }) })`。
HTTP：`GET /api/tasks?conversation_id=`、`GET /api/tasks/{id}`（`.task` + `.events`）、`POST /api/tasks/{id}/cancel`。

## Verify（判据）

- 任务 `status === COMPLETED` 且 `task.review.verdict === 'pass'`；`task.result.artifacts[]`（`kind: 'file'`，含 `name`、`content`）；`reports/xxx.md` 之类产物在文件工具目录里真实存在，下载内容与其一致。
- 走马灯：展开后条目按时间升序，不含“步骤 N”；运行中时存在“执行中”指示，结束后消失。
- 取消：第一次点击后按钮文字变“确认取消”，第二次后 `status === CANCELLED`，API 与页面一致；3 秒不点会恢复原样。
- 执行者：`assignment` 里记录 agent、`member_index`（0=主力）、排队时长；旧库自动补列。
- 失败任务：卡片末尾含失败原因，且之前的执行历史未丢。

## Gotchas

- **进程成功 ≠ 验收通过。** 代码/文档类要 Reviewer pass 才 COMPLETED；排查“看不到结果”时同时查任务状态、事件里的审查结论、产物是否落盘。
- 页面上有多个“取消”“下载”按钮（对话区与任务面板各一份）：必须用 `main` 或任务区限定作用域。
- “待批准”的任务只在一处显示取消按钮，不要期待两处都有。
- 任务超时从**首次分到 agent** 起算，排队不计；服务重启时被中断的任务标记为 FAILED（无断点续跑）。
- 取消旧任务与创建新任务之间非原子（补充要求场景），极端情况下可能短暂并存。
- 多节点真实执行组的“备份接手”未在浏览器验证，只有后端 RT-ASSIGN-* 覆盖。

## Backed by

源码：`web/src/components/TaskPanel.tsx`、`TaskTicker.tsx`、`ChatView.tsx`、`AssignmentLine.tsx`、`web/src/artifacts.ts`；`crates/runtime/src/orchestrator.rs`、`registry.rs`、`store.rs`；`crates/agents/src/general.rs`
用例：E2E-FILE、E2E-TICKER、E2E-FAILCARD、E2E-CANCEL、E2E-ASSIGN；RT-ASSIGN-01～04；RT-CONV-01/02（`crates/server/tests/supplement_routing.rs`）
**高风险文件**：`orchestrator.rs`、`store.rs`、`registry.rs` 修改前须先给方案（见 AGENTS.md）。
验证状态：自动化已覆盖（受控模型）。
