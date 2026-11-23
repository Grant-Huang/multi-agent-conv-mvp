# T-84 第 4 线（执行协议与共享契约）验收复跑：T-81 重启语义 + 当日合入链

| 项目 | 内容 |
|---|---|
| 任务 | T-84 第 4 线验收复跑（任务卡未给 ID，此为建议值，**待用户确认**；T-82 已被第 1 线在途分支占用、T-83 归第 2 线，避让取 T-84） |
| 主线 | **第 4 线：执行协议与共享契约**（[五线分工](../../decisions/2026-11-22-five-workstreams.md)） |
| 授权来源 | 用户第 4 线角色任务卡（复跑完整业务链路与失败/恢复链路；三层证据）；#249（T-81）合并后下一卡 |
| 验收人 | 本 worker；**注：#249（T-81 实现）作者即本会话，其独立验收不在本卡宣称**——本卡只做「合并后链路不破」的回归复跑，#249 自身的独立验收由第 2 线全量轮或用户承担（宪章原则 4：执行者不当自己的裁判） |
| 基线 | `origin/master` `1a9ae70`（含 #241～#249；本轮焦点：#249 T-81 重启语义、#248 M3 观察 fixture、#247 harness 深就绪门） |
| 检出方式 | 隔离 worktree `/tmp/t79-wt`（主工作树为其他卡片分支；本卡未触碰主树文件与未跟踪文件） |
| 日期 | 2026-11-23 |
| 口径 | 合同 / Playwright harness = fixture·受控模型·进程内服务层；远程真站点不在本轮范围 |
| 运行端口 | harness 18787（本轮空闲）；E2E-RESTART 自管后端 18793（[restartServer.ts](../../../web/e2e/helpers/restartServer.ts)，`[acp] auto_detect=false`） |
| G15 / nightly | 11-23 定时截至本报告**仍未触发**（第 2 线监视至 ~21:50Z）；最新 11-22 失败（run 45052408864）。「三晚连过」未满足，G15 / GR-4-NIGHTLY-E2E 状态**不改** |
| 证据等级 | 本地所列为 T2；PR CI 为 VERIFIED；master push @ `c2b2be9`/`1a9ae70` 收稿时点运行中（监视回填） |

## 0. 范围依据

第 4 线近期次序（[集成计划](../../INTEGRATION_PLAN.md)五线表）已全部交付：持久受理/重试去重/可查分流（T-79，PR #239 已合并）、GS3 桌面主路径与终态回传双出口（#238）、WP7 重启语义（T-81，#249）。本轮 = 合并后回归复跑 + 三层证据，覆盖当日全部合入链。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 持久受理 / 重试去重 / 可查分流（desktop_accept + desktop_replay） | **VERIFIED / T2** | 合同 4 + 5 passed |
| WP7 重启语义（restart_semantics，T-81 新套件） | **VERIFIED / T2** | 合同 6 passed（中断恢复原因事件/失败交付 + 跨重启判定/返工/批准基线 + 终态保护） |
| WP2 人工验收（review_verdict） | **VERIFIED / T2** | 合同 5 passed |
| 浏览器桌面链（desktop.spec 全套） | **VERIFIED / T2** | 31 passed（含 E2E-HOMEACCEPT/HOMEREV/HOMEREJ/ADRESUME/ADACCEPT） |
| E2E-RESTART（GS6 重启分支，T-81） | **VERIFIED / T2** | 1 passed（执行中重启→FAILED+失败交付；待验收重启→判定通过→COMPLETED） |
| E2E-REV | **VERIFIED / T2** | 1 passed |
| web 单测 / 功能地图 | **VERIFIED / T2** | vitest 31/31；test:map **33 features / 92 browser cases valid** |
| `E2E-MOBILE` | **仍 FAILED** | `1a9ae70` 上 1 failed（30s 超时同签名）→ Q-T65-01 第 5 轮未修，挂账延续 |
| PR CI #241～#249 | **VERIFIED** | 全部 8/8 SUCCESS（#245 CLOSED，无未通过项） |
| master push | **VERIFIED** | `cf6b508`（run 45253023534）、`c2b2be9`（run 45257605418）、`1a9ae70`（run 45257708316）均 **success** |
| nightly（11-23） | **未触发** | G15 状态不变 |

**总体**：T-81 合并后第 4 线全部链路回归 **PASS**，无新增业务缺陷；E2E-MOBILE 与 nightly 为已知挂账延续。

---

## 1. 本地证据（worktree @ `1a9ae70`，binary 与 web/dist 全新构建）

### 1.1 合同层（4 套件 20/20）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_accept --test desktop_replay \
    --test restart_semantics --test review_verdict
desktop_accept:      4 passed    desktop_replay:     5 passed
restart_semantics:   6 passed    review_verdict:     5 passed
合计 20 passed / 0 failed
```

### 1.2 浏览器 E2E（正常桌面入口）

```text
$ npx playwright test e2e/desktop.spec.ts e2e/restart.spec.ts
  32 passed (28.6s)          ← desktop 31 + E2E-RESTART 1
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE'
✓ E2E-REV                    1 passed
✘ E2E-MOBILE                 1 failed (30s timeout)
$ npm test                   vitest 10 files / 31 tests passed
$ npm run test:map           Feature map valid: 33 features, 92 browser cases
```

harness `runtime/state.json` 记录 baseline = `1a9ae7016c6e…`。

### 1.3 E2E-MOBILE（Q-T65-01 跟踪，第 5 轮）

`1a9ae70` 仍 failed（30s 超时，签名同 T-65）；无修复提交。归属不变（桌面/前端 + G15 根因合并处理）。

### 1.4 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理回执 / 幂等 / 409 冲突 / 丢回执回读 | 通过 | rt_adaccept_01～04、E2E-HOMEACCEPT/ADACCEPT |
| SSE 重放 / 去重 / 410 恢复 / 重启对账 / 不串会话 | 通过 | rt_adreplay_01～05、E2E-ADRESUME |
| **中断恢复写原因事件与失败交付（新）** | 通过 | rt_restart_01/02、E2E-RESTART（会话可见「任务因服务重启中断，可重新委派」，绑定 `task_id`） |
| **跨重启人工出口保持（新）** | 通过 | rt_restart_03（判定→COMPLETED）/04（返工额度按库扣减）/05（批准→执行）；终态不被触碰（rt_restart_06） |
| 判定通过/拒绝双出口、终态回传 | 通过 | rt_rev_01～05、E2E-REV/HOMEREV/HOMEREJ |
| Mock / 受控模型可辨 | 通过 | harness 受控 LLM；E2E-RESTART 自管 mock 模型口径同 harness；远程真站不宣称 |
| 可追踪性 | 通过 | 会话/任务/事件 id 贯通；重启原因事件入时间线；state.json 记录 baseline SHA |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #241 | T-78 验收记录（docs） | pass（8/8） |
| #242 | T-80 全量轮验收记录（docs） | pass（8/8） |
| #243 | convmem S1 会话材料（第 5 线） | pass（8/8） |
| #244 | M3 live observe 评审（docs） | pass（8/8） |
| #245 | （已关闭） | 无未通过项 |
| #246 | T-83 验收记录（docs） | pass（8/8） |
| #247 | harness 深就绪门（Q-T67-02 修复） | pass（8/8） |
| #248 | M3 观察 fixture（正式 Observe 路由） | pass（8/8） |
| #249 | **T-81 WP7 重启语义（本线）** | pass（8/8） |

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `cf6b508`（#247） | **success**（run 45253023534） |
| push @ `c2b2be9`（#248） | **success**（run 45257605418） |
| push @ `1a9ae70`（#249，T-81 合并） | **success**（run 45257708316，rust 作业收尾后全绿） |
| nightly（11-23，收集时点） | **未触发**；最新 11-22 失败（run 45052408864）；第 2 线监视至 ~21:50Z |
| G15 / GR-4-NIGHTLY-E2E | 状态**不变**：三连败（11-20/21/22），「连续三晚通过」未满足 |

## 4. 问题清单

### Q-T65-01（延续，第 5 轮）— E2E-MOBILE 未修复

`1a9ae70` 复跑仍 failed（30s 超时，签名同 T-65）。归属与建议见 `t65-cross-group-acceptance.md` §4。它是当前 nightly 关闭的唯一阻塞项。

### 观察 — 本轮无新增缺陷

T-81 合并（含 master 两次前进的 rebase 场景）与 #247 harness 深就绪门、#248 M3 fixture 合入后，第 4 线全部链路（含新增重启链）回归绿。

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节）。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 受理→执行→审查→判定→终态回传全链 + 重启明确失败/人工出口保持（WP7） | 断点续跑不做（决策 2026-11-23-wp7-restart-semantics）；QUEUED 重排（方案 C）另卡待授权 |
| GS6 重启分支浏览器化（E2E-RESTART） | GS6 外发合规部分归 WP6（未开工） |
| 合同 20/20 + 浏览器 33 绿 + 地图校验通过 | E2E-MOBILE 挂账（Q-T65-01）；nightly 未触发（G15 不变） |
| PR CI #241～#249 全绿 + master push `cf6b508`/`c2b2be9`/`1a9ae70` 全 success | E2E-MOBILE 挂账（Q-T65-01）；nightly 未触发（G15 不变） |

## 6. 非目标（本卡遵守）

- 未改任何代码、契约、断言、PLAN、GUARDRAIL_REGISTRY（docs-only 验收记录）
- **不对 #249/T-81 宣称独立验收**（作者=本会话；回归复跑 ≠ 独立验收，独立验收归第 2 线/用户）
- 未跑远程真站点/真 DES live；未定位 E2E-MOBILE 根因；master push 运行中结论已回填（均 success）
