# Evidence: WA M1-4 远程 DES E2E + M0 回归

## Scope

- 方案：`docs/mom/WA_M1_4_PLAN.md`
- 工作台 DES 读水位 → `/api/world/des`（bind / step / observe / subscribe）
- 浏览器：`E2E-WADES` / `WAREAD` / `WAOBS`
- 套件：`scripts/wa-m1-suite.sh`（含 `wa-m0-suite`）
- **未改** `engine.rs` / `DesConnector` / `orchestrator`

## Commands（摘录）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des
test result: ok. 8 passed; 0 failed

$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome \
    npx playwright test e2e/wa-des-remote.spec.ts
3 passed

$ ./scripts/wa-m0-suite.sh
RT-WA 9 passed；Playwright DES*/WABIND/WACHANNEL 8 passed
== wa-m0 suite green ==

$ npm run test:map && npm run build
Feature map valid；vite build ok
```

## Notes

- 写路径仍 Mock（`waMockServer` / `DesExperiment` → `worldMock`）。
- 注册表 WAM-01～03 已标 M1 读路径状态。
- 套件入口：`./scripts/wa-m1-suite.sh` / `npm run test:wa-m1`。
