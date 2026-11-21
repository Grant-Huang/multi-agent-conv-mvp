# 主应用 DesConnector / `with_des` 服务层补接

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 用户确认「服务层优先」 |
| 状态 | 有效 |
| 相关 | [WA_DES_CONNECTOR_PLAN.md](../mom/WA_DES_CONNECTOR_PLAN.md)、[wa-des-connector.md](../testing/evidence/wa-des-connector.md)、T-50 |

- **问题**：M1 `/api/world/des` 为独立内存桩，与 MOM `Engine` 分叉；`an-mom --des-mcp` 已可 `with_des`，主应用未接。
- **定了什么**（用户确认方案后开工）：
  - `[mom] des_mcp`（默认空；亦可 `AN_MOM_DES_MCP`）；非空则 `DesConnector::spawn` → `AppState::with_des`，与 World Adapter **共用同一 Engine**。
  - 有连接器时：bind/trial/official → `create_simulation`；step → `tick`；Observe 投影 tick 成功后的 Engine 边界；半提交测试控件拒用。
  - 无连接器且 `world_mock`：保持内存桩回归。
  - **不改** `engine` 状态机、`DesConnector` 内部、`store`/`orchestrator`；M2 写路径另卡。
- **为什么**：消灭双引擎分叉，复用已验收连接器公共 API，风险可控；新决策写单文件，避免与 `DECISIONS.md` 第 69 条（C1）撞号。
- **没选什么**：本卡拆改 `tick` 投递原子性；本卡做 Propose-Execute 写路径；继续在 `DECISIONS.md` 末尾追加编号。
- **出问题先看**：`docs/mom/WA_DES_CONNECTOR_PLAN.md`；`mom_world_des` 桩 vs `with_connector`；`des_mcp` 启动失败是否退出。
