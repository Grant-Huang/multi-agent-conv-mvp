# 运营桌面与运营频道：实现前 Red 证据

日期：2026-11-18。命令：`cd web && PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts --reporter=list`（用例 `web/e2e/ops.spec.ts`，断言见 `docs/testing/CASES.md` 末节）。

第一次运行因本机没有 Playwright 自带的 Chromium 而失败（环境问题，不算 Red）；改用本机已缓存的浏览器后重跑。第二次运行因测试里命令体字段写成 `type`、应为 `command` 失败（测试夹具的拼写错误，已修，不涉及断言）。修正后的结果：

| Case | 结果 | 失败原因（业务缺失） |
|---|---|---|
| E2E-OPSDESK | 失败 | 案例已成功注入，但页面上没有“运营桌面”页签 |
| E2E-OPSDRAWER | 失败 | 同上，没有“运营桌面”页签 |
| E2E-OPSCHANNEL | 失败 | 案例已注入，但没有未读数 `ops-unread`，没有运营频道 |
| E2E-OPSNOISE | 失败 | 案例已注入，但 `ops-channel` 会话不存在，没有任何频道消息 |
| E2E-OPSOFF | 通过 | 基线：当前本来就没有桌面和频道，用于防止回归 |

4 个失败、1 个通过。失败都发生在业务断言处（案例注入已成功），不是环境或夹具错误。
