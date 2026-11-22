# World Adapter M2 执行任务卡

本文件是 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) **M2 远程 DES 动作**的执行版。硬约束仍见该文 §0（DES-32）；协议落地见 [WORLD_ADAPTER.md](WORLD_ADAPTER.md) §4.3～4.5、§6.2～6.3；Mock 替换见 [WA_MOCK_REGISTRY.md](WA_MOCK_REGISTRY.md) WAM-04～06。M1 读路径已准出（含 T-50 `with_des`）。

**准出（缺一不可）**：远程 DES 上 Propose → 审批 → Execute → Confirm 闭环可用；子动作回执与答案同步可查；WA-03、04、05、08 在远程 DES 有可执行断言；RT-DES-08～11 与 RT-DESC 回归绿；动态场景证明已发生前缀不变、未来事件随干预改变；双角色同一正式运行完成处置；生产模拟仍可走同一 UI（写演示可仍在隔离世界）；全量 M0/M1 套件回归绿。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| M2-0 | 冻结用例草案与任务卡；同步 PLAN / Feature Map / 注册表 | **完成**（#194） | M1 准出 + T-50 |
| M2-1 | 正式 CASES + Red 空壳；Propose/Execute/Confirm 接远程（高风险先方案） | **完成**（#196） | M2-0 |
| M2-2 | 授权/并发/取消 + 写路径服务层 Green（RT-WADES-09～14） | **完成**（#199） | M2-1 |
| M2-3 | 恢复与幂等：丢回执同厂恢复；部分回执补齐；工厂重启旧运行可见失败 | **完成**（#200） | M2-2 |
| M2-4 | DES 闭环：干预后未来事件改变；答案与干预一致；RT-DES/DESC 回归 | **完成**（#202） | M2-2、M2-3 |
| M2-5 | 浏览器：WA-03/04/05/08 远程场景 + M0/M1 全量回归 | **完成**（#206） | M2-4 |
| M2-6 | 证据、WAM-04～06 写路径状态、准出检查 | **进行中**（本 PR） | M2-5 |

一张卡 ≈ 一个 PR。每卡先 Red（业务断言失败）再 Green；断言冻结后不得弱化。

## 1. 用例命名（已登记）

设计项 WA-03、04、05、08 在远程 DES 上的验收独立于 M0 Mock 写路径。正式 id 已写入 `CASES.md` 表格（M2-1）：

| 设计 | 正式 id | 要点 |
|---|---|---|
| WA-03（授权与并发） | `E2E-WAEXEC` | 参数/方案/版本变化拒绝执行；同授权并发只一次业务执行；取消后待发不发、迟到回执不改终态 |
| WA-04（恢复与幂等） | `E2E-WARECV` | 同存活工厂丢响应后找回原工单；部分回执可补齐；工厂重启后旧运行失败可见且不误投新实例 |
| WA-05（DES 闭环） | `E2E-WADYN` | 会话→MCP 动态场景：已发生前缀不变；批准干预后未来事件改变；答案与干预一致 |
| WA-08（产品端到端） | `E2E-WALOOP` | 正常入口查看→方案→审批→核验；运营视图同一执行证据；双角色同一正式运行 |
| 合同：提案绑定 | `RT-WADES-09` | Propose 绑定 case/`plan_revision`；过期或修订变化 → `APPROVAL_INVALID` / `PRECONDITION_FAILED` |
| 合同：并发执行 | `RT-WADES-10` | 同 `idempotency_key` 并发 Execute 只一次；键同载荷异 → `IDEMPOTENCY_CONFLICT` |
| 合同：取消守卫 | `RT-WADES-11` | 取消后待发子动作不发送；已发迟到回执只更新证据、案例终态保留 |
| 合同：同厂恢复 | `RT-WADES-12` | 丢回执后按原关联键 Confirm 回原工单；部分回执可补齐 |
| 合同：工厂重启 | `RT-WADES-13` | 工厂实例变更后旧运行提交失败可见；不向新实例误投 |
| 合同：干预闭环 | `RT-WADES-14` | 干预后已发生前缀不变、未来事件改变；审核答案与干预一致；普通会话无标准答案 |
| 合同：关联键恢复 | `RT-WADES-15` | 丢回执后仅用 `correlation_key` Confirm 回原 `execution_id` |
| 合同：部分回执补齐 | `RT-WADES-16` | 部分失败后补齐剩余子动作回执，再 Confirm 到终态 |
| 合同：重启不误投 | `RT-WADES-17` | 工厂重启后历史可查；旧写失败可见；pending 不误投新实例 |
| 合同：答案一致 | `RT-WADES-18` | 干预后 `ground_truth` 的 intervention/execution/answer 与干预一致且 `synced` |
| 合同：答案同步重试 | `RT-WADES-19` | 同步失败 → `pending`；补偿重试 → `synced` 且与原干预一致 |
| 回归 | 既有 `RT-WA-*`、`RT-WADES-01～08`、`RT-DES-08～11`、`RT-DESC-*`、`wa-m0/m1-suite` | M2 合入前全绿；Mock 写路径保留作回归 |

M0/M1 已冻结用例 **不得弱化**。`E2E-DESOPS` / `E2E-DESLOOP` 在 M2 补远程物理断言（设计表已标明）。

## 2. M2-0：冻结与收尾（已完成）

见 #194 / `docs/testing/evidence/wa-m2-0.md`。

## 2b. M2-1：正式表格 + Red（已完成，#196）

1. CASES 正式表格 + Playwright 空壳 `web/e2e/wa-des-write.spec.ts` + `features.json` cases。
2. 高风险 Green 方案：[`WA_M2_1_PLAN.md`](WA_M2_1_PLAN.md)（服务层优先；不改 engine/DesConnector 内部）。
3. `RT-WADES-09`～`14`：表格已冻结；本地 HTTP Red 证据见 `wa-m2-1-red.md`；**Rust 失败测试不入库**（由 M2-2 挂 `mom_world_des_write` + `runtime_cases`）。
4. **本卡不写 Green。**

## 2c. M2-2：写路径服务层 Green（已完成，#199）

1. 按已批方案在 `mom_world_des` 实现 `/api/world/des/propose|execute|confirm` 与测试控件。
2. 合同测试入库：`crates/server/tests/mom_world_des_write.rs` → `RT-WADES-09`～`14` Green。
3. `features.json` `runtime_cases` 挂入；证据 `wa-m2-2-green.md`。
4. **不改** `engine.rs` / `DesConnector` 内部；Playwright E2E 写路径 Green 留给 M2-5。

## 2d. M2-3：恢复与幂等加深（已完成，#200）

见 [`WA_M2_3_PLAN.md`](WA_M2_3_PLAN.md)。

1. Confirm 支持按 `correlation_key` 找回原工单（`RT-WADES-15`）。
2. `complete-partial` 补齐部分回执后再 Confirm（`RT-WADES-16`）。
3. 工厂重启后历史可查、待发不误投（`RT-WADES-17`）。
4. **仍不改** engine / DesConnector；`E2E-WARECV` 浏览器 Green 留给 M2-5。

## 2e. M2-4：DES 闭环与回归（已完成，#202）

见 [`WA_M2_4_PLAN.md`](WA_M2_4_PLAN.md)。

## 2f. M2-5：浏览器远程写（已完成，#206）

见 [`WA_M2_5_PLAN.md`](WA_M2_5_PLAN.md)。

## 2g. M2-6：证据与准出（本卡）

见 [`WA_M2_6_PLAN.md`](WA_M2_6_PLAN.md)。

1. 准出证据 `wa-m2-6.md`；对照任务卡文首准出表逐条勾选。
2. `WA_MOCK_REGISTRY` WAM-04～06 写路径状态定稿。
3. 同步 PLAN / Feature Map / COVERAGE / `features.json`。
4. **不改**业务代码与 engine / DesConnector。

## 3. M2-2～M2-4：服务端（纲要）

- 优先服务层：`/api/world/des` Propose/Execute/Confirm 调用既有 MOM `select_des_plan` / `approve` / `execute` + `DesConnector::dispatch_writes`；读路径继续用 T-50 投影边界。详见 [`WA_M2_1_PLAN.md`](WA_M2_1_PLAN.md)。
- **高风险**：若需改 `engine.rs`（`execute` / `des_after_execute_remote` 提交窗口）、`store`、`orchestrator`、或 `DesConnector` 派发/回执语义，**先出方案等人批准**（`AGENTS.md`）。
- 持久化：执行 ID、幂等键、子动作关联键、审核同步状态（见 WORLD_ADAPTER.md §8）；新增表须附迁移。
- 故障与时钟测试控件不得在生产配置默认打开。
- 共享守卫与 [R-20261120-H2](../fix-plans/r-20261120-h2.md) 对齐（若该方案已批）。

## 4. M2-5 / M2-6：自动化与准出

- Playwright：远程 DES fixture（`des_mcp` / 测试连接器）+ 双角色上下文；写路径对照保留 `waMockServer`。
- 每条 `E2E-WAEXEC` / `WARECV` / `WADYN` / `WALOOP` 与 `RT-WADES-09～14` 可独立重跑；失败留 Trace/关联 ID。
- 合入前：`./scripts/wa-m0-suite.sh` + `./scripts/wa-m1-suite.sh` + 新 M2 套件 + RT-DES/DESC 相关回归。
- 更新 `WA_MOCK_REGISTRY`：WAM-04～06 写路径状态；证据 `docs/testing/evidence/wa-m2-*.md`。

## 5. 不做（M2）

- 不做生产站点适配与真实 MES 写（M3/M4）。
- 不退役 `/api/world/mock` 回归路径。
- 不弱化 M0/M1 已冻结断言。
- 不开放跨工厂进程重启后的 `durable` 声明（独立能力，见 WORLD_ADAPTER.md §9）。
