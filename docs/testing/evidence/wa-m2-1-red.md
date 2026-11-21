# World Adapter M2-1 — 正式 CASES + 业务 Red

分支：`cursor/wa-m2-1-red-cases-f2df`。未改 `DesConnector` / `engine.rs` / `orchestrator.rs`。

## 范围

- CASES 正式表格：`E2E-WAEXEC` / `E2E-WARECV` / `E2E-WADYN` / `E2E-WALOOP`、`RT-WADES-09`～`14`（对齐 M2-0 冻结草案）
- Playwright 空壳：`web/e2e/wa-des-write.spec.ts`（已挂 `features.json`）
- 高风险 Green 方案：[`docs/mom/WA_M2_1_PLAN.md`](../../mom/WA_M2_1_PLAN.md)（服务层优先；助手建议待批）
- `RT-WADES-09`～`14`：表格已冻结；HTTP 空壳曾对 `/api/world/des/{propose,execute,confirm,…}` 打出业务 Red（见下），**未入库**以免 PR CI `cargo test` 红（对齐 M1-1）

## Red（实现前）

均为**业务断言失败**，非编译或环境超时。

### E2E-WAEXEC / WARECV / WADYN / WALOOP（留在树内）

空壳断言 DES Propose/Execute/Confirm、丢回执控件、工厂重启控件、干预后事件前缀、双角色同一 `execution_id`。当前写路由未挂，业务 Red 预期失败（PR CI 不跑 Playwright）。

```text
$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome \
    npx playwright test e2e/wa-des-write.spec.ts
# 期望：4 failed（/api/world/des/propose|execute|confirm 等 404 / 缺 UI 控件）
```

### RT-WADES-09～14（本地空壳跑通 Red 后不入库）

对已挂读路由的 `WorldDesState` Router 请求写路径：

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture
rt_wades_09_propose_binds_plan_revision … FAILED
  — 远程 DES Propose 合同尚未挂载（期望 200 + proposal_id）；got 404 Not Found
rt_wades_10_execute_idempotent … FAILED
  — 远程 DES Execute 合同尚未挂载（期望 200 + execution_id）；got 404 Not Found
rt_wades_11_cancel_pending_not_dispatched … FAILED
  — 取消守卫依赖 Propose；合同尚未挂载；got 404 Not Found
rt_wades_12_confirm_after_lost_receipt … FAILED
  — 远程 DES Confirm 合同尚未挂载（期望 200 + status）；got 404 Not Found
rt_wades_13_factory_instance_change_rejects_stale … FAILED
  — 工厂重启测试控件尚未挂载；got 404 Not Found
rt_wades_14_intervention_changes_future_events … FAILED
  — 干预闭环依赖 Propose；合同尚未挂载；got 404 Not Found
test result: FAILED. 0 passed; 6 failed
```

Green 卡将恢复 `crates/server/tests/mom_world_des_write.rs`（或扩 `mom_world_des.rs`）并写入 `features.json` 的 `runtime_cases`。

### 地图

```text
$ cd web && npm run test:map
# → Feature map valid（含新增 4 个 E2E；不含未入库 RT Rust）
```

## 转绿依赖（下一步方案要点）

见 [`WA_M2_1_PLAN.md`](../../mom/WA_M2_1_PLAN.md)。摘要：

| 项 | 建议 | 风险 |
|---|---|---|
| `/api/world/des/propose\|execute\|confirm` | 服务层扩 `mom_world_des`；测试控件 lose/restore/restart | L2（`crates/server/src/**`） |
| 复用 `DesConnector::dispatch_writes` | 只调公共 API | 不改连接器内部 |
| `engine.rs` / `store` / `orchestrator` | **默认不动**；若必须改先升级方案 | 高风险，先停下来问人 |
| 前端 | 执行证据 / `execution-id` / provenance=des | L1（`web/**`） |
| Mock 写路径 | 保留作回归 | — |

## 非目标

本卡不完成 Green；不接生产 MES；不弱化 M0/M1。
