# M1-4 方案：远程 DES 浏览器场景 + M0 回归

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) M1-4 / T-45。**不改** `engine.rs` / `DesConnector` / `orchestrator`。

## 目标

1. `E2E-WADES` / `WAREAD` / `WAOBS` 在最新 master 可独立重跑且绿。
2. 工作台 DES 读水位走 `/api/world/des`（Observe / Subscribe / step），不再仅靠本地假数。
3. 合入前：`wa-m0-suite` + `mom_world_des` + `wa-des-remote` 全绿。
4. 更新 `WA_MOCK_REGISTRY`：WAM-01～03 标注 M1 读路径已挂 `/api/world/des`。

## 实现

| 项 | 做法 |
|---|---|
| `api.worldDes` | bind / observe / subscribe / step |
| `MomWorkbench` | DES 世界：bind→step→observe；「重新读取」走 subscribe；过期游标 409 → banner |
| 套件 | `scripts/wa-m1-suite.sh`（含 M0 + 远程） |
| 写路径 | 仍 `worldMock` / `waMockServer` |

## 非目标

- 真 MCP / 子进程 DesConnector
- Propose/Execute 真写（M2）
- 退役 `/api/world/mock`

## 验收

- `playwright test e2e/wa-des-remote.spec.ts` 3 passed
- `./scripts/wa-m1-suite.sh` 绿
- `npm run test:map` 绿
