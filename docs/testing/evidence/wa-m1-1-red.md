# World Adapter M1-1 — 正式 CASES + 业务 Red

分支：`cursor/wa-m1-1-red-cases-f2df`。未改 `DesConnector` / `engine.rs`（接线前须另出方案）。

## 范围

- CASES 正式表格：`E2E-WADES` / `E2E-WAREAD` / `E2E-WAOBS`、`RT-WADES-01`～`04`
- Playwright 空壳：`web/e2e/wa-des-remote.spec.ts`
- HTTP 合同空壳：`crates/server/tests/mom_world_des.rs`（约定前缀 `/api/world/des`）
- `features.json` / Feature Map / `WA_M1_TASKS` 同步

## Red（实现前）

均为**业务断言失败**（缺远程 DES 合同/UI），非编译或环境超时。

### RT-WADES-01～04

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture
rt_wades_01 … FAILED — 远程 DES 绑定合同尚未挂载（期望 200 + provenance=des）；got 404
rt_wades_02 … FAILED — Observe 合同尚未挂载；got 404
rt_wades_03 … FAILED — Subscribe 合同尚未挂载；got 404
rt_wades_04 … FAILED — 远程试跑合同尚未挂载；got 404
test result: FAILED. 0 passed; 4 failed
```

### E2E-WADES / WAREAD / WAOBS

```text
$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome npx playwright test e2e/wa-des-remote.spec.ts
3 failed（缺 data-provenance=des、重新读取/水位控件、adapter-mode=des 等）
```

### 地图

```text
$ cd web && npm run test:map
# → Feature map valid（含新增 3 E2E + 4 RT）
```

## 转绿依赖

服务层挂载 `/api/world/des`（复用 `with_des` / `DesConnector`）；工作台 DES 世界与续读/观测控件。**改 `engine.rs` 或连接器状态机前先出方案**（`AGENTS.md`）。写路径仍走 `/api/world/mock`。

## 非目标

不接真实写；不弱化 M0 断言；本卡不完成 Green。
