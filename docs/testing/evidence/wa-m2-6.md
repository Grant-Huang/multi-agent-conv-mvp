# World Adapter M2-6 准出证据

分支：`cursor/wa-m2-6-exit-checklist-f2df`。方案：`docs/mom/WA_M2_6_PLAN.md`。任务卡：[WA_M2_TASKS.md](../../mom/WA_M2_TASKS.md)。

## 范围

- 写路径：`/api/world/des` Propose / Execute / Confirm + `ops/approve-execute`（服务层）
- 合同：`RT-WADES-09`～`19`（`mom_world_des_write.rs`）+ 读路径 `RT-WADES-01`～`08`
- 浏览器：`E2E-WAEXEC` / `WARECV` / `WADYN` / `WALOOP`（`wa-des-write.spec.ts`）
- 回归：`RT-DES-08`～`11`、`RT-DESC-*`（connector）、`wa-m0/m1-suite`
- 注册表：WAM-04～06 写路径状态（[WA_MOCK_REGISTRY.md](../../mom/WA_MOCK_REGISTRY.md)）
- 套件：`./scripts/wa-m2-suite.sh`
- **未改** `engine.rs` / DesConnector 内部；生产 MES 写属 M3/M4

## 准出检查（对照 WA_M2_TASKS 文首）

| # | 准出项 | 结论 | 证据 |
|---|---|---|---|
| Q1 | Propose → 审批 → Execute → Confirm 闭环可用 | **通过（服务层）** | `RT-WADES-09`～`14`；`ops/approve-execute` + Confirm（M2-5） |
| Q2 | 子动作回执与答案同步可查 | **通过** | `RT-WADES-12/15/16`；`RT-WADES-18/19` |
| Q3 | WA-03/04/05/08 远程可执行断言 | **通过** | `E2E-WAEXEC` / `WARECV` / `WADYN` / `WALOOP` 4 passed（#206） |
| Q4 | RT-DES-08～11 与 RT-DESC 回归绿 | **通过** | `wa-m2-regression.sh` |
| Q5 | 动态场景：前缀不变、未来随干预改变 | **通过** | `RT-WADES-14`；`E2E-WADYN` |
| Q6 | 双角色同一正式运行闭环 | **通过** | `E2E-WALOOP` |
| Q7 | 生产模拟仍可走同一 UI（写演示可隔离世界） | **通过** | DES 工作台 + 运营频道；Mock 写路径未退役 |
| Q8 | 全量 M0/M1 套件回归绿 | **通过** | `wa-m1-suite`（含 `wa-m0-suite`）嵌在 `wa-m2-suite` |

## 卡片追溯

| 卡 | PR | 证据 |
|---|---|---|
| M2-0 | #194 | `wa-m2-0.md` |
| M2-1 Red | #196 | `wa-m2-1-red.md` |
| M2-2 Green | #199 | `wa-m2-2-green.md` |
| M2-3 | #200 | `wa-m2-3-recovery.md` |
| M2-4 | #202 | `wa-m2-4-closed-loop.md` |
| M2-5 | #206 | `wa-m2-5-browser.md` |
| M2-6 | 本 PR | 本文 |

## 回归结果

| 层 | 状态 |
|---|---|
| RT-WADES-09～19（write） | 见 `wa-m2-regression` / `mom_world_des_write` |
| RT-WADES-01～08（read） | 12 passed（含 live） |
| E2E-WAEXEC / WARECV / WADYN / WALOOP | 4 passed |
| RT-DES-08～11 | 4 passed |
| RT-DESC connector | 6 passed |
| wa-m0 / wa-m1 suite | 绿（嵌套） |
| `npm run test:map` | Feature map valid |

## 命令（本分支本地）

```text
$ ./scripts/wa-m2-suite.sh
== wa-m2-regression green ==
== Playwright：E2E-WAEXEC / WARECV / WADYN / WALOOP == 4 passed
== wa-m1 suite green ==（含 wa-m0：RT-WA 9 + Playwright 8）
== wa-m2 suite green ==

$ cd web && npm run test:map
Feature map valid: … features, … browser cases
```

## 非目标 / 已知缺口

- 生产站点适配与真实 MES 写（M3/M4）
- 退役 `/api/world/mock` 写路径
- 跨工厂进程重启后的 `durable` 声明（WORLD_ADAPTER.md §9）
- `E2E-DESOPS` / `E2E-DESLOOP` 远程物理断言加深（设计表后续；本里程碑以 WAEXEC/WARECV/WADYN/WALOOP 为准）

## 人工项

- [ ] L2 审查人批准本 PR（作者不可自批）
- [ ] 合入后确认 tracker T-62 / 父里程碑 T-49 状态
