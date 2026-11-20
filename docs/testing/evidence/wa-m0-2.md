# World Adapter M0-2 — DES 仿真实验前端

## 范围

- `web/src/desktop/DesExperiment.tsx`：配置 / 试跑 / 正式运行 / 结果四视图
- 入口：「我的 AI 桌面」→ 链接「DES 仿真实验」（设置与管理）
- API：`api.worldMock.*` → `/api/world/mock/experiment/*`（`X-WA-Role: experimenter`）
- Harness：`[mom] world_mock=true`（`web/e2e/harness.mjs`）
- 不改 `engine.rs`；不做运营频道发卡（属 M0-3）

## Red → Green

| Case | 状态 | 说明 |
|---|---|---|
| E2E-DESTRIAL | Green | 非法时长提示；试跑 ID；`ops-delivery-count=0` |
| E2E-DESSTART | 部分（仍 Red） | 实验页配置版本 / 正式 ID / 幂等 / 试跑≠正式已通；卡在 `ops-case-card`（M0-3） |
| E2E-DESOPS～LOOP | Red | 待 M0-3～M0-5 |

```text
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/bin/google-chrome-stable \
  npx playwright test e2e/des-experiment.spec.ts -g 'E2E-DESTRIAL'
  ✓  E2E-DESTRIAL: 配置试跑不向运营频道投递
```

## 控件契约（已落地）

`DES 仿真实验`、`配置`、`试跑`、`启动正式运行`、`暂停`、`单步推进`、`运营频道`；
`trial-run-id`、`ops-delivery-count`、`config-version`、`target-channel`、`official-run-id`、`last-trial-run-id`、`run-status`。

## 后续

M0-3：正式运行事项投影到运营频道 `ops-case-card`，打通 E2E-DESSTART 余下断言与 E2E-DESOPS。
