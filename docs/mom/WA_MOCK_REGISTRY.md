# World Adapter Mock 注册表与替换钩子

承接 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) §4 与硬约束 H3。本表是 WA 协议 Mock 的唯一替换台账；产品壳通用 Mock 见 [ai-desktop/MOCK_REGISTRY.md](../ai-desktop/MOCK_REGISTRY.md)；领域夹具见 [MOCK_REGISTER.md](MOCK_REGISTER.md)。

## 1. 替换钩子（可演示）

| 钩子 | 行为 | 演示 / 断言 |
|---|---|---|
| `[mom] world_mock`（默认 `false`） | `true` 且 loopback 时 `main` 挂载 `/api/world/mock/*`；关闭则路由不存在（404） | `RT-WA-09`；浏览器套件经 `waMockServer` 写 `world_mock=true` |
| `/api/world/des`（同门控） | `world_mock=true` 时挂载；`[mom] des_mcp` / `AN_MOM_DES_MCP` 非空则真 `DesConnector`+`with_des`，否则内存桩 | `RT-WADES-*`（含 live）、`E2E-WADES*`；`wa-m1-suite` |
| `[mom] des_mcp`（默认空） | 主应用 MOM `with_des`；与 `/api/world/des` 共用 Engine | T-50 / `WA_DES_CONNECTOR_PLAN`；`rt_wades_*_live_*` |
| 响应 `protocol_version` | Mock 与未来真实适配器共用合同字段 | `RT-WA-01～08` / `RT-WADES-*` 响应含 `protocol_version=1` |
| 卡片 `provenance` | Mock 正式运行 `OpsCase.provenance=world_mock`；DES 正式运行 `provenance=des`；工作台种子区分 `mock` / `des` / `production` | `E2E-DES*`、`E2E-WABIND`、`RT-WADES-08`；UI 来源标签 |
| 测试控件 | `WorldMockState::new(true)` / DES `with_test_controls` 才开放 fault/中断注入；生产挂载关闭 | `RT-WA-06`、`RT-WA-07`、`RT-WADES-05` |

替换五步（与 AI Desktop 注册表一致）：冻结契约 → Mock/真实对测同一套件 → 混合模式全量 Playwright → 验持久性/副作用 → 从真实路径删替身并更新本表。

## 2. 注册表（PLAN §4）

| ID | Mock | M0 行为 | 替换时机 | 替换验收 | 状态（M0 / M1） |
|---|---|---|---|---|---|
| WAM-01 | 实验控制与消息 | 试跑/正式分登记；时钟；幂等启动；事项投递 | **M1** 读与观察；**M2** 真实回执 | `wa-m0-suite` + `wa-m1-suite` | **写：M0 Mock**；**读：M1 + T-50**（`/api/world/des`；主应用可 `with_des`） |
| WAM-02 | 世界与权限 | 两身份、两世界、跨范围拒绝 | **M1** DES 绑定；**M3** 生产权限 | 列表/详情/计数/隔离合同不变 | **M0 Mock** + **M1** `E2E-WADES` / `RT-WADES-01`（含 live bind） |
| WAM-03 | 读与事件 | 快照、有序事件、游标过期 | **M1** MOM 投递；**M3** 多源水位 | 续读无漏、无重复业务应用 | **M0 Mock** + **M1/T-50** 正式投递水位（桩 + tick 投影；`RT-WADES-02/03/05/06`、`E2E-WAREAD`） |
| WAM-04 | 方案与批准 | 版本、角色；审批走运营频道卡 | **M2** 复用 MOM；**M4** 真实审批 | 过期批准零写入 | **M0 Mock**；**M2-1 Red**（见 [WA_M2_TASKS.md](WA_M2_TASKS.md) / `wa-m2-1-red.md`） |
| WAM-05 | 写入、回执、取消 | 稳定 execution_id、丢回执、部分失败 | **M2** DES；**M4** MES | 重复执行、终态保护 | **M0 Mock**；**M2-1 Red**（`RT-WADES-09～13` 表格已冻） |
| WAM-06 | 审核与诊断 | 标准答案仅审核可见 | **M2** DES 审核源 | 访问边界、刷新失败恢复 | **M0 Mock**；**M2-1 Red**（`RT-WADES-14` / `E2E-WADYN`） |
| WAM-07 | 生产来源 | 模拟站点/延迟（种子标识） | **M3** 读；**M4** 写 | 对账可复验 | **M0 种子标识**；完整路径 **M3/M4** |

退役真实路径上的 Mock 路由时：关掉 `world_mock`、保留隔离套件作回归、更新本表状态为「已替换」并附 PR。

## 3. 证据入口

- 套件：`./scripts/wa-m0-suite.sh` / `cd web && npm run test:wa-m0`；M1：`./scripts/wa-m1-suite.sh` / `npm run test:wa-m1`
- 准出：`docs/testing/evidence/wa-m0-6.md`（H1～H4；M0 已关闭）；M1：`docs/testing/evidence/wa-m1-5.md`
- M1 读路径替换任务卡：[WA_M1_TASKS.md](WA_M1_TASKS.md)（M1-0～5 已关闭；T-50 #187）
- M2 写路径替换任务卡：[WA_M2_TASKS.md](WA_M2_TASKS.md)（M2-0 完成；M2-1 Red 进行中，见 [WA_M2_1_PLAN.md](WA_M2_1_PLAN.md)）
- 覆盖矩阵：本文 §4 与 [world-adapter Feature Map](../feature-map/features/world-adapter.md)

## 4. M0 设计清单 → 可执行断言（H4）

| 设计项 | M0 可执行断言 | 备注 |
|---|---|---|
| DES-UI-01～06 | `E2E-DESTRIAL`～`E2E-DESLOOP` | 全绿；DESLOOP 双上下文 |
| WA-01 | `E2E-WABIND` | 三入口同 case/world |
| WA-02 | `RT-WA-03` | 续读/游标；完整断线 UI 随 M1 |
| WA-03 | `E2E-WABIND`（资源入口切片） | 刀具/首件完整条件随后续登记 |
| WA-04 | `E2E-DESOPS` + `RT-WA-04` | 方案/执行证据；节点通俗解释随后续 |
| WA-05 | `E2E-WACHANNEL` + `E2E-DESOPS` | 冲突旧授权随 M1+ |
| WA-06 | `RT-WA-06` | 浏览器查证路径随后续 |
| WA-07 | `E2E-DESLOOP` + `RT-WA-07`/`08` | 物理动态随 M2 |
| WA-08 | `E2E-DESISO` + `RT-WA-05` | 会话草稿/小屏完整 UI 随后续；小屏基线见 `E2E-MOBILE` |
| UI01、02、16 | `E2E-WABIND`；小屏 `E2E-MOBILE` | 返回保持范围 |
| UI03、05、06 | `E2E-WABIND` / `E2E-DESPAUSE` | 深度筛选/缺口随后续 |
| UI04、07、08、09 | `E2E-DESOPS` / `E2E-DESLOOP` | 模板来源细节随后续 |
| UI10、15 | `RT-WA-06`；provenance 标签 | 混合来源可辨 |
| UI11 | Mock 路径保留 | 多事项争用完整 E2E 后续登记 |
| UI12 | Desktop AI 入口存在；模型故障深度随后续 | 与 AI Desktop 共用 |
| UI13、14 | `E2E-COMPLAINT`（mom 客诉模拟） | 生产关闭/移交属后续 |
| Feature Map `E2E-WA-09/10` | 未登记正式 id | 客诉移交 / 生产确认属 M3/M4；Mock 路径保留 |
