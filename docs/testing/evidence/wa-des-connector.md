# T-50 证据：DesConnector / `with_des` 补接

分支：`cursor/wa-des-connector-wire-f2df`。方案：`docs/mom/WA_DES_CONNECTOR_PLAN.md`（用户确认「服务层优先」）。决策：`docs/decisions/2026-11-21-wa-des-connector-with-des.md`。

## 改动摘要

- `[mom] des_mcp`（默认空；亦可 `AN_MOM_DES_MCP`）
- `main`：共用 Engine；非空则 `DesConnector::spawn` → `AppState::with_des`；`world_mock` 下 WorldDes 走 `with_connector` 或桩
- `mom_world_des`：有连接器时 `create_simulation` / `tick`；Observe 投影 tick 成功后 Engine 边界；半提交控件拒用
- **未改** `engine.rs` / `DesConnector` 内部 / `store` / `orchestrator`

## 合同

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture
# → 12 passed（RT-WADES-01～08 桩 + live bind/tick + des_mcp 默认空 + abort 拒用）
```

## 非目标

- M2 写路径（Propose-Execute / `dispatch_writes` 产品前门）
- 拆改 `DesConnector::tick` 投递原子性
- E2E 真 `des_mcp` harness（合同层已覆盖；浏览器仍用桩 harness）
