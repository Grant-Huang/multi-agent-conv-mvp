# World Adapter M0-6 准出证据（H1～H4）

分支：`cursor/wa-m0-6-exit-checklist-f2df`（基于已合入的 M0-5 / #173）。

## 范围

- Mock 替换台账与钩子：[WA_MOCK_REGISTRY.md](../../mom/WA_MOCK_REGISTRY.md)
- 新合同：`RT-WA-09`（默认关闭 `world_mock`；未挂载 404；挂载后 `protocol_version=1`）
- 全量回归：`./scripts/wa-m0-suite.sh`（RT-WA-01～09 + DES* + WABIND + WACHANNEL）
- H4 覆盖矩阵：注册表 §4

## 准出检查

| # | 硬约束 | 结论 | 证据 |
|---|---|---|---|
| H1 | 完整前端（DES 实验 + 运营频道 + 三入口工作台 / 事项） | **通过** | `DesExperiment`、`OpsCards`、`MomWorkbench`；`E2E-DES*` / `E2E-WABIND` / `E2E-WACHANNEL` |
| H2 | 同一前端 Mock 走完全流程；E2E = Feature Map + Playwright | **通过** | 套件 9 RT + 8 Playwright 全绿（见下） |
| H3 | Mock 开发与取代计划；可演示替换钩子 | **通过** | `WA_MOCK_REGISTRY.md`；`RT-WA-09`；`[mom] world_mock` |
| H4 | WA-01～08、UI01～16、DES-UI-01～06 均有可执行断言；剩余 Mock 路径保留 | **通过（M0 适用）** | 矩阵见注册表 §4；`E2E-WA-09/10` 与深度争用/客诉/生产确认属 M3/M4，已标保留 |

## 回归结果

| 层 | 状态 |
|---|---|
| RT-WA-01～09 | 9 passed |
| E2E-DESTRIAL～DESLOOP | 6 passed |
| E2E-WABIND | passed |
| E2E-WACHANNEL | passed |
| `npm run test:map` | Feature map valid |

## 命令

```text
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome ./scripts/wa-m0-suite.sh
# → test result: ok. 9 passed (RT-WA)
# → 8 passed (Playwright)
# → wa-m0 suite green

cd web && npm run test:map
# → Feature map valid
```

## 非目标

不接真实 DES MCP；不改 `engine.rs`；不登记生产写 / 客诉移交完整浏览器用例（M3/M4）；不退役 `/mom/` 沙盘。
