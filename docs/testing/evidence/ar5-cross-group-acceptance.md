# AR-5 跨组场景独立验收（主线 #2：入口适配全量收敛——92/93）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 + AR-2/AR-3 收敛承诺（第 3 线适配合入后的全量收敛轮） |
| 验收人 | 本 worker（未参与 #291 实现） |
| 基线 | `origin/master` `6fbc83c`（#291 第 3 线适配已合入；含 #270 以来的全部入口迁移与适配） |
| 检出方式 | 隔离 worktree `/tmp/ar5b-wt` @ `6fbc83c`，`verification/run.mjs` 全新构建；安静窗口独占 18789/18787 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点 04:5xZ）；**预计今晚起 nightly 形态从「35 红」收敛到「≤1 红（E2E-ADREAD）」** |
| 证据等级 | 本地/PR CI 为 T2；results.json 机读核对；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围与定位

**收敛轮**：#291（第 3 线 8 套件 25 例适配，「断言零改动」）合入后，按 AR-2 指派流程执行的全量复跑。验证三件事：① 31→预期清零的适配欠账实际清到多少；② AGFIRST（第 1 线）终态；③ 剩余红的真实性质。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| **全量 verify 收敛** | **92 passed / 1 failed / 0 skipped**（93 例，1.9m） | results.json 机读核对；`stats: expected 92 / unexpected 1 / skipped 0` |
| 第 3 线适配（#291） | **VERIFIED / T2** | 25 例中 **24 例转绿**；「断言零改动」声明经 diff 核查成立（仅 2 处定位器随新壳收窄，断言保留：WABIND aria-pressed 移入导航作用域、OPSCHANNEL `toHaveText('1')` 原样） |
| **唯一剩红：E2E-ADREAD（第 3 线 ops）** | **FAILED（新定性）** | 「运营频道阅读锚点离开后恢复」——适配后仍红，失败点 `ops.spec.ts:274`（「我的桌面」点击后锚点恢复断言，2 分钟超时）；**性质为锚点恢复语义缺陷而非导航问题**（11-24 nightly 同例 2.0m 同位失败） |
| AGFIRST（Q-AR2-02） | **转绿 → 建议关闭** | `expected/passed`（机读）——第 1 线修复在新壳全量下生效（fa9970d 时的红为中间态时序，如实留痕于 AR-2/AR-3） |
| E2E-MOBILE（Q-T65-01） | 第四次确认绿 | `expected/passed` |
| 合同层 | **VERIFIED / T2** | judge_materials 5（#290 S4 新套件）+ session_materials 5 + introspection_materials 5 预验全绿；AR-4 已证 61/61 |
| PR CI #291 | **VERIFIED** | 8/8 pass |
| master push @ `6fbc83c` | in_progress → 终态回填 §3 | run 45548460843 |

**总体**：入口适配收敛 **29 红 → 1 红**（E2E-ADREAD，归第 3 线）；AGFIRST 转绿；MOBILE 稳定。**全量收敛达标（92/93），唯一遗留为单一用例级缺陷。**

---

## 1. 本地证据（worktree @ `6fbc83c`，run.mjs 全新构建）

```text
$ npm run verify
stats: expected 92 / unexpected 1 / skipped 0 / flaky 0（duration 1.9m）
唯一失败：ops.spec.ts:232 › E2E-ADREAD（锚点恢复；ops.spec.ts:274 超时）

关键用例（results.json 机读）：
E2E-AGFIRST → expected    E2E-MOBILE → expected
E2E-REV → expected        E2E-HOMEREV → expected

合同预验（本基线）：judge_materials 5（#290 S4）+ session_materials 5
+ introspection_materials 5 → 15 passed / 0 failed
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 第 3 线 25 例中 24 例适配后真实可用 | 通过 | 全量内对应用例 passed |
| 断言强度未放松（抽查 2 处定位器变更） | 通过 | diff 核查（§0/结论表） |
| AGFIRST 修复在最终基线生效 | 通过 | results.json expected |
| MOBILE 修复稳定 | 通过 | 连续第 4 次确认 |
| S4 判断路径资料接线 | 通过 | judge_materials 5（#290，预验） |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#291：**8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `6fbc83c`（#291，run [45548460843](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45548460843)） | 记录时 **in_progress**；终态回填：（待回填） |
| nightly 11-25 | 未触发（收稿时点）；预期收敛形态见摘要 |

## 4. 问题清单

### Q-AR5-01 — E2E-ADREAD 锚点恢复缺陷（中，第 3 线）

| 字段 | 内容 |
|---|---|
| 提交 | `6fbc83c`（适配已含本例）；11-24 nightly（run 45484039389）同例 2.0m 同位失败 |
| 现象 | 「运营频道阅读锚点离开后恢复」：锚点离开→回频道后的恢复断言超时（ops.spec.ts:274，「我的桌面」点击路径） |
| 定性 | 适配已入仍红 → **锚点恢复语义缺陷**（新壳的频道阅读状态在离开/返回后未按用例预期恢复），非导航问题 |
| 期望 | 阅读锚点离开后返回按 CASES 语义恢复 |
| 建议归属 | **第 3 线**（ops/运营频道）；修复后本线复跑该套件确认 |
| 备注 | 全量唯一剩红；清零后 G15/G6 具备「三晚连过」评估条件（另需 master rust 稳定性多推样本） |

### 收敛台账（更新）

| ID | 状态 |
|---|---|
| Q-AR2-01（适配欠账） | **基本清账**：31 → 1（ADREAD） |
| Q-AR2-02（AGFIRST） | **转绿 → 建议关闭**（第 1 线） |
| Q-AR3-01（第 3 线适配） | **完成**（25 例中 24 绿 + ADREAD 转缺陷项） |
| Q-AR2-04（rust 抖动） | cf8d793/fa9970d 红、467d6e0 绿——持续观察，多推样本后定论 |
| #290（S4）/#292（宪章 v1.4 DRAFT） | #290 已验（合同预验）；#292 宪章变更 DRAFT 归用户确认，本线不参与 |

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **桌面入口全形态收敛**：默认桌面/侧栏运营分区/设置-连接与能力/窄屏抽屉，浏览器 92/93 绿 | **E2E-ADREAD 锚点恢复缺陷**（第 3 线，唯一剩红） |
| 入口迁移以「断言零改动」完成（抽查核实） | nightly 需以新形态连过三晚（G15/G6 关闭条件；今晚起观察） |
| 会话资料两路（回答 S1 + 判断 S4）合同全绿 | master rust 稳定性多推样本（Q-AR2-04）；GR-4 例外评审 11-28 |
| AGFIRST/MOBILE/REV/HOMEREV 等历史缺陷全部转绿 | 远程真站点 live；真设备路径；M4 生产写；分层记忆 REQ-03/04 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替第 3 线修 ADREAD（用例级缺陷归实现线）
- #292 宪章 v1.4 为 DRAFT，确认流程归用户；本线未参与
- 远程真站点未跑
