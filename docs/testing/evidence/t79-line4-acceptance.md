# T-79 第 4 线（执行协议与共享契约）协议链独立复验

| 项目 | 内容 |
|---|---|
| 任务 | T-79 第 4 线协议链独立复验（任务卡未给 ID，此为建议值，**待用户确认**） |
| 主线 | **第 4 线：执行协议与共享契约**（[五线分工](../../decisions/2026-11-22-five-workstreams.md)） |
| 授权来源 | 用户第 4 线角色任务卡：复跑完整业务链路与失败/恢复链路，核对副作用、回执、版本绑定、Mock 标记与可追踪性，分别记录本地 / PR CI / master 定时证据 |
| 验收人 | 本 worker（未参与 #235/#236/#237/#238 实现）；独立复核待用户或第 2 线 |
| 基线 | `origin/master` `ba9a7c7`（含 #235 首页委派切片、#237 T-78 记忆持久化、#238 终态交付回发起会话；本轮执行期间 master 由 `2c59a3a` 前进到 `ba9a7c7`，已 rebase 并全量复跑） |
| 检出方式 | **隔离 worktree `/tmp/t79-wt`**（主工作树为其他卡片分支；本卡未触碰主树任何文件与未跟踪文件） |
| 日期 | 2026-11-23 |
| 口径 | 合同 / Playwright harness = fixture·受控模型·mock ACP·进程内服务层；远程真站点不在本轮范围 |
| 运行端口 | **18792**（错峰：18787 被并行会话 E2E 占用；仅在 worktree 内改 `playwright.config.ts` / `harness.mjs` 的端口字面量，**断言未改**，改动未提交、已还原） |
| G15 / nightly | 最新仍为 11-22 失败（run 45052408864）；11-23 定时截至本报告**未触发**。「master 连续三晚通过」条件未满足，G15 / GR-4-NIGHTLY-E2E 状态**不改** |
| 证据等级 | 本地所列为 T2（真实执行记录，通过/失败数可核对）；PR CI 为 VERIFIED；master push @ `a32e6b0`/`ba9a7c7` 均 success，见 §3 |

## 0. 范围依据（第 4 线近期次序）

[集成计划](../../INTEGRATION_PLAN.md)五线表第 4 线：先验**持久受理、重试去重、可查分流**；再把已合入的执行后人工验收（WP2，#215）纳入桌面主路径（GS3 日常入口 `E2E-HOMEREV`）；补终态结果可靠回传（**#238 本轮执行期间已合入 `ba9a7c7`，纳入复验**）。运行中恢复按 WP7 分期（本轮只验既有已实现语义：同库重启对账 `rt_adreplay_04`）。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 持久受理链 `E2E-HOMEACCEPT` + desktop_accept 合同 | **VERIFIED / T2** | 浏览器 1 passed；合同 4 passed |
| 重试去重与冲突 `E2E-ADACCEPT` | **VERIFIED / T2** | 浏览器 1 passed（丢回执回读、同 op 幂等、409 冲突不二次派发） |
| 断线重放/重启 `E2E-ADRESUME` + desktop_replay 合同 | **VERIFIED / T2** | 浏览器 1 passed；合同 5 passed（重放、去重、410 恢复、重启对账、不串会话） |
| GS3 人工验收桌面主路径 `E2E-HOMEREV` + `E2E-REV` + review_verdict 合同 | **VERIFIED / T2** | 浏览器 2 passed；合同 5 passed；#238 强化后的断言（`task_id` 绑定交付、`result` 含产物/证据、刷新后仍可断言）通过 |
| **终态失败交付回发起会话 `E2E-HOMEREJ`（#238 新增）** | **VERIFIED / T2** | 浏览器 1 passed：拒绝→返工→二次拒绝→FAILED，失败交付 durable 写入发起会话，刷新后仍可断言 |
| web 单测（含 homeDelegate 终态回传用例） | **VERIFIED / T2** | vitest 10 files / 31 tests passed |
| `E2E-MOBILE` | **仍 FAILED** | `ba9a7c7` 上 1 failed → Q-T65-01 未修复，挂账延续 |
| PR CI #235/#236/#237/#238 | **VERIFIED** | 各 8/8 SUCCESS |
| master push @ `ba9a7c7` / `a32e6b0` | **VERIFIED** | run 45231143046 / 45231108751 均 **success** |
| nightly（11-23） | **未触发** | 最新 11-22 失败；G15 状态不变 |

**总体**：第 4 线近期次序全部已合入项（持久受理、重试去重、可查分流、GS3 桌面主路径、终态回传通过/拒绝双出口）独立复验 **PASS**；E2E-MOBILE 缺陷与 nightly 失败为已知挂账延续，无新增业务缺陷。

---

## 1. 本地证据（worktree @ `ba9a7c7`，target 与 web/dist 全新构建）

### 1.1 合同层（3 套件 14/14）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_accept --test desktop_replay --test review_verdict
desktop_accept:   4 passed   (rt_adaccept_01 202+ids / 02 同 op 幂等不二次派发 /
                             03 同 op 改正文冲突 / 04 丢回执后 GET 回读)
desktop_replay:   5 passed   (rt_adreplay_01 SSE 重放后接 live / 02 重复事件 id 去重 /
                             03 游标过期 410 / 04 重启从 SQLite 对账 / 05 并行任务不串会话)
review_verdict:   5 passed   (rt_rev_01 判定通过 COMPLETED / 02 退回返工 / 03 轮次耗尽失败 /
                             04 终态后判定冲突 / awaiting_approval_cannot_review_verdict)
合计 14 passed / 0 failed（`2c59a3a` 首跑与 `ba9a7c7` 复跑结果一致）
```

### 1.2 浏览器 E2E（正常桌面入口，端口 18792）

```text
$ cd web && npx playwright test e2e/desktop.spec.ts \
    -g 'HOMEACCEPT|HOMEREV|HOMEREJ|ADRESUME|ADACCEPT'
✓ E2E-HOMEACCEPT / E2E-HOMEREV / E2E-HOMEREJ / E2E-ADRESUME / E2E-ADACCEPT
                                                                5 passed (6.4s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE'
✓ E2E-REV：审查待验收后判定通过                                1 passed
✘ E2E-MOBILE：窄屏会话抽屉                                      1 failed (30s timeout)
$ npm test                                                      vitest 10 files / 31 tests passed
```

首轮在 `2c59a3a` 上曾出现 4 failed，根因是全新 worktree 缺 `web/dist` 与 `target/debug/agentnexus` / `an-mock-acp`（页面空白导致超时）；补 `npm run build` + `cargo build -p an-server` 后复跑全绿，与业务无关。master 前进到 `ba9a7c7` 后 rebase 全量复跑（含重建二进制与 dist），结果如上。

### 1.3 E2E-MOBILE（Q-T65-01 跟踪）

`ba9a7c7`（#235～#238 合入后）仍 failed，失败签名与 T-65 记录一致（30s 超时）；无相关修复提交。维持 Q-T65-01 归属与建议（桌面/前端实现组修复 + G15 根因合并处理）。

### 1.4 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理回执 202 + operation/task id | 通过 | rt_adaccept_01、E2E-HOMEACCEPT（芯片为真实 task id） |
| 丢回执只信 GET 回读 | 通过 | rt_adaccept_04、E2E-HOMEACCEPT |
| 同 `client_operation_id` 幂等，dispatch_count=1 | 通过 | rt_adaccept_02、E2E-ADACCEPT |
| 同 op 改正文 409 冲突且不二次派发 | 通过 | rt_adaccept_03、E2E-ADACCEPT |
| SSE 重放后接 live、重复事件 id 去重 | 通过 | rt_adreplay_01/02、E2E-ADRESUME（A/B 状态不串） |
| 游标过期 410 → 恢复续读无漏无重 | 通过 | rt_adreplay_03 |
| 同库重启后从 SQLite 对账（既有语义） | 通过 | rt_adreplay_04（运行中恢复 WP7 分期，未开工，不在本轮） |
| 并行任务不串会话 | 通过 | rt_adreplay_05、E2E-ADRESUME |
| 待验收判定通过 → COMPLETED、交付绑定 `task_id` 回发起会话 | 通过 | rt_rev_01、E2E-REV、E2E-HOMEREV（含 #238 强化断言：`GET messages` 有 `task_id` 绑定的 Composer 交付、`result` 含产物/证据、刷新后仍可断言） |
| 判定不通过 → 返工同会话；二次拒绝 → FAILED 且失败交付 durable | 通过 | rt_rev_02/03、E2E-HOMEREJ（#238 新增；对齐 RT-REV-02/03） |
| 终态冲突与状态守卫 | 通过 | rt_rev_04、awaiting_approval_cannot_review_verdict |
| Mock / 受控模型可辨 | 通过 | harness 受控 LLM + mock ACP（`AN_MOCK_ACP_REPLY`）+ 夹具，口径 = fixture·Mock；远程真站不宣称 |
| 可追踪性 | 通过 | 会话/任务/事件 id 贯通（E2E-HOMEACCEPT/HOMEREV/HOMEREJ 断言 `task_id` 绑定与刷新恢复）；harness `runtime/state.json` 记录 baseline SHA |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #235 | 首页 S1 真实委派切片（B1 受理 + GS3） | pass（8/8 SUCCESS） |
| #236 | T-78 记忆持久化表方案（docs） | pass（8/8 SUCCESS） |
| #237 | T-78 记忆持久化实现（已合入 `a32e6b0`） | pass（8/8 SUCCESS） |
| #238 | 首页委派终态交付回发起会话（已合入 `ba9a7c7`） | pass（8/8 SUCCESS） |

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `2c59a3a`（#236 合并） | **success**（run 45226186879） |
| push @ `a32e6b0`（#237 合并） | **success**（run 45231108751） |
| push @ `ba9a7c7`（#238 合并） | **success**（run 45231143046；rust 作业收尾后全绿，其余作业先行 success） |
| nightly（11-23 UTC 收集时点前） | **未触发**；最新仍为 11-22 失败（run 45052408864） |
| G15 / GR-4-NIGHTLY-E2E | 状态**不变**：三连败（11-20/21/22），「连续三晚通过」未满足 |

## 4. 问题清单

### Q-T65-01（延续）— E2E-MOBILE 未修复

`ba9a7c7` 复跑仍 failed（30s 超时，签名同 T-65）。G15 三连败的根因候选不变；归属与建议见 `t65-cross-group-acceptance.md` §4（桌面/前端实现组修复 + 与 G15 根因合并处理）。

### 观察 — 并行会话共用 18787 端口（非缺陷，协调事项）

本轮执行时 18787 被并行会话的 E2E 服务占用（短窗口内出现两个不同 PID）。[集成计划坑 2](../../INTEGRATION_PLAN.md) 已有「新增用例用 18792 起」的规约；本轮按该规约用 18792 错峰完成，不影响他人。建议各线并行复跑时主动错峰，避免 webServer 互踩。

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节）。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 首页委派可靠受理全链（202/回读/幂等/重连原会话）合同 + 浏览器双绿 | —（本轮收口） |
| GS3 人工验收桌面主路径：判定**通过与拒绝双出口**、终态交付/失败交付均回发起会话且刷新后可断言（#238） | 运行中恢复按 WP7 分期未开工（重启仅验既有对账语义） |
| 断线重放/重启对账/不串会话合同 5/5 绿 | E2E-MOBILE 缺陷未修（Q-T65-01）；nightly 三连败（G15） |
| PR CI（#235～#238）与 master push（`a32e6b0`/`ba9a7c7`）全绿 | 远程真 DES/站点 live（Q-T60-02，非本轮目标） |

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务代码、契约正文、`features.json` 断言、GUARDRAIL_REGISTRY、PLAN/CASES；主工作树的并行在途改动与未跟踪文件未触碰
- 未验收 T-78 记忆持久化的业务语义（#237 属第 5 线地基，本线只确认其合入未破坏第 4 线链路——复跑全绿）
- 未跑远程真站点/真 DES live；未做 WP7 运行中恢复；未定位 E2E-MOBILE 根因（实现组职责）
- 端口补丁（18787→18792）仅存在于本地 worktree，已还原，不入库
