# M3-1 / Green 方案：生产只读 Bind–Observe–Subscribe（高风险先批）

承接 [WA_M3_TASKS.md](WA_M3_TASKS.md) M3-1、[WORLD_ADAPTER.md](WORLD_ADAPTER.md) §3 / §4.1～4.2 / §7、CASES 已冻结的 `E2E-WAPROD`/`WAPISO`/`WAPREAD`/`WAPDELAY` 与 `RT-WAPROD-01`～`06`。  
**本文件定 Green 路径；M3-1 Red 卡只冻结表格与空壳，不实现。** 改 `engine` / `store` 表结构 / `orchestrator` / 对外凭据与 egress / 生产身份映射前须再批。

**状态：助手建议「服务层 `/api/world/prod` + 生产 stub 先于真站点」为 Green 默认路径（2026-11-22）；待用户确认。偏离该路径须先对照规范说明。M3-1 本卡仍只到 Red，实现另开 M3-2+。**

## 1. 现状缺口

| 路径 | 现状 |
|---|---|
| `/api/world/mock` | M0 读/写合同仍在；回归保留 |
| `/api/world/des` | M1 读 + M2 写已准出 |
| `/api/world/prod` | **不存在**（Red：bind/observe/subscribe/propose → 404） |
| 工作台生产世界 | `MomWorkbench` 种子仅有 mock/des；`provenance=production` 标签已有，无生产世界选项 |
| WAM-07 | M0 种子标识；M3 读替换尚未接线 |

## 2. 推荐实现（服务层优先）

1. 新增 `mom_world_prod`（或同前缀子模块），路由前缀 `/api/world/prod`，字段对齐 `/api/world/des` 的 Bind / Observe / Subscribe（`protocol_version`、`world_id`、`provenance=production`、`as_of`、`cursor`、`source`）。
2. **生产 stub 先于真站点**：内存/配置表登记站点权威源；未配置权威源 → 明确错误（非 200）（`RT-WAPROD-01`）。
3. **Observe / Subscribe（RT-WAPROD-02/04/05）**：水位含 `source` / `as_of`；过期游标 → `CURSOR_EXPIRED`；恢复后续读按 `event_id` 去重；测试控件注入多源延迟（**仅** `allow_test_controls`，生产配置默认关闭）。
4. **权限 / 跨范围（RT-WAPROD-03 / E2E-WAPISO）**：无授权或跨站点 → `ACCESS_DENIED`；列表/详情不泄露其他站点。
5. **写关闭（RT-WAPROD-06）**：生产世界 Propose/Execute **不挂载或明确拒绝**；Mock/DES 写路径继续可用。
6. **前端 / E2E**：工作台增加生产世界种子；看板/观测显示 `provenance=production` 与水位；写入口关闭或跳转隔离 Mock/DES（`E2E-WAPROD`/`WAPREAD`/`WAPDELAY`）。
7. **门控**：与 mock/des 同属 `[mom] world_mock` loopback 演示门控，或独立 `world_prod_stub` 开关（Green 卡定稿）；真站点凭据与 egress **不进本默认路径**。

## 3. 明确不改（除非升级方案再批）

- `engine.rs` / `orchestrator.rs` / `registry.rs` / `store` 表结构
- `DesConnector` / `mom_world_des` 既有读/写语义（只并存，不改）
- 真实 MES/ERP 凭据、egress allowlist、生产身份映射（属接真站点升级方案）
- 生产写 / WA-06（M4）
- 弱化 M0～M2 已冻结断言；退役 `/api/world/mock` 或 `/api/world/des`

若 Red→Green 证明必须改 store 新表或接真实 egress，先停下来升级本方案再动手。

## 4. 风险与回滚

| 项 | 说明 |
|---|---|
| 风险 | L2：`crates/server/src/**`、可能 `main`/配置；误接凭据/egress 升 L2/L3 |
| 回滚 | 去掉 `/api/world/prod` 路由与 stub；读路径继续 Mock/DES；revert Green PR |

## 5. 验收（后续卡）

| 卡 | 验收 |
|---|---|
| M3-2 | `RT-WAPROD-01/02` Green；bind/observe/subscribe 服务层入库 |
| M3-3 | `RT-WAPROD-03/04/05/06` Green（权限、续读、延迟、写关闭） |
| M3-4 | Playwright `E2E-WAPROD`/`WAPISO`/`WAPREAD`/`WAPDELAY` + M0～M2 全量回归 |
| M3-5 | 证据 / WAM-07 状态 / 准出 |

## 6. 请批示

**推荐批准「服务层 `/api/world/prod` + 生产 stub，不改 engine/store/凭据 egress」作为 Green 默认路径。**  
M3-1 Red 本 PR 只交表格 + Playwright 空壳 + 本地 Rust Red 证据 + 本方案。
