# M1-3 方案：实验控制与运营频道投影（服务层）

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) M1-3。**不改** `engine.rs` / `DesConnector` / `orchestrator`。

## 目标

1. 试跑：`delivery_count=0`，**不**向运营频道投递事项卡。
2. 正式运行：`delivery_count≥1` 且有 `case_id`；向运营频道投影 `OpsCase`（`provenance=des`）。
3. 实验推进：既有 `/experiment/step` 保持可用（M1-1/2）。

## 实现

| 项 | 做法 |
|---|---|
| `WorldDesState` | 可选 `app_store: Option<Arc<Store>>`（同 Mock） |
| `start_trial` | 不调用运营投影 |
| `start_official` | 有 `app_store` 时 `ensure_conversation` + `OpsCase` 卡 |
| `main` | `world_mock` 门控下 `with_app_store(rt.store)` |
| 合同 | `RT-WADES-07` 试跑隔离；`RT-WADES-08` 正式投递运营频道 |

## 非目标

- 真 DesConnector 创建仿真
- Propose/Execute 真写（M2）
- 改引擎状态机

## 验收

- `cargo test -p an-server --test mom_world_des` 全绿（含 07/08）
- `wa-m0-suite` 仍绿
- `npm run test:map` 绿
