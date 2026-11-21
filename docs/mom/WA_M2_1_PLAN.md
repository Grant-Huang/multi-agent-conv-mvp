# M2-1 / Green 方案：远程 DES Propose–Execute–Confirm（高风险先批）

承接 [WA_M2_TASKS.md](WA_M2_TASKS.md) M2-1、[WORLD_ADAPTER.md](WORLD_ADAPTER.md) §4.3～4.5 / §6.2～6.3、CASES 已冻结的 `E2E-WAEXEC`/`WARECV`/`WADYN`/`WALOOP` 与 `RT-WADES-09`～`14`。  
**本文件定 Green 路径；M2-1 Red 卡只冻结表格与空壳，不实现。** 改 `engine` / `DesConnector` 内部 / `store` 表结构 / `orchestrator` 前须再批。

**状态：助手建议（待用户批准后再开 Green 卡）。**

## 1. 现状缺口

| 路径 | 现状 |
|---|---|
| `/api/world/des` | 已有 bind / observe / subscribe / trial / official / step + 测试控件（M1 + T-50） |
| Propose / Execute / Confirm | 仅 `/api/world/mock`；DES 前缀写路由 **404**（Red 证据见 `wa-m2-1-red.md`） |
| 远程写 | `DesConnector::dispatch_writes` 已存在；World Adapter 产品前门未接 |

## 2. 推荐实现（服务层优先）

1. 在 `mom_world_des`（或同前缀子模块）增加 `propose` / `execute` / `confirm`，以及测试控件：`lose_receipt`、`partial_fail`、工厂实例变更/重启模拟。
2. **Propose（RT-WADES-09）**：绑定远程 `case_id` + `plan_revision`（经既有 `select_des_plan` 语义）；修订或前置条件变化后旧批准不可执行 → `APPROVAL_INVALID` / `PRECONDITION_FAILED`。
3. **Execute（RT-WADES-10）**：`(world_id, idempotency_key)` 唯一；同键并发只登记一次业务执行；键同载荷异 → `IDEMPOTENCY_CONFLICT`，可找回原 `execution_id`；调用 MOM `approve`/`execute` + `DesConnector::dispatch_writes`。
4. **取消守卫（RT-WADES-11）**：案例取消后待发子动作不发送；已发迟到回执只更新证据，案例终态保留。
5. **Confirm / 恢复（RT-WADES-12）**：丢回执后 `unknown` 或按原关联键找回；部分失败 `failed` 且含部分成功子动作证据（字段对齐 Mock `RT-WA-04/06`，`provenance=des`）。
6. **工厂重启（RT-WADES-13）**：工厂实例变更后对旧运行再提交失败可见；不向新实例误投。
7. **干预闭环（RT-WADES-14 / E2E-WADYN）**：干预后已发生前缀事件不变、未来事件改变；审核答案与干预一致；运营员/普通会话 Confirm/Observe 不含标准答案。
8. **前端 / E2E**：双角色 + 远程 DES fixture；`E2E-WAEXEC`（授权/并发/取消）、`WARECV`（恢复）、`WADYN`（动态）、`WALOOP`（产品闭环同一 `execution_id`）。

## 3. 明确不改（除非升级方案再批）

- `engine.rs` 状态机 / `des_after_execute_remote` 提交窗口拆分
- `DesConnector` 超时、子进程、`ops` 锁语义（只调用公共 API）
- `orchestrator.rs` / `registry.rs`
- 生产 MES 写（M4）
- 弱化 M0/M1 已冻结断言或退役 `/api/world/mock` 回归路径

若 Red→Green 证明必须改引擎提交边界或 store 新表，先停下来升级本方案再动手。

## 4. 风险与回滚

| 项 | 说明 |
|---|---|
| 风险 | L2：`mom_world_des`、可能 `main`/配置；误改 engine/connector 升 L2/L3 |
| 回滚 | 去掉 DES 写路由；写路径继续 Mock；revert Green PR |

## 5. 验收（Green 卡）

- `cargo test -p an-server --test mom_world_des_write`（或扩 `mom_world_des`）：`RT-WADES-09`～`14` 绿，并挂入 `features.json` `runtime_cases`
- Playwright：`E2E-WAEXEC` / `WARECV` / `WADYN` / `WALOOP` 绿
- `wa-m0-suite` + `wa-m1-suite` 仍绿；RT-DES-08～11 / RT-DESC 相关回归绿

## 6. 请批示

**推荐批准「服务层接线，不改 engine/DesConnector 内部」作为 Green 默认路径。**  
M2-1 Red 本 PR 只交表格 + Playwright 空壳 + 本地 Rust Red 证据。
