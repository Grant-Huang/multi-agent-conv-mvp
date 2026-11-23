# observability / feedback / http-host：诊断、路由日志、派错反馈与访问防护

## Sub-features

- **诊断页**：后端（连通性、槽位）、执行组、问题统计（最近 500 条）、最近的问题；“刷新”“复制诊断信息”
- **路由日志页**：每轮前台判断 + 路由的决策与模型原始输出（可展开）
- **开发者模式**：默认关；开关“开发者模式：关/开”，状态保存在 `localStorage`；关闭时 诊断/路由日志页签隐藏
- **派错了反馈**：展开可匹配用户消息的任务 →“派错了”→ 选动作/角色 +“一句说明”→“保存”→“已记下”
- **JSON 日志**：`logs/agentnexus.jsonl.<日期>`，固定 `event` + `code` + `task_id`
- **http-host**：主服务与 `an-mom` 统一 Host 校验；拒绝时 WARN `http.host_rejected` / `E_HTTP_HOST`

## How to get to it (user POV)

右栏底部点“开发者模式：关” → 出现“诊断”“路由日志”页签。任务卡展开后看“派错了”。

## Driving it with Playwright / HTTP

```ts
await page.getByRole('button', { name: '开发者模式：关', exact: true }).click();
await page.getByRole('tab', { name: '诊断', exact: true }).click();
await expect(page.getByRole('heading', { name: '后端', exact: true })).toBeVisible();
await page.getByRole('tab', { name: '路由日志', exact: true }).click();
```

HTTP：`GET /api/diagnostics`、`GET /api/route-logs?limit=`、`POST /api/feedback`。
日志：`jq -c 'select(.task_id == "task-xxxx") | [.timestamp, .event, .status, .code]' logs/agentnexus.jsonl.*`

## Verify（判据）

- 诊断页显示真实后端地址/状态，不是占位符；路由日志里能匹配本次消息，展开能看到模型原文。
- 反馈：`POST /api/feedback` 返回统一信封并持久化；非法动作/角色被拒。导出仅为草稿，不改评测集。任务卡「派错了」经 `taskMessageIds` 关联用户消息（优先 `message.task_id`，否则 goal===content；委派尚未回写 user.task_id）。
- Host 防护：恶意 Host 返回 403 且无写入副作用；静态页、API、SSE、同源 MOM、合法 IPv6 authority 正常；Host 最多记 100 字符，路径最多 200 字符，不记请求体/其他头/查询串；合法请求不新增拒绝事件。

## Gotchas

- 开发者模式关闭时若停在“诊断/路由日志”，会自动显示“任务”页签但不改已保存的选择。
- 诊断/路由日志只在页签打开时随事件节流刷新；断言前先点“刷新”。
- “复制诊断信息”依赖浏览器剪贴板权限，完整浏览器验证尚缺。
- Host 防护不是身份认证，也不是 DNS 重绑定的浏览器级复现；用反向代理访问会被 403。
- 日志目录不可写时降级为控制台/内存诊断，不要因日志文件缺失就判服务异常；降级时不应再误报 `log.file`。
- 新增埋点须使用 `docs/DESIGN.md` 第 10 节的事件名并关联 `task_id`。

## Backed by

源码：`web/src/components/DiagnosticsPanel.tsx`、`RouteLogPanel.tsx`、`TaskPanel.tsx`（misroute）、`crates/observe/`、`crates/server/src/host_guard.rs`
用例：E2E-DIAG；E2E-FEEDBACK（`web/e2e/codeflow.spec.ts`）；RT-HOST-01～11（`crates/server/tests/host_guard.rs`、`host_diagnostics.rs`、`host_log_limits.rs`）；misroute 组件与 Rust HTTP 测试
验证状态：诊断/路由日志与反馈写入浏览器已覆盖；反馈→评测仍缺（G06）；剪贴板复制未完整验证。
