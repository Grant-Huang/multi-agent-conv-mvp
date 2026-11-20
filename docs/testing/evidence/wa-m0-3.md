# World Adapter M0-3 — 运营频道联动

## 范围

- `mom_world_mock`：正式运行向 `ops-channel` 投影 `OpsCase`（`provenance=world_mock`）
- `OpsCaseCard`：`data-testid=ops-case-card`；查看方案 / 批准 / 查看执行 / 查看核验证据
- `/api/world/mock/ops/approve`、`ensure-seed`；DESISO 拒绝桩 `/api/world/approve`、`/api/world/runs/{id}`
- 桌面「运营频道」入口；实验页返回链路（运营员视图隐藏 DES 入口）
- `des-experiment.spec.ts`：独立实例 `18792` + `world_mock`（断言未改）

## Red → Green

| Case | 状态 |
|---|---|
| E2E-DESTRIAL | Green（保持） |
| E2E-DESSTART | Green |
| E2E-DESOPS | Green |
| E2E-DESPAUSE | Green |
| E2E-DESISO | Green |
| E2E-DESLOOP | Green |

```text
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/bin/google-chrome-stable \
  npx playwright test e2e/des-experiment.spec.ts
  6 passed
```

## 非目标

不接真实 DES MCP；不改 `engine.rs`；三入口工作台属 M0-4。
