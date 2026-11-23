# M3-5 方案：证据、WAM-07 读路径与准出检查

承接 [WA_M3_TASKS.md](WA_M3_TASKS.md) M3-5 / T-80。**不改**业务代码与高风险引擎路径；本卡为文档 / 台账 / 套件证据收尾。

## 目标

1. 写准出证据 `docs/testing/evidence/wa-m3-5.md`（对照任务卡「准出」逐条勾选）。
2. `WA_MOCK_REGISTRY`：WAM-07 **读路径**状态定稿；入口指向 `wa-m3-suite` / `wa-m3-5.md`。
3. 同步 `WA_M3_TASKS`、`docs/PLAN.md`、Feature Map / COVERAGE / `features.json` / CASES。
4. 本地跑通 `./scripts/wa-m3-suite.sh`（或等价分步）并贴输出。

## 诚实边界（写进准出表）

- M3 生产读落地为服务层 `/api/world/prod` **桩数据**（权威源登记 stub；与五线规划 / `WORLD_ADAPTER_PLAN` 近期措辞一致）。
- **未**接真实 MES/ERP 凭据或 egress；真站点只读验收在候选站点与权限就绪后另开，不阻塞本卡收口。
- 写演示仍 Mock/DES；生产 Propose/Execute 显式 `WRITE_DISABLED`（RT-06 / E2E-WAPROD）。
- 测试控件仅 `world_mock` + `with_test_controls`；不改 `engine` / `store` / `orchestrator`。

## 非目标

- 不改 `engine` / `store` / `orchestrator` / DesConnector
- 不接真站点、不开 M4 生产写
- 不退役 `/api/world/mock` 或 `/api/world/des`
- 不自行开下一卡

## 验收

- `wa-m3-5.md` 准出表完整；任务卡 M3-0～5 均标已完成（本 PR 合入后）
- `./scripts/wa-m3-suite.sh`（或分步等价）绿；`npm run test:map` 绿
- M3 里程碑结论：**桩数据路径准出通过**
