# World Adapter M3-5 准出证据

分支：`cursor/wa-m3-5-exit-checklist-f2df`。方案：`docs/mom/WA_M3_5_PLAN.md`。任务卡：[WA_M3_TASKS.md](../../mom/WA_M3_TASKS.md)。

## 范围

- 读路径：`/api/world/prod` Bind / Observe / Subscribe + 测试控件（桩数据）
- 合同：`RT-WAPROD-01`～`06`（`mom_world_prod.rs`）
- 浏览器：`E2E-WAPROD` / `WAPISO` / `WAPREAD` / `WAPDELAY`（`wa-prod-read.spec.ts`）
- 回归：`wa-m3-suite` = Playwright 生产读 + `wa-m2-suite`（含 M0/M1）
- 注册表：WAM-07 **读**路径状态（[WA_MOCK_REGISTRY.md](../../mom/WA_MOCK_REGISTRY.md)）
- **未改** `engine.rs` / `store` / `orchestrator`；真站点凭据/egress **未接入**；生产写属 M4

## 准出检查（对照 WA_M3_TASKS 文首）

| # | 准出项 | 结论 | 证据 |
|---|---|---|---|
| Q1 | 可绑定生产世界并只读观测（来源与截至水位） | **通过（桩数据）** | `RT-WAPROD-01`/`02`；`E2E-WAPROD`；`wa-m3-2-green.md` / `wa-m3-4-browser.md` |
| Q2 | 权限/跨范围拒绝可执行 | **通过** | `RT-WAPROD-03`；`E2E-WAPISO`；`wa-m3-3-green.md` |
| Q3 | 断线或游标过期后续读无漏无重复 | **通过** | `RT-WAPROD-04`；`E2E-WAPREAD` |
| Q4 | 多来源延迟与 `as_of` 可辨 | **通过** | `RT-WAPROD-05`；`E2E-WAPDELAY` |
| Q5 | 写能力在生产世界上关闭或仍走隔离 Mock/DES | **通过** | `RT-WAPROD-06`；E2E 写入口禁用；Mock Propose 仍可用 |
| Q6 | 全量 M0～M2 套件回归绿 | **通过** | `./scripts/wa-m3-suite.sh`（嵌套 `wa-m2-suite`） |

## 卡片追溯

| 卡 | PR | 证据 |
|---|---|---|
| M3-0 | #209 | `wa-m3-0.md` |
| M3-1 Red | #213 | `wa-m3-1-red.md` |
| M3-2 Green | #218 | `wa-m3-2-green.md` |
| M3-3 | #220 | `wa-m3-3-green.md` |
| M3-4 | #224 | `wa-m3-4-browser.md` |
| M3-5 | 本 PR | 本文 |

## 回归结果

| 层 | 状态 |
|---|---|
| RT-WAPROD-01～06 | 6 passed（`mom_world_prod`） |
| E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY | 4 passed |
| wa-m2-regression + wa-des-write | 嵌套于 `wa-m2-suite` |
| wa-m0 / wa-m1 suite | 绿（嵌套） |
| `npm run test:map` | Feature map valid |

## 命令（本分支本地）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_prod -- --nocapture
test result: ok. 6 passed; 0 failed

$ cd web && npx playwright test e2e/wa-prod-read.spec.ts
  ✓ E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY
  4 passed

$ ./scripts/wa-m3-suite.sh
== Playwright：E2E-WAPROD* == 4 passed
== wa-m2 suite green ==（含 wa-m1 / wa-m0）
== wa-m3 suite green ==

$ cd web && npm run test:map
Feature map valid: 33 features, 76 browser cases
```

## M3 准出结论

**World Adapter M3（生产只读 · 桩数据路径）准出通过。**

- 合同 + 浏览器 + M0～M2 回归齐备；WAM-07 **读**路径以 `/api/world/prod` stub 定稿。
- **非**真站点验收：候选站点、权威源与读权限就绪后另开真实只读验收；不阻塞本里程碑收口。
- 生产写 / WA-06 属 **M4**。

## 非目标 / 已知缺口

- 真实 MES/ERP 凭据、egress、生产身份映射
- 退役 `/api/world/mock` / `/api/world/des`
- 跨工厂 `durable` 声明（WORLD_ADAPTER.md §9）
- M4 生产写确认

## 人工项

- [ ] L2 审查人批准本 PR（作者不可自批）
- [ ] 合入后确认 tracker T-80 / 父里程碑状态
