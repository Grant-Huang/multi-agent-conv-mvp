# M2-6 方案：证据、WAM 写路径与准出检查

承接 [WA_M2_TASKS.md](WA_M2_TASKS.md) M2-6 / T-62。**不改**业务代码与高风险引擎路径；本卡为文档 / 台账 / 套件证据收尾。

## 目标

1. 写准出证据 `docs/testing/evidence/wa-m2-6.md`（对照任务卡「准出」逐条勾选）。
2. `WA_MOCK_REGISTRY`：WAM-04～06 写路径状态定稿；入口指向 `wa-m2-suite` / `wa-m2-6.md`。
3. 同步 `WA_M2_TASKS`、`docs/PLAN.md`、Feature Map / COVERAGE / `features.json`。
4. 本地跑通 `./scripts/wa-m2-suite.sh`（或等价：`wa-m2-regression` + Playwright 写路径 + `wa-m1-suite`）并贴输出。

## 诚实边界（写进准出表）

- M2 写路径落地为服务层 `/api/world/des` Propose/Execute/Confirm + 运营投影（与 M2-1～5 方案一致）；**未**改 `engine.rs` / DesConnector 内部派发语义。
- 生产 MES / 真实审批属 M3/M4；`/api/world/mock` 写路径保留作回归。
- 测试控件仅 harness / `with_app_store` 桩路径开放；真连接器挂载默认关闭。

## 非目标

- 不改 `engine` / `DesConnector` / 编排状态机
- 不退役 `/api/world/mock`
- 不开 M3/M4 生产接入

## 验收

- `wa-m2-6.md` 准出表完整；任务卡 M2-0～6 均标已完成（本 PR 合入后）
- `./scripts/wa-m2-suite.sh`（或分步等价）绿；`npm run test:map` 绿
