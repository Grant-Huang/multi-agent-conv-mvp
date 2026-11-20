# World Adapter M0-5 证据

分支：`cursor/wa-m0-5-playwright-suite-f2df`（基于已合入的 M0-4）。

## 范围

- 共用 Mock harness：`web/e2e/helpers/waMockServer.ts`
- `E2E-DESLOOP`：实验员 / 运营员双浏览器上下文（Then 断言未弱化）
- 新用例 `E2E-WACHANNEL`：工作台 → 前往运营频道
- 集成合同套件：`./scripts/wa-m0-suite.sh`（RT-WA-01～08 + DES* + WABIND + WACHANNEL）
- npm：`cd web && npm run test:wa-m0`

## 结果

| 层 | 状态 |
|---|---|
| RT-WA-01～08 | 8 passed |
| E2E-DESTRIAL～DESLOOP | 6 passed（DESLOOP 双上下文） |
| E2E-WABIND | passed |
| E2E-WACHANNEL | passed |

## 命令

```text
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome ./scripts/wa-m0-suite.sh
# → test result: ok. 8 passed (RT-WA)
# → 8 passed (Playwright DES* + WABIND + WACHANNEL)
# → wa-m0 suite green
```

## 非目标

不接真实 DES MCP；不改 `engine.rs`；WA-02～04/06～10 全量与 H1～H4 准出清单属 M0-6。

