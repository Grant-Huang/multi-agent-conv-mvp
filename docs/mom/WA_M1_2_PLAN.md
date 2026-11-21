# M1-2 方案：正式投递水位（服务层）

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) M1-2 与 [WORLD_ADAPTER.md](WORLD_ADAPTER.md) §6.1。**不改** `engine.rs` / `store.rs` / `orchestrator.rs` / `DesConnector` 内部。

## 缺口（相对 #180）

`mom_world_des` 已有 pending→published，但缺：

1. **快照与事件同一边界**：Observe 返回的 `objects`/`as_of` 须与已发布事件水位一致。
2. **投递中断**：step 在「写入 pending」与「发布」之间可中断；中断后 Observe/Subscribe 仍只见最近完整边界，pending 不可见。
3. **事件 ID 去重**：同 ID 重入队不重复出现在 published。

## 推荐实现（仅 `mom_world_des` + 测试）

| 改动 | 说明 |
|---|---|
| `Store` | 增加 `snapshots: world_id → SnapshotRec`；`publish_generation` |
| `experiment/step` | 拆成 ingest（pending）与 `commit_publish`；默认一步完成；测试控件可只 ingest |
| `POST .../test/abort-before-publish` | 仅 `allow_test_controls`：完成 ingest 后不发布 |
| Observe | 返回 `events`（已发布）+ `objects`/`as_of` 来自同边界快照 |
| 去重 | publish 时按 `event_id` 跳过已有 |

## 明确不改

- `DesConnector::{tick,spawn,…}` 与引擎状态机
- 生产默认打开故障控件
- Mock 写路径

## 验收

- 新合同 `RT-WADES-05`（中断边界）、`RT-WADES-06`（快照对齐 + 去重）绿
- 既有 `RT-WADES-01`～`04`、`wa-m0-suite`、`wa-des-remote` 仍绿
- `npm run test:map` 绿

## 风险与回滚

- L2：`crates/server/src/mom_world_des.rs`
- 回滚：revert 本 PR
