# M1-1 Green 方案：`/api/world/des` 接线（待批准）

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) 与 Red 证据 [wa-m1-1-red.md](../testing/evidence/wa-m1-1-red.md)。**本文件是方案，不是已批准开工令。**

## 目标

让 `RT-WADES-01`～`04` 与 `E2E-WADES` / `WAREAD` / `WAOBS` 转绿；写路径继续走 Mock。

## 推荐实现（服务层优先）

1. **新增** `crates/server/src/mom_world_des.rs`：路由前缀 `/api/world/des`，合同字段对齐 `mom_world_mock`（`protocol_version`、`provenance=des`、cursor/events、trial/official）。
2. **`main.rs`**：仅当配置了可用 DES MCP（或测试注入的 `DesConnector`）时 `merge` 该路由；默认生产不挂。
3. **读路径**：创建/推进调用既有 `DesConnector::{spawn,tick,…}`；Observe/Subscribe 只暴露 **MOM 正式投递后** 的快照与事件（内部可用 `events_poll` 搬运，不把工厂游标当产品水位）。
4. **前端**：工作台增加可辨 DES 世界；testid：`data-provenance`、`event-watermark`、`applied-event-ids`、`cursor-expired-banner`、`formal-observation`、`adapter-mode`。
5. **测试**：恢复 `crates/server/tests/mom_world_des.rs` 入 `features.json` `runtime_cases`；Playwright 继续用独立 harness。

## 明确不改（除非另批）

- `crates/runtime/src/mom/engine.rs` 状态机
- `store.rs` / `orchestrator.rs` / `registry.rs`
- `DesConnector` 超时、子进程、锁语义（只调用公共 API）

若证明必须改引擎投递边界，先停下来用本文件升级方案再动手。

## 风险与回滚

- 风险：L2（`crates/server/src/**` 新模块 + `main` merge）。
- 回滚：去掉 merge、删除 `mom_world_des` 路由模块；前端控件可留但隐藏。

## 验收

- `cargo test -p an-server --test mom_world_des` 4 passed
- `playwright test e2e/wa-des-remote.spec.ts` 3 passed
- `./scripts/wa-m0-suite.sh` 仍绿
- `npm run test:map` 绿
