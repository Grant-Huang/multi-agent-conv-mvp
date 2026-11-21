# Evidence: WA M1-3 实验控制与运营频道投影

## Scope

- 方案：`docs/mom/WA_M1_3_PLAN.md`
- 服务层：`WorldDesState::with_app_store`；试跑零运营投影；正式运行 `OpsCase`（`provenance=des`）
- 合同：`RT-WADES-07`、`RT-WADES-08`
- **未改** `engine.rs` / `DesConnector` / `orchestrator`

## Commands

```bash
CARGO_TARGET_DIR=target cargo test -p an-server --test mom_world_des --locked
```

## Notes

- `main`：`world_mock` 门控下 DES 路由挂 `rt.store`，与 Mock 同源运营频道。
- 写路径仍走 `/api/world/mock`；本卡仅正式运行投影。
