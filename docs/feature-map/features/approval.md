# approval：人工审批

高风险任务先停在“待批准”，没有执行也没有结果；用户点“批准执行”后才开始。

## Sub-features

- 触发：Router 风险 ≥ `[orchestrator].approval_risk`（默认 high），或 Policy 规则要求
- 对话内与任务卡内都有“批准执行”
- 审批说明（`ApprovalInfo`）：将交给谁（执行组顺序）、预计工具（Router 预估）、为什么要批准（Policy 触发规则）、完成后
- 批准后进入排队/执行；不批准可取消（两步确认）

## How to get to it (user POV)

发一条高风险请求（如删除、对外发送、部署）→ 对话区状态显示“待批准” → 点“批准执行”。

## Driving it with Playwright / HTTP

```ts
const out = await send(page, '审批验证：部署到生产');       // 受控模型：目标含“审批”→ high
await expect(page.locator('main').getByRole('button', { name: '批准执行', exact: true })).toBeVisible();
// 此时任务没有 result：
expect(out.task.status).toBe('NEEDS_HUMAN');                // POST /messages 响应
const t = (await (await page.request.get(`/api/tasks/${out.task.id}`)).json()).task;
expect(t.result).toBeNull();
await page.locator('main').getByRole('button', { name: '批准执行', exact: true }).click();
```

HTTP：`POST /api/tasks/{id}/approve`。**不要在真实环境批准真实危险操作来做验证。**

## Verify（判据）

- 批准前：`status === NEEDS_HUMAN`，`result` 为空，没有任何 agent 事件。
- 点击后才出现 RUNNING/COMPLETED。
- 批准按钮上方的说明同时包含“将交给”“预计工具”“为什么要批准”，内容与任务的 assignment/policy 记录一致。

## Gotchas

- “预计工具”是 Router **预估**，不是硬限制；agent 执行时可能调用其他工具。
- Agent 执行中再请求的工具权限默认自动允许（因任务已被批准），逐次记录在证据里；这与“批准执行”是两件事。
- 风险等级是模型建议，**最终是否审批由 Policy 的确定性规则决定**；开启 `[conversation].risk_rules` 后风险只升不降。
- 只有一处取消按钮；批准与取消都经 `App.taskAction`，失败显示在顶部红色错误栏（点击关闭），不用 `alert`。

## Backed by

源码：`crates/runtime/src/policy.rs`、`web/src/components/ChatView.tsx`、`ApprovalInfo.tsx`
用例：E2E-APPROVE、E2E-APPROVEINFO；RT-ASSIGN-05
验证状态：自动化已覆盖。准入接口（`/api/admission`）的审批见 [routing-and-admission.md](routing-and-admission.md)。
