# World Adapter M1-5 准出证据

分支：`cursor/wa-m1-5-exit-checklist-f2df`。方案：`docs/mom/WA_M1_5_PLAN.md`。任务卡：[WA_M1_TASKS.md](../../mom/WA_M1_TASKS.md)。

## 范围

- 读路径：`/api/world/des`（服务层；与 `[mom] world_mock` 同门控）
- 合同：`RT-WADES-01`～`08`（`mom_world_des.rs`）
- 浏览器：`E2E-WADES` / `WAREAD` / `WAOBS`（`wa-des-remote.spec.ts`）
- 注册表：WAM-01～03 读路径状态（[WA_MOCK_REGISTRY.md](../../mom/WA_MOCK_REGISTRY.md)）
- 套件：`./scripts/wa-m1-suite.sh`（含 M0 回归）
- **未改** `engine.rs` / 真 `DesConnector` 子进程；写路径仍 Mock

## 准出检查（对照 WA_M1_TASKS）

| # | 准出项 | 结论 | 证据 |
|---|---|---|---|
| Q1 | 主应用 MOM 可读远程 DES 世界 | **通过（服务层）** | `mom_world_des` + `MomWorkbench` DES 读水位；真 `with_des`/`DesConnector` **未接**（M1-1～4 明示） |
| Q2 | 世界绑定到远程 DES 运行 | **通过** | `RT-WADES-01`；`E2E-WADES`（含 `/api/world/des/bind`） |
| Q3 | Observe/Subscribe = 正式投递边界（非裸 poll） | **通过** | `RT-WADES-02/03/05/06`；ingest→commit 水位 |
| Q4 | 试跑零投递；正式可投递 | **通过** | `RT-WADES-04/07/08`；正式 `OpsCase provenance=des` |
| Q5 | WA-01/02/07 远程场景可执行 | **通过** | `E2E-WADES` / `WAREAD` / `WAOBS` 3 passed |
| Q6 | 写路径仍隔离 Mock | **通过** | `DesExperiment`→`worldMock`；`wa-m0-suite` 未弱化 |
| Q7 | 全量 M0 套件回归绿 | **通过** | `wa-m0-suite` 嵌在 `wa-m1-suite` |

## 卡片追溯

| 卡 | PR | 证据 |
|---|---|---|
| M1-0 | #176 | `wa-m1-0.md` |
| M1-1 Red/Green | #178 / #180 | `wa-m1-1-red.md` / `wa-m1-1-green.md` |
| M1-2 | #181 | `wa-m1-2.md` |
| M1-3 | #183 | `wa-m1-3.md` |
| M1-4 | #184 | `wa-m1-4.md` |
| M1-5 | 本 PR | 本文 |

## 回归结果

| 层 | 状态 |
|---|---|
| RT-WADES-01～08 | 8 passed |
| E2E-WADES / WAREAD / WAOBS | 3 passed |
| RT-WA-01～09 | 9 passed |
| E2E-DES* + WABIND + WACHANNEL | 8 passed |
| `npm run test:map` | Feature map valid |

## 命令

```text
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome ./scripts/wa-m1-suite.sh
# → mom_world_des: 8 passed
# → wa-des-remote: 3 passed
# → wa-m0-suite: RT-WA 9 + Playwright 8
# → wa-m1 suite green

cd web && npm run test:map
# → Feature map valid
```

## 非目标 / 已知缺口

- 真 `DesConnector` / MCP 子进程创建与推进（可归 M1 后续补接或 M2 前置）
- 真工厂 Propose/Execute/Confirm（M2）
- 生产只读 / 生产写（M3/M4）
- 退役 `/api/world/mock`
