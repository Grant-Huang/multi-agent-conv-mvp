# World Adapter M1-1 — 正式 CASES + 业务 Red

分支：`cursor/wa-m1-1-red-cases-f2df`。未改 `DesConnector` / `engine.rs`。

## 范围

- CASES 正式表格：`E2E-WADES` / `E2E-WAREAD` / `E2E-WAOBS`、`RT-WADES-01`～`04`
- Playwright 空壳：`web/e2e/wa-des-remote.spec.ts`（已挂 `features.json`）
- `RT-WADES-*`：表格已冻结；HTTP 空壳曾对 `/api/world/des` 打出业务 Red（见下），**未入库**以免 PR CI `cargo test` 红（对齐 M0-0：PR CI 不跑 Playwright，Rust Red 随 Green 卡再挂 `mom_world_des.rs` + `runtime_cases`）

## Red（实现前）

均为**业务断言失败**，非编译或环境超时。

### E2E-WADES / WAREAD / WAOBS（留在树内）

```text
$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome npx playwright test e2e/wa-des-remote.spec.ts
3 failed（缺 data-provenance=des、重新读取/水位控件、adapter-mode=des 等）
```

### RT-WADES-01～04（本地空壳跑通 Red 后不入库）

当时对空 Router 请求 `/api/world/des/*`：

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture
rt_wades_01 … FAILED — 远程 DES 绑定合同尚未挂载（期望 200 + provenance=des）；got 404
rt_wades_02 … FAILED — Observe 合同尚未挂载；got 404
rt_wades_03 … FAILED — Subscribe 合同尚未挂载；got 404
rt_wades_04 … FAILED — 远程试跑合同尚未挂载；got 404
test result: FAILED. 0 passed; 4 failed
```

Green 卡将恢复 `crates/server/tests/mom_world_des.rs` 并写入 `features.json` 的 `runtime_cases`。

### 地图

```text
$ cd web && npm run test:map
# → Feature map valid（含新增 3 个 E2E）
```

## 转绿依赖（下一步方案要点）

| 项 | 建议 | 风险 |
|---|---|---|
| 新路由 `/api/world/des/*` | 服务层新模块（类比 `mom_world_mock`），`main` 在 DES 连接可用时 merge | L2（`crates/server/src/**`） |
| 复用 `DesConnector` | `tick` / 创建仿真；公开读走 MOM 投递边界 | 不改连接器内部则避免子进程高风险改动 |
| `engine.rs` | **默认不动**；若必须改投递水位，先单独方案 | 高风险，先停下来问人 |
| 前端 | 工作台 DES 世界、`data-provenance`、续读/观测 testid | L1（`web/**`） |
| 写路径 | 仍 `/api/world/mock` | — |

## 非目标

本卡不完成 Green；不接真实写；不弱化 M0。
