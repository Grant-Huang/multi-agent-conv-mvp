# T-85 跨组场景独立验收（主线 #2：重启语义 + 正式 Observe 选单 + 深就绪门）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+「继续」指示（2026-11-23） |
| 验收人 | 本 worker（未参与 #247/#248/#249 实现） |
| 基线 | `origin/master` `1a9ae70`（含 #247～#249；#246 为本线 T-83，已合并） |
| 检出方式 | 隔离 worktree `/tmp/t85-wt` @ `1a9ae70`，独立 CARGO_TARGET_DIR 全新构建 |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 最新仍 11-22 失败；**11-23 至收稿 20:25Z 定时仍未触发**（第 3 线程序化监视至 ~21:50Z，结论回填其 PR #240）；登记表不改 |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`c69c9e5` → `1a9ae70`）与编号说明

| PR | 内容 | 复验入口 |
|---|---|---|
| #247 | harness 深就绪门（等首页数据接口，非只等 `/api/health`）——**Q-T67-02 冷启动 flake 的修复** | desktop.spec 全量复跑 |
| #248 | M3 正式 Observe 路线选单工单（fixtures） | `mom_world_prod`（6→8 用例） |
| #249 | WP7 重启语义：中断任务写原因事件与失败交付 | `restart_semantics` 合同 + `restart.spec` 浏览器 |

**任务号**：T-84 已被第 4 线在途验收分支（`cursor/line4-t84-acceptance`）占用，本卡取 **T-85**。
**台账观察**：#241（第 1 线）与 #249（第 4 线）均标注 T-81——**一号双占已进入合并历史**；建议各线领号前查 `git worktree list` + PR 历史（本线自 T-83 起执行）。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| WP7 重启语义合同（RT-RESTART-01～05，#249） | **VERIFIED / T2** | `restart_semantics` **6 passed**：中断任务 FAILED+原因事件、会话失败交付、验收/审批跨重启可判定、返工额度按库扣减 |
| 重启语义浏览器（E2E-RESTART） | **VERIFIED / T2** | 1 passed（运行中重启明确失败；待验收重启后判定通过） |
| M3 正式 Observe 选单（#248） | **VERIFIED / T2** | `mom_world_prod` **8 passed**（+2 选单工单用例） |
| harness 深就绪门（#247，Q-T67-02 修复） | **VERIFIED / T2** | desktop.spec 全量 **31 passed** 且整跑 25.1s→21.2s；本轮无冷启动超时复现 |
| 既有回归（C2/记忆/prod 门禁） | **VERIFIED / T2** | 7+9+8 合同绿；E2E-REV 绿 |
| E2E-MOBILE | **FAILED（第 6 轮确认）** | Q-T65-01 未修，G15 唯一阻塞不变 |
| PR CI #247/#248/#249 | **VERIFIED** | 全部 8/8 pass |
| master push @ `1a9ae70` | in_progress → 终态回填 §3 | run 45257708316 |

**总体**：重启语义、正式 Observe 选单、深就绪门三项独立复验 **PASS**；无新业务缺陷。

---

## 1. 本地证据（worktree @ `1a9ae70`，全新 target）

```text
$ cargo test --locked -p an-server --test restart_semantics --test desktop_mem \
    --test desktop_c2 --test mom_world_prod
desktop_c2: 7   desktop_mem: 9   mom_world_prod: 8（+2 选单）   restart_semantics: 6
合计 30 passed / 0 failed

$ cd web && npm run build → 通过
$ npx playwright test e2e/restart.spec.ts            → 1 passed（独占运行）
$ npx playwright test e2e/desktop.spec.ts            → 31 passed (21.2s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → REV ✓；MOBILE ✗（同签名）
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 中断任务终态明确（FAILED）且带「重启中断」原因事件 | 通过 | RT-RESTART-01 |
| 失败交付回绑定会话（task_id 关联、可重新委派文案） | 通过 | RT-RESTART-02 |
| 待验收（有 result）跨重启判定照常、事件含人工 note | 通过 | RT-RESTART-03 |
| 待验收 reject 返工额度按库扣减、二次 reject FAILED | 通过 | RT-RESTART-04 |
| 待批准跨重启按路由续执行至终态 | 通过 | RT-RESTART-05 |
| 浏览器运行中重启 → 明确失败呈现；重启后判定通过 | 通过 | E2E-RESTART |
| Observe 路线选单工单（正式路线 fixtures） | 通过 | mom_world_prod +2 用例 |
| 深就绪门：等首页数据而非 health | 通过 | desktop 全量绿 + 无冷启动超时（Q-T67-02 关闭候选，归属第 3 线复核） |
| Mock / fixture 可辨 | 通过 | harness world_mock；fixtures Observe 口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#247 / #248 / #249：全部 **8/8 pass**（逐个 `gh pr checks`，2026-11-23）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `1a9ae70`（#249，run [45257708316](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45257708316)） | **success**（记录时 in_progress，同会话复跑确认终态，全部作业通过） |
| nightly | 11-23 至 20:25Z 未触发（第 3 线监视窗口 ~21:50Z 止）；**若窗口关闭仍无运行，「定时未执行」为升级链覆盖不到的新缺口（只覆盖失败、不覆盖缺失）**，将记入下轮问题清单；G15 状态不变 |

## 4. 问题清单

### Q-T65-01（延续，第 6 轮确认）— E2E-MOBILE 未修复

`1a9ae70` 同签名失败。归属不变（Desktop/前端，与 G15 根因合并）。

### Q-T85-01 — 任务号 T-81 一号双占已入合并历史（轻微，台账）

#241（第 1 线，记忆探针）与 #249（第 4 线，WP7 重启语义）均标注 T-81。两者内容独立、不冲突，但台账一号两事违反可追踪口径。建议：各线领号前查 worktree/PR/PLAN 占用（第 1/4 线）；已合并历史不回改，PLAN 登记时以任务内容为准消歧。归属：各线 + 用户确认编号约定。

### 观察

- **nightly 未触发风险**：11-23 定时至收稿未运行（第 3 线监视中）。若坐实，升级链（失败自动开 issue）对「没跑」无覆盖——登记表例外条目 2026-11-28 到期评审时建议一并处理。

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **服务重启语义闭环**：中断明确、交付可达、审批/验收/返工跨重启一致——主轴「状态由系统自己保存」的关键一环 | — |
| M3 正式 Observe 路线覆盖选单工单 | 远程真站点 live（试点窗口前提） |
| E2E 冷启动稳定性修复落地（深就绪门） | **E2E-MOBILE 未修 → G15/G6 无法关闭**（唯一业务阻塞）；11-23 nightly 未触发待定性 |
| — | 首页真实快照（#250 在途，第 1 线）、记忆 5.2a（#252 在途，第 5 线，含 DB 补列）——合入后本线复验 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、契约正文、断言、登记表、PLAN；未修 E2E-MOBILE；未跑远程真站点
- 在途 PR #250/#251/#252 未纳入（未合入）；nightly 监视归第 3 线，本卡只记录状态
