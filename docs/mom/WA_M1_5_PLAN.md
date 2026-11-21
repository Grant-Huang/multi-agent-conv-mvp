# M1-5 方案：证据、注册表与准出检查

承接 [WA_M1_TASKS.md](WA_M1_TASKS.md) M1-5 / T-48。**不改**业务代码与高风险引擎路径；本卡为文档 / 台账 / 套件证据收尾。

## 目标

1. 写准出证据 `docs/testing/evidence/wa-m1-5.md`（对照任务卡「准出」逐条勾选）。
2. `WA_MOCK_REGISTRY`：WAM-01～03 读路径状态定稿；入口指向 `wa-m1-suite` / `wa-m1-5.md`。
3. 同步 `WA_M1_TASKS`、`docs/PLAN.md`、Feature Map / COVERAGE / `features.json`。
4. 本地跑通 `./scripts/wa-m1-suite.sh`（含 M0 回归）并贴输出。

## 诚实边界（写进准出表）

- M1 读路径落地为服务层 `/api/world/des`（与 `world_mock` 同门控），**未**接真 `DesConnector` / `with_des` 子进程（与 M1-1～4 方案一致）。
- 写路径仍隔离 Mock；真工厂写属 M2。

## 非目标

- 不改 `engine` / `DesConnector` / 前端行为
- 不退役 `/api/world/mock`
- 不开 M2

## 验收

- `wa-m1-5.md` 准出表完整；任务卡 M1-0～5 均标已完成
- `./scripts/wa-m1-suite.sh` 绿；`npm run test:map` 绿
