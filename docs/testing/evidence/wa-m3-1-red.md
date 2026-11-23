# World Adapter M3-1 — 正式 CASES + 业务 Red

分支：`cursor/wa-m3-1-red-cases-f2df`。未改 `DesConnector` / `engine.rs` / `orchestrator.rs` / `store` 表结构 / 凭据 egress。

## 范围

- CASES 正式表格：`E2E-WAPROD` / `E2E-WAPISO` / `E2E-WAPREAD` / `E2E-WAPDELAY`、`RT-WAPROD-01`～`06`（对齐 M3-0 冻结草案）
- Playwright 空壳：`web/e2e/wa-prod-read.spec.ts`（已挂 `features.json`）
- 高风险 Green 方案：[`docs/mom/WA_M3_1_PLAN.md`](../../mom/WA_M3_1_PLAN.md)（服务层 `/api/world/prod` + 生产 stub；助手建议待批）
- 决策草案：[`docs/decisions/2026-11-22-wa-m3-1-prod-read-service-layer.md`](../../decisions/2026-11-22-wa-m3-1-prod-read-service-layer.md)
- `RT-WAPROD-01`～`06`：表格已冻结；HTTP 空壳曾对 `/api/world/prod/{bind,observe,subscribe,…}` 打出业务 Red（见下），**未入库**以免 PR CI `cargo test` 红（对齐 M1-1 / M2-1）

## Red（实现前）

均为**业务断言失败**，非编译或环境超时。

### E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY（留在树内）

空壳断言生产 Bind/Observe、跨站拒绝、续读水位、多源延迟控件。当前生产路由未挂，业务 Red 预期失败（PR CI 不跑 Playwright）。

```text
$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome \
    npx playwright test e2e/wa-prod-read.spec.ts
# 期望：4 failed（/api/world/prod/* 404 / 缺生产世界选项与 UI 控件）
```

### RT-WAPROD-01～06（本地空壳跑通 Red 后不入库）

对仅挂 DES 路由的 `WorldDesState` Router 请求生产前缀：

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_prod_red -- --nocapture
rt_waprod_01_bind_requires_prod_route … FAILED
  — 生产 Bind 合同尚未挂载（期望 200 + world_id）；got 404 Not Found
rt_waprod_02_observe_watermark … FAILED
  — 生产 Observe 合同尚未挂载（期望 200 + as_of/source）；got 404 Not Found
rt_waprod_03_access_denied_cross_scope … FAILED
  — 生产权限合同尚未挂载（期望 ACCESS_DENIED）；got 404 Not Found
rt_waprod_04_cursor_expired … FAILED
  — 生产续读合同尚未挂载（期望 CURSOR_EXPIRED）；got 404 Not Found
rt_waprod_05_inject_delay … FAILED
  — 生产延迟测试控件尚未挂载；got 404 Not Found
rt_waprod_06_write_closed_on_prod … FAILED
  — 生产写关闭合同尚未挂载（期望拒绝写）；got 404 Not Found
test result: FAILED. 0 passed; 6 failed
```

Green 卡（M3-2+）将恢复 `crates/server/tests/mom_world_prod.rs` 并写入 `features.json` 的 `runtime_cases`。

### 地图

```text
$ cd web && npm run test:map
# → Feature map valid: 32 features, 71 browser cases
# （含新增 4 个 E2E；不含未入库 RT Rust）
```

## 转绿依赖（下一步方案要点）

见 [`WA_M3_1_PLAN.md`](../../mom/WA_M3_1_PLAN.md)。摘要：

| 项 | 建议 | 风险 |
|---|---|---|
| `/api/world/prod/bind\|observe\|subscribe` | 服务层新 `mom_world_prod`；生产 stub；延迟测试控件 | L2（`crates/server/src/**`） |
| 真站点 / 凭据 / egress | **默认不接**；升级方案再批 | L2/L3 |
| `engine.rs` / `store` / `orchestrator` | **默认不动** | 高风险，先停下来问人 |
| 前端 | 生产世界种子；`provenance=production`；写入口关闭 | L1（`web/**`） |
| Mock/DES | 保留作回归与写演示 | — |

## 非目标

本卡不完成 Green；不接真实 MES；不弱化 M0～M2。
