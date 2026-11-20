# ai-desktop：组织级 AI 工作桌面（V1.7）

规格：[docs/ai-desktop/DESIGN.md](../../ai-desktop/DESIGN.md)；计划与 Mock：[PLAN.md](../../ai-desktop/PLAN.md)、[MOCK_REGISTRY.md](../../ai-desktop/MOCK_REGISTRY.md)；追溯：[TRACEABILITY.md](../../ai-desktop/TRACEABILITY.md)。

## Sub-features（本阶段）

| 子功能 | 说明 |
|---|---|
| 桌面首页 | 默认入口；「我的工作 / Agent 工作 / 为你关注」；Mock 卡片含 SMT-03 |
| 工作区 S2/S3 | 同一 Task 分屏与专注；布局偏好按设备本地保存 |
| 模块入口 | 会话（真实 API）、运营桌面（有开关则真，否则明确未启用） |

## How to get to it

打开主站 `/` → 默认「我的 AI 桌面」。点 SMT-03 卡进入工作区；侧栏「＋ 新会话」进入会话；「运营桌面」进入运营模块或未启用说明。

## Driving it with Playwright

```ts
await page.goto('/');
await expect(page.getByTestId('desktop-home')).toBeVisible();
await page.getByTestId('card-smt-03').click();
await expect(page.getByTestId('workspace')).toContainText('TASK-2026-018');
```

用例：`E2E-ADNAV`、`E2E-ADLAYOUT`（A1）；`E2E-ADDRAFT`、`E2E-ADSCOPE`（A2）；`E2E-ADREAD`（A2.1，含运营频道）；`E2E-ADEVID`、`E2E-ADHISTORY`（A3）；`E2E-ADPROF`、`E2E-ADINPUT`（A4）；`E2E-ADASK`、`E2E-ADATTN`（A5.1）；`E2E-ADAPPR`、`E2E-ADRESUME`（A5.2）。其余 AD-* 见 FEATURE_MAP，随里程碑登记。
