# T-88 跨组场景独立验收（主线 #2：G15 编译关修复复核 + 门范围统一）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+「继续下一个任务」指示（2026-11-24） |
| 验收人 | 本 worker（未参与 #259 实现，作者为另一 Claude 会话） |
| 基线 | `origin/master` `33b2d05`（#259 已合并；#258 为本线 T-87） |
| 检出方式 | 隔离 worktree `/tmp/t88-wt` @ `33b2d05`，独立 CARGO_TARGET_DIR 全新构建 |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 最新仍 11-23 失败（TS6133，2m44s 编译死）；**11-24 定时未触发**（按 11-23 形态预计 ~22:10Z）；本卡验证的 #259 修复在下次定时前已入 master |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`6be0d89` → `33b2d05`：#259）

| 修复面 | 内容 | 复验入口 |
|---|---|---|
| Q-T87-01 | 删除 `restartServer.ts:41`（`goal`）与 `restart.spec.ts:34`（`api`）两处未使用变量 | 双范围 typecheck + 夜间编译关 |
| Q-T87-02 | `package.json` `typecheck` 纳入 `tsc --noEmit -p e2e/tsconfig.json`——PR CI web 作业与 nightly verify 同范围 | typecheck 脚本对照 |
| 设计索引同步 | DESIGN 0.3 记忆行补全（5.1 默认开 / 5.2a 已实现 / REQ-03/04 未开始）+ `desktop_memories` 数据清单行 | DESIGN diff 核查 |
| 治理文档 | `decisions/2026-11-24-lift-paper-constraint.md`（解除论文约束）、DEV_STANDARD/PRODUCT 微调 | 存在性核查（内容归治理线，不在本卡范围） |

**任务号**：T-88 取号查重通过（worktree/PR/PLAN 无占用）。**编号竞态延续提醒**：T-87 双占（#257/#258）已入历史；AR-N 前缀或领号台账提议**仍待用户拍板**。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| Q-T87-01 修复 | **VERIFIED / T2** | 双范围 typecheck（主代码 + e2e/tsconfig）本地 **exit 0**；两处未使用变量已删（diff 各 -1 行） |
| Q-T87-02 修复 | **VERIFIED / T2** | `typecheck` 脚本现为 `tsc --noEmit && tsc --noEmit -p e2e/tsconfig.json`——PR 门与 nightly 门同范围，编译卫生债回归 PR 层拦截 |
| 设计索引同步 | **VERIFIED / T2** | DESIGN 0.3 记忆行与数据清单行与实现现状一致（含 5.1 默认开、REQ-03/04 未开始如实标注） |
| 既有回归 | **VERIFIED / T2** | 合同 8 套件 **48/48**；浏览器 desktop 全量 **31 passed**；E2E-REV 绿 |
| E2E-MOBILE | **FAILED（第 8 轮本地确认）** | Q-T65-01 未修；**11-24 nightly 编译关已解除，将首次跑进运行期——若今晚失败，MOBILE 将成为唯一直接嫌疑** |
| PR CI #259 | **VERIFIED** | 8/8 pass |
| master push @ `33b2d05` | in_progress → 终态回填 §3 | run 45330732823 |

**总体**：G15 编译关修复与门范围统一独立复核 **PASS**；G15 关闭剩余唯一运行期阻塞 = Q-T65-01（MOBILE）。

---

## 1. 本地证据（worktree @ `33b2d05`，全新 target）

```text
$ npm run typecheck（web）
> tsc --noEmit && tsc --noEmit -p e2e/tsconfig.json    → exit 0

$ cargo test --locked -p an-server（8 套件）
desktop_snapshot: 4  desktop_drafts: 4  personal_memory: 5  session_materials: 5
desktop_mem: 9       restart_semantics: 6  mom_world_prod: 8  desktop_c2: 7
合计 48 passed / 0 failed（8/8 套件零失败）

$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts             → 31 passed (23.6s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → REV ✓；MOBILE ✗（同签名）
```

### 核对清单

| 检查点 | 结果 | 依据 |
|---|---|---|
| 夜间编译关解除（TS6133 清除） | 通过 | typecheck 双范围 exit 0 |
| PR 门与 nightly 门同范围 | 通过 | `typecheck` 脚本含 `e2e/tsconfig.json` |
| 修复未触及业务行为 | 通过 | diff 仅 -2 行未使用变量 + scripts + docs；合同/浏览器回归全绿 |
| 设计索引与实现一致 | 通过 | DESIGN 0.3 diff 核查 |
| Mock / fixture 可辨 | 通过 | 同既有口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#259：**8/8 pass**（含 web 作业——typecheck 新范围在 PR 层即已拦截类问题，正例见本 PR 绿灯）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `33b2d05`（#259，run [45330732823](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45330732823)） | **success**（记录时 in_progress，同会话复跑确认终态） |
| nightly | 11-24 未触发（收稿时点）；修复已入 master，预计今晚首次越过编译关进入运行期——**若失败，MOBILE 为唯一直接嫌疑（Q-T65-01）** |

## 4. 问题清单

### Q-T65-01（延续，第 8 轮本地确认）— E2E-MOBILE 未修复

`33b2d05` 同签名失败。G15 剩余唯一运行期阻塞；归属不变（Desktop/前端）。**今晚 11-24 nightly 是修复后的首个运行期验证窗口**：若失败，失败记录将首次精确指向 MOBILE（此前各晚失败原因各不相同：AG 连/OPSNOISE/AGFIRST/MOBILE/TS6133）。

### 编号竞态（延续）

T-87 双占（#257/#258）已入历史；AR-N 前缀或领号台账提议待用户拍板。本线暂沿用 T- 序列（T-88 查重通过）。

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **G15 编译关修复 + 门范围统一**：nightly 不再死于编译，质量门左移回 PR 层 | **Q-T65-01 MOBILE**：G15/G6 关闭的最后一公里 |
| 设计索引与实现同步（记忆链现状如实入表） | 真实/模拟快照的实时性、真设备路径、远程真站点 live |
| 治理：论文约束解除决策入库（归治理线，本卡存目） | 分层记忆 REQ-03/04（预览与生效闸门、共享知识与内省接入，第 5 线排期） |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、契约正文、断言、登记表、PLAN；未修 E2E-MOBILE（实现线）；治理文档（论文约束解除）内容不在本卡复核范围
- 未跑远程真站点；11-24 nightly 结论待触发后归下轮引用
