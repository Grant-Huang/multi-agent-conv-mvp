# T-50 方案：DesConnector / `with_des` 补接（高风险先批）

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) 准出缺口 Q1、[WORLD_ADAPTER.md](WORLD_ADAPTER.md) §6.1、父项 T-49。  
**状态：已批准（服务层优先）并实施。** 不改 `engine` / `DesConnector` 内部 / `store` 表结构。

## 1. 现状缺口

| 路径 | 现状 |
|---|---|
| `an-mom --des-mcp` | 已 `DesConnector::spawn` + `AppState::with_des`（RT-DESC-*） |
| 主应用 `agentnexus` | MOM `AppState` **未** `with_des`；`/api/world/des` 为 **独立内存桩**（`WorldDesState`），与 MOM `Engine` **不同实例** |
| 公开读合同 | M1 桩自维护 `publish_seq` / pending→commit；与 `DesConnector::tick`（`events_poll`→`des_ingest_remote`→`advance`）未打通 |

目标：主应用在配置启用时挂真连接器；World Adapter 读路径创建/推进走 `DesConnector` 公共 API；Observe/Subscribe 暴露 **MOM 投递后** 可见边界（非裸 `events_poll`）。

## 2. 推荐实现（服务层优先）

### 2.1 配置（公开字段，需说明）

`[mom]` 新增可选：

```toml
# 空 = 不挂连接器；非空 = 启动 an-des-mcp（stdio），失败则启动失败或 DES 路由报 E_DES_UNAVAILABLE
des_mcp = ""           # 或命令路径，如 "target/debug/an-des-mcp"
# 亦可读环境变量 AN_MOM_DES_MCP（与 an-mom 对齐）
```

默认空：行为与今相同（world_mock 时仍可挂桩 DES，便于 M0/M1 回归）。

### 2.2 `main.rs` 接线

1. loopback + MOM 启用时：创建 **同一** `Arc<Mutex<Engine>>`。
2. 若 `des_mcp` 非空：`DesConnector::spawn` → `state = state.with_des(connector.clone())`；管理 token 随机、不落盘（同 `an-mom`）。
3. `WorldDesState` 改为携带：
   - `app_store`（已有）
   - `engine: Option<Arc<Mutex<Engine>>>`（与 MOM 共用）
   - `connector: Option<Arc<DesConnector>>`
4. `mom::router(state)` 与 `mom_world_des::router(...)` 共用上述 engine/connector。

### 2.3 `/api/world/des` 行为（有连接器时）

| API | 行为 |
|---|---|
| bind / trial / official | 调 `DesConnector::create_simulation`；`world_id` ↔ `simulation_id` 映射；`provenance=des` |
| step | `DesConnector::tick(engine, to_minute)`；**成功后**再投影公开水位 |
| observe / subscribe | 读 **Engine** 已提交事件/案例视图（`Engine::events` / 案例列表等公共 API），映射为 WA 合同字段；**禁止**把 MCP `events_poll` 游标当产品水位 |
| 无连接器 | 保持现有内存桩（`connector_available`）；或 bind 返回 `E_DES_UNAVAILABLE`——**建议**：配置未开 des_mcp 时仍允许桩（回归），显式 `connector_available=false` 测不可用 |

试跑 vs 正式：继续服务层语义（试跑 `delivery_count=0` 且不投 OpsCase；正式 ≥1 且可投 `provenance=des`）。正式运行的引擎侧登记与运营投影仍走现有 `app_store` 路径。

### 2.4 投递边界（不改 engine 状态机的投影策略）

`DesConnector::tick` 已在锁内：`des_ingest_remote` + `Engine::advance`。T-50 **不拆改**该原子性。

- 产品水位：以 **tick 成功返回后** 的 Engine 可见事件为准投影 `publish_seq` / events。
- M1 测试控件 `abort-before-publish`：仅对 **无连接器的桩模式** 保留；真连接器模式下返回 `ACCESS_DENIED` 或 no-op 说明「真实 tick 不可半提交」（写进合同注释，不弱化桩测）。

若后续必须在 ingest 与 advance 之间暴露「中断仍服务上一完整边界」，再单独立项改 `DesConnector::tick` / engine（本卡不做）。

## 3. 明确不改（除非升级方案再批）

- `crates/runtime/src/mom/engine.rs` 状态机 / 表结构
- `DesConnector` 超时、子进程、`ops` 锁、`kill_on_drop` 语义（只调用 `spawn` / `create_simulation` / `tick` / 只读探活）
- `store.rs` / `orchestrator.rs` / `registry.rs`
- M2 写路径（`dispatch_writes` / Propose-Execute）→ **T-51+**

## 4. 风险与回滚

| 项 | 说明 |
|---|---|
| 风险等级 | L2：`main`、`config` 字段、`mom_world_des`；若误改 connector/engine 升 L2/L3 |
| 子进程 | 沿用既有 10s 握手 / 30s 调用 / `kill_on_drop`；主进程退出清理 |
| 双 Engine | 本卡消灭 world_des 与 MOM 双引擎分叉（有 des_mcp 时） |
| 回滚 | 清空 `des_mcp`；world_des 回退桩；revert PR |

## 5. 验收（写清）

**合同 / 回归**

- `cargo test -p an-server --test mom_world_des`：桩模式 RT-WADES-01～08 仍绿
- 新增（或扩展）连接器模式用例：例如 `RT-WADES-01` 在 `with_des` 下 bind 成功；`step` 后 Observe 事件来自 Engine 投影且 `provenance=des`；未配置连接器时不可用路径明确错误
- `cargo test -p an-server --test mom_des_connector` / `mom_des_http_mcp`：既有 RT-DESC 绿（不改 connector 内部）
- `./scripts/wa-m1-suite.sh` 绿（含 M0）

**E2E**

- `e2e/wa-des-remote.spec.ts`：默认 harness 仍绿（桩或同门控）
- 可选：带 `des_mcp` 的隔离 harness 冒烟（若环境可构建 `an-des-mcp`）；否则合同层覆盖即可，E2E 标「连接器模式随后续」

**文档**

- 更新 `WA_MOCK_REGISTRY`：WAM-01/03 注明「主应用可 `with_des`」
- 证据：`docs/testing/evidence/wa-des-connector.md`
- `WA_M1_TASKS` / `wa-m1-5` Q1 交叉引用「已补接」

## 6. 实施顺序（批准后）

1. 配置字段 + `main` 共用 Engine / 可选 spawn  
2. `WorldDesState` 注入 connector+engine；有连接器时 create/tick/投影  
3. 测试：桩回归 + 连接器模式 Red→Green  
4. 套件与证据、开 PR  

## 7. 请批示

**推荐批准本方案（服务层接线，不改 engine/DesConnector 内部）。**  
若要求 T-50 必须改 `tick` 拆分投递边界，请明确升级范围后再动高风险文件。
