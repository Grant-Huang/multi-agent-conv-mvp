# AR-12 跨组场景独立验收（主线 #2：审查发现派工轮 + mcp 抖动样本 #5 + UX-08 地图验证）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收（集成协调）** |
| 授权来源 | 用户「从 GitHub 领取本线 issues，从严重度最高开始修」指示 → 查证 `dispatch + line:2` 队列为空（#312 派工表「第 2 线暂无」）→ **用户拍板：维持分工，开 AR-12 验收轮**（2026-11-26） |
| 验收人 | 本 worker（未参与 #355/#356/#357 实现） |
| 基线 | `origin/master` `6e784a4`（#350 ADREAD 收尾、#352 AR-11 证据、#355 UX-08 地图、#356 审查 2026-11-26、#357 M1/H4 待批方案） |
| 检出方式 | 隔离 worktree `/private/tmp/ar12-wt` @ `6e784a4`，独立 `CARGO_TARGET_DIR=/private/tmp/ar12-target`；主树他卡未提交改动（`cursor/convmem-s3-intro-link`）未触碰 |
| 日期 | 2026-11-26（UTC） |
| 口径 | 合同 / harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 形态 2 红（ADREAD 已修 / UX05=Q-AR9-01）；**11-26 定时截至收稿（~17:1xZ）未落地**，已挂监视，终态回填 §4 |
| 证据等级 | 本地为 T2；master push #352 红 / #357 绿（§3）；nightly 回填见 §4 |

## 0. 本轮范围（`a383e26` → `6e784a4`）与轮型判定

| PR | 内容 | 性质 |
|---|---|---|
| #350 | E2E-ADREAD 锚点修复收尾（`web/e2e/ops.spec.ts`） | AR-11 已复验（ops 7/7），无新增面 |
| #352 | AR-11 证据合入（本线上一卡） | docs |
| #355 | **UX-08 纠正/撤回个人记忆旅程**（INTENT-030～032 双向检查基线审查） | docs（journeys.json + 审查文档） |
| #356 | **代码审查 2026-11-26**：新发现 13 条（P1×1 已复现 / P2×8 / P3×4）+ 3 项待用户决策 | docs |
| #357 | R-20261124-H4 store 回归方案 + R-20261125-M1 方案（**均待用户批准，未改代码**） | docs |

**轮型：轻量轮**——`git diff a383e26..6e784a4` 非 docs 改动仅 `web/e2e/ops.spec.ts`（AR-11 已覆盖）。合同面免重跑，沿用 AR-10/AR-11 基线；佐证：#356 评审在 `f419500` 自跑 fmt/clippy/cargo test（850 passed）/web 全套/tools pytest（114 passed）全绿。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| UX-08 旅程登记（#355） | **VERIFIED / T2** | test:map 全绿（node --test 15 pass）；map valid 33 features / 99 cases；**experience valid：8 journeys、欠账 28 条透明**（AR-11 为 7 journeys / 24 条——UX-08 新增不变量暂无用例，欠账如实可见；CH-U6/CH-U7 未覆盖延续） |
| #356 审查发现派工 | **DONE** | 13 条全部落卡 **#358～#369**（§2 台账）；GitHub 上此前无任何 R-20261126 卡（派工前查重通过） |
| mcp.rs 抖动样本 #5 | **RECORDED** | #352 push（纯 docs）rust 红 ×2（§3）；本地同族 10/10 绿对照；已回贴第 4 线伞卡 [#343 comment](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/343#issuecomment-7198680813) |
| 合同面 | CARRIED-OVER | 代码零增量（§0 判定），AR-10/AR-11 基线维持 |
| master push | 分裂态 | `47b8cd5`（#352）**failure**（rust，Q-AR2-04 样本 #5）；`6e784a4`（#357，1 分钟后）**success**（run 45859754403） |
| nightly 11-26 | PENDING | 未落地，监视中（§4） |

**总体**：**PASS（轻量轮）**——本线无新缺陷发现；主要产出为 #356 审查发现的**派工闭环**（13 卡 + 2 项待用户决策阻塞标注）与 Q-AR2-04 第 5 样本取证。

## 1. 本地证据（worktree @ `6e784a4`，独立 target）

```text
$ cd web && node --test verification/check-map.test.mjs verification/check-experience.test.mjs
  → # pass 15 / # fail 0
$ node verification/check-map.mjs → Feature map valid: 33 features, 99 browser cases
$ node verification/check-experience.mjs
  → Experience map valid: 8 journeys, 28 invariant(s) without case (debt); not yet covered: CH-U6, CH-U7
$ CARGO_TARGET_DIR=/private/tmp/ar12-target cargo test --locked -p an-agents --test mcp
  → 10 passed; 0 failed（含 CI 红掉的 expired_http_session… 与 streamable_http_handshake… 两例）
```

UX-08 旅程（journeys.json）：charter=CH-G1/CH-G2/CH-G5/CH-U2，features=ai-desktop+conversation-memory；I1（记忆列表与详情）已映射 E2E-ADMEM，其余不变量待实现卡（#353/#354 在账）补用例——地图先行、欠账透明，符合 AR-6 试点口径。

## 2. #356 审查发现派工台账（13/13）

| Issue | 发现 | 线 | 级 | 阻塞/约束 |
|---|---|---|---|---|
| #358 | R-20261126-H1（P1 已复现）撤回记忆仍进待批任务执行者 | 5 | high | ⚠️等用户决策①；触 orchestrator 先出方案 |
| #359 | M7 conv_lock.rs:121 断言改 `(false,1)` | 4 | med | 高风险路径先出方案；**AR-10 证据 20/30 行更正由本线随其合入执行** |
| #360 | M6 外部 A2A exec 块外泄疑点（先复现） | 4 | med | 对外发送=先出方案 |
| #361 | M4 已读表加 user_id（store 先出方案附迁移） | 4 | med | 高风险 |
| #362 | M1 conversation.rs:575 四开关取或、digest=judge‖route | 5 | med | 与 #358 同文件错峰 |
| #363 | M2 session_materials title 截断 + max_tokens 整裁 | 5 | med | — |
| #364 | L1 delegate 埋点 exec_chars/INJECTED + DESIGN 10.3 | 5 | low | — |
| #365 | M3 desktop_api 两 fixture/reset 口入 test_fixtures 门 | 1 | med | 与台账 R-20261124-M8 一并修 |
| #366 | M8 恢复「执行区收起重进保持」断言 | 1 | med | ⚠️等用户决策② |
| #367 | L2 WorkspaceComposer loadAudits taskRef 守卫 | 1 | low | — |
| #368 | L3a S5/S6 决策补「背景、问题」小节 | 5 | low | — |
| #369 | L3b DESIGN 11.1 补登记三表 + ai-desktop DESIGN:104 | 1 | low | — |
| （不派卡） | L4：#337 改 acp.rs 无事先方案/认领记录 | — | P3 | 流程警示，记录在案；后续 acp.rs PR 须指向已批准方案 |

另：评审「需要人决定的事」第 3 条（FIX-A 门维持）见 §5；2 条与台账同源未开新卡（评审自记，驳回 0）。

## 3. PR CI / master push 三层证据

| 层 | 对象 | 结果 |
|---|---|---|
| 本地 | test:map + mcp.rs（独立 target） | 全绿（§1） |
| PR CI | #355/#357 push | success（run 45829512754 等） |
| master push | `47b8cd5`（#352）run 45859611420 | **failure**：rust `cargo test`，`mcp.rs` 两例同刻 panic（:350/:190）——纯 docs push 上复现=非业务回归 |
| master push | `6e784a4`（#357）run 45859754403 | success（与前者相隔 1 分钟的并发 push 窗口，一红一绿） |
| nightly | 11-25 run 45691251266 | failure（ADREAD+UX05 双红；ADREAD 已于 #349/#350 修复并经 AR-11 复验） |

**Q-AR2-04 累计样本**：rt_restart_09（#269 记录）→ mcp expired_http_session（AR-2）→ conv_lock（AR-6）→ ETXTBSY（#343/#339）→ **本次 mcp.rs ×2（docs push）**。#335 判据范式（等待分支语义化断言）已复制到 conv_lock（#359 在途），建议第 4 线同法覆盖 mcp.rs 会话家族。

## 4. nightly / G15 轨迹与监视

- G15 三晚连绿计数：**0/3**（11-25 失败）。ADREAD 修复后，若 11-26 形态收敛至仅 UX05（Q-AR9-01，CI 抖动候选），则具备评估条件。
- **11-26 定时截至本卡收稿（~17:1xZ）未落地**（cron 10:30Z，历史落地 17:4xZ～22:1xZ）。已挂后台监视，运行出现并终态后**回填本节**。

## 5. 待用户决策与待批方案（主轴差距清单）

**需要用户拍板（评审「需要人决定的事」，对应阻塞卡）：**
1. **H1 撤回语义**：撤回后已存任务 context 是否失效？评审推荐：**是，按宪章 CH-G2e**，修订 S6 决策第 4 条 → 阻塞 #358。
2. **M8 断言去留**：恢复「执行区收起重进保持」E2E 断言，还是从 CASES 删？评审推荐：**恢复**（desktop.spec 播种 SMT-03）→ 阻塞 #366。
3. **FIX-A 门**：维持「M4 生产写入开工前必须先合 FIX-A（H1/H2/H5）」？评审推荐：**维持**（同 11-25 报告）。

**待批方案（fixer 已提交、明确等批准，未改任何代码）：**
- `docs/fix-plans/r-20261124-h4-store-regression.md`（H4 确定性根因回归，#357）
- `docs/fix-plans/r-20261125-m1.md`（按 S5/S6 现状收窄为回答门控，#357）
- #336 nightly 确定性方案 docs（AR-10 起，仍待批）

**其他在账**：ADD01-02 间歇（#351，第 1 线 worktree 在途）；UX05 CI 抖动（Q-AR9-01）；GR-4 例外到期评审 **2026-11-28**（owner：用户）。

## 6. 主轴差距（宪章 v1.4 口径：面向组织的 AI 工作桌面，制造业差异化）

- **受控闭环**：nightly 收敛是 GR-4 主门槛——当前 0/3 连绿，今晚（11-26）为 ADREAD 修复后首个潜在收敛夜；ADREAD 本身已收官。
- **宪章约束级风险**：CH-G2e 违反项（R-20261126-H1，已复现）在账未修，是当前唯一 P1——阻塞在用户决策①，建议尽快拍板。
- **生产写入**：M4 仍被 FIX-A 门控（决策③）；审查台账 P0/P1 共 11 条 open，无 fix_claimed 在途 PR（评审核对）。
- **体验线**：UX-08 地图先行达标（欠账透明）；体验反目标 CH-U6/U7 尚无旅程覆盖（test:map 如实报告）。

---
*执行：主线 #2 worker（GLM/claude-code）；派工与回贴动作均通过 GitHub Issue/Comment 留痕。*
