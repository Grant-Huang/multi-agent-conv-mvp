# T-89 第 4 线（执行协议与共享契约）验收复跑：方案 C 合并后 + 当日合入链

| 项目 | 内容 |
|---|---|
| 任务 | T-89 第 4 线验收复跑（任务卡未给 ID，此为建议值，**待用户确认**；T-86/T-88 已分别被第 1/2 线占用，避让取 T-89） |
| 主线 | **第 4 线：执行协议与共享契约**（[五线分工](../../decisions/2026-11-22-five-workstreams.md)） |
| 授权来源 | 用户第 4 线角色任务卡（复跑完整业务链路与失败/恢复链路；三层证据）；#264（T-85 方案 C）合并后下一卡 |
| 验收人 | 本 worker；**#264/#249 作者=本会话，其独立验收不在本卡宣称**（宪章原则 4；本卡只做合并后回归，独立验收归第 2 线全量轮或用户） |
| 基线 | `origin/master` `cf8d793`（含 #261～#266；焦点：#264 T-85 方案 C 合并后、#263 trace-id 最小范围、#266 RT-WAPROD-09） |
| 检出方式 | 隔离 worktree `/tmp/t79-wt`（主工作树为其他卡片分支；本卡未触碰主树文件与未跟踪文件） |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·受控模型·进程内服务层；远程真站点不在本轮范围 |
| 运行端口 | harness 18787/18789 被并行会话占用 → 本地错峰 18792/18794（断言未改、补丁已还原）；E2E-RESTART 自管后端 18793 |
| G15 / nightly | **11-23 定时已触发（22:10Z，run 45277117267）：`verify` 作业 failure → 四连败（11-20/21/22/23）**。「master 连续三晚通过」条件未满足，G15 / GR-4-NIGHTLY-E2E 状态**不改**；失败用例定位归第 2 线 G15 监视（候选 Q-T65-01） |
| 证据等级 | 本地所列为 T2；PR CI 为 VERIFIED；master push @ `cf8d793` 收稿时点运行中（回填） |

## 0. 范围依据

第 4 线近期次序已全部交付并两轮回归（T-79/T-84）。本轮 = 方案 C（#264）合并后回归 + 当日其余合入链，重点新覆盖 RT-RESTART-07～11 的浏览器侧稳定性与合同矩阵。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 持久受理 / 重试去重 / 可查分流（desktop_accept + desktop_replay） | **VERIFIED / T2** | 合同 4 + 5 passed |
| WP7 全量（restart_semantics：方案 A 6 用例 + 方案 C 5 用例） | **VERIFIED / T2** | 合同 **11 passed**（重排/开关回滚/家族排除/副作用态失败/人工出口保持/终态保护） |
| WP2 人工验收（review_verdict） | **VERIFIED / T2** | 合同 5 passed |
| 浏览器桌面链（desktop.spec 全套） | **VERIFIED / T2** | 31 passed |
| E2E-RESTART（GS6 重启分支） | **VERIFIED / T2** | 加固后 **2× passed (4.0s)**；过程中一次失败，定位为**测试基建端口交接竞态**（非产品缺陷，见 §1.3），合同 25/25 同基线佐证 |
| E2E-REV / web 单测 / 功能地图 | **VERIFIED / T2** | REV 1 passed；vitest 31/31；test:map 33 features / 92 browser cases valid |
| `E2E-MOBILE` | **仍 FAILED** | 第 6 轮未修（30s 超时同签名），Q-T65-01 挂账延续 |
| PR CI #253/#260～#266 | **VERIFIED** | 已合并者全绿（#263 仅 shadow-review SKIPPED，无害）；#265（第 5 线 S2b）OPEN 在途 |
| master push @ `cf8d793` | 运行中 | run 45335684975（前序 3 条 cancelled 为排队取代，属预期） |
| nightly | **failure** | 11-23 已触发且失败（见上）；G15 不变 |

**总体**：方案 C 合并后第 4 线全链路回归 **PASS**（含新重排链合同+浏览器双确认）；本轮唯一失败经定位为测试基建竞态并已加固；E2E-MOBILE 与 nightly 为已知挂账延续。

---

## 1. 本地证据（worktree @ `cf8d793`，binary 与 web/dist 全新构建）

### 1.1 合同层（4 套件 25/25）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_accept --test desktop_replay \
    --test restart_semantics --test review_verdict
desktop_accept:      4 passed    desktop_replay:     5 passed
restart_semantics:  11 passed    review_verdict:     5 passed
合计 25 passed / 0 failed
```

### 1.2 浏览器 E2E 与 web

```text
$ npx playwright test e2e/desktop.spec.ts e2e/restart.spec.ts
  31 passed (24.6s) + E2E-RESTART 首跑 1 failed（见 §1.3）
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE'
✓ E2E-REV 1 passed          ✘ E2E-MOBILE 1 failed (30s timeout)
$ npm test                  vitest 10 files / 31 tests passed
$ npm run test:map          Feature map valid: 33 features, 92 browser cases
```

### 1.3 E2E-RESTART 首跑失败——定位为测试基建竞态（非产品缺陷）

| 步骤 | 结果 |
|---|---|
| 首跑失败签名 | `backend exited early: 1`（restartServer 自管后端启动即退出） |
| 排除产品回归 | ① 同基线合同 25/25（覆盖同一条 assemble 启动路径）；② 用遗留 tmp 目录的手工配置直接启动 `agentnexus`：**正常起服并持续服务** |
| 根因 | 重启后端口交接窗口：旧进程退出后的 TIME_WAIT 使新进程 bind 失败即退（200ms 间隔偶发不足；[testing/README「子进程HTTP服务就绪」](../testing/README.md)记载的同类交接问题） |
| 加固 | `restartServer.ts` boot() 对「启动即退出」做有界重试（3 次、1s 退避；30s 健康门不变）——只重试基础设施就绪，不重试业务断言 |
| 验证 | 加固后 **2× passed (4.0s)** |

### 1.4 E2E-MOBILE（Q-T65-01 跟踪，第 6 轮）

`cf8d793` 仍 failed（30s 超时同签名）；无修复提交。归属不变（桌面/前端 + G15 根因合并处理）。

### 1.5 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理回执 / 幂等 / 409 / 丢回执回读 | 通过 | rt_adaccept_01～04、E2E-HOMEACCEPT/ADACCEPT |
| SSE 重放 / 去重 / 410 / 重启对账 / 不串会话 | 通过 | rt_adreplay_01～05、E2E-ADRESUME |
| 中断恢复原因事件与失败交付（方案 A） | 通过 | rt_restart_01/02/09、E2E-RESTART |
| **未起跑单任务重排（方案 C，新）** | 通过 | rt_restart_07/08 + 浏览器 E2E-RESTART |
| **开关回滚路径（新）** | 通过 | rt_restart_10（false → 全失败 + 无重排事件） |
| **模板家族不重驱（新）** | 通过 | rt_restart_11（root/branch/join 仍失败恢复） |
| 跨重启人工出口 + 终态保护 | 通过 | rt_restart_03～06 |
| 判定双出口 / 终态回传 | 通过 | rt_rev_01～05、E2E-REV/HOMEREV/HOMEREJ |
| Mock / 受控模型可辨 | 通过 | harness 受控 LLM；自管 mock 同口径；远程真站不宣称 |
| 可追踪性 | 通过 | 会话/任务/事件 id 贯通；重排/中断均有原因事件；state.json 记录 baseline |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #253 | T-84 第 4 线验收记录 | pass（8/8，已合并） |
| #260 | WA M4 门禁 / M3.5 进度（docs） | pass（8/8，已合并） |
| #261 | T-88 第 2 线验收记录 | pass（8/8，已合并） |
| #262 | M3 Odoo 试点评审（docs） | pass（8/8，已合并） |
| #263 | trace-id 最小范围（docs + doc_lint） | pass（shadow-review SKIPPED 属无害，已合并） |
| #264 | **T-85 WP7 方案 C（本线）** | pass（8/8，已合并） |
| #265 | 第 5 线 convmem S2b | OPEN 在途（rust 跑动中，非本卡范围） |
| #266 | RT-WAPROD-09 源响应无效 502 合同补录 | pass（8/8，已合并） |

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `cf8d793`（#266） | 运行中（run 45335684975，收稿时点未出结论；前序 `8f8d3d2`/`1aad409`/`b096851` 三条 **cancelled** 为新 push 排队取代，属预期形态，非失败） |
| nightly 11-23（run 45277117267，22:10Z 触发） | **failure**（`verify` 作业）；具体失败用例定位归第 2 线 G15 监视（候选 Q-T65-01） |
| G15 / GR-4-NIGHTLY-E2E | 四连败（11-20/21/22/23），「连续三晚通过」未满足，状态**不改** |

## 4. 问题清单

### Q-T65-01（延续，第 6 轮）— E2E-MOBILE 未修复
`cf8d793` 仍 failed。它是 nightly 关闭的唯一阻塞候选；归属与建议见 `t65-cross-group-acceptance.md` §4。

### Q-T89-01（已当场处置）— restartServer 端口交接竞态（测试基建，轻微）
自管重启环境偶发 `bind` 失败即退出（TIME_WAIT 窗口）。已按 testing/README 就绪交接纪律加固（有界重试），2× 复跑稳定。归本线测试基建，随本 PR 一并入库。

### 观察 — 本轮无新增产品缺陷
#264（方案 C）合并后合同 25/25 + 浏览器重排链双确认；#263/#266 合入未破坏第 4 线链路。

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节）。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 受理→执行→审查→判定→终态回传 + 重启「明确失败」与「未起跑重排」双语义（WP7 A+C 收官） | 断点续跑不做（决策维持）；模板家族级恢复如需另卡 |
| GS6 重启分支合同 11 用例 + 浏览器稳定复跑 | GS6 外发合规部分归 WP6（未开工） |
| 合同 25/25 + 浏览器 32+2 + 地图校验通过 | E2E-MOBILE 挂账（Q-T65-01）；nightly 四连败（G15 不变） |
| PR CI 全绿、master push 收尾中 | 远程真 DES/站点 live（Q-T60-02，非本轮目标） |

## 6. 非目标（本卡遵守）

- 未改业务代码与断言；主工作树并行在途改动与未跟踪文件未触碰
- 不对 #249/#264 宣称独立验收（作者=本会话；本卡为合并后回归，独立验收归第 2 线/用户）
- 未跑远程真站点 live；未定位 E2E-MOBILE 根因；nightly 失败用例归属不越界（归第 2 线监视）
- 本 PR 附带唯一代码改动为 restartServer 测试基建加固（§1.3，已披露），其余为验收记录 docs
