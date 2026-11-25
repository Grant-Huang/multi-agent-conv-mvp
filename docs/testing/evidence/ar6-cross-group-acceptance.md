# AR-6 跨组场景独立验收（主线 #2：宪章 v1.4 首个验收对象——体验地图 UX-01）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「已合，你继续」指示（2026-11-25）；宪章 v1.4 反目标「逐步写成检查」落地的验证职责（本线宪章透镜） |
| 验收人 | 本 worker（未参与 #294 实现） |
| 基线 | `origin/master` `81445e7`（#294 体验地图试点 UX-01 已合入） |
| 检出方式 | 隔离 worktree `/tmp/ar6-wt` @ `81445e7`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点 05:4xZ）；AR-5 已证浏览器收敛形态 92/93（唯一红 ADREAD） |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮定位

宪章 v1.4 新增「体验反目标/体验原则」（CH-N/CH-U 系列，约束类「逐步写成检查」）。#294 为该机制首个试点：UX-01 旅程（P-SUP-NEW 角色发现桌面→委托→证据→专注→往返）+ 校验脚本 `check-experience.mjs`（接入 `test:map`）+ E2E-UX01 基线用例 + 探索式审查发现 INTENT-2026-001～008。本卡验证**该机制首个落地物的真实性与有效性**——宪章透镜（本线新职责）的首次行使。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| 校验脚本 `check-experience.mjs`（#294） | **VERIFIED / T2** | `npm run test:map` 实测：`Feature map valid: 33 features, 94 browser cases; Experience map valid: 1 journeys, 3 invariant(s) without case (debt); not yet covered: CH-U4, CH-U6`——**欠账与未覆盖条款如实报告**（宪章透明度要求的正面样本） |
| E2E-UX01 基线用例 | **VERIFIED / T2** | 1 passed：发现 SMT-03 → 工作区 → 证据 → 专注 → 离开再返回；对话可达、草稿与位置恢复、无整页跳转（探针记录步数） |
| 回归 | **VERIFIED / T2** | desktop 32 + REV/MOBILE 2 全绿（MOBILE 第五次确认） |
| PR CI #294 | **VERIFIED** | 8/8 pass |
| master push @ `81445e7` | in_progress → 终态回填 §3 | run 45554642160 |
| ADREAD（Q-AR5-01） | **仍未修** | 本轮无 ops 套件变化；ADREAD 修复合入后跑 AR-6 第二段（ops + 全量抽查 + 归属并案定论） |

**总体**：宪章 v1.4 验证机制首个落地物独立复验 **PASS**；体验地图以「欠账透明」方式起步（3 条不变量无用例、CH-U4/CH-U6 未覆盖——已被校验脚本如实暴露，符合「先写欠账再补」的预期形态）。

---

## 1. 本地证据（worktree @ `81445e7`）

```text
$ npm run build → 通过
$ npm run test:map
Feature map valid: 33 features, 94 browser cases
Experience map valid: 1 journeys, 3 invariant(s) without case (debt); not yet covered: CH-U4, CH-U6
$ npx playwright test e2e/experience.spec.ts → 1 passed（E2E-UX01）
$ npx playwright test e2e/desktop.spec.ts    → 32 passed
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
```

**过程勘误（本线自纠，留痕）**：本卡首跑 Playwright 因 worktree 未建后端二进制（harness 启动即退，Exit 1）——本线补 `cargo build --bin agentnexus --bin an-mock-acp` 后复跑。与知会硬注意 #1 同源（E2E 前置完整构建：dist **与** 后端二进制），建议并入坑表。

### 核对清单

| 检查点 | 结果 | 依据 |
|---|---|---|
| 体验地图校验接入 CI 门（test:map） | 通过 | run.mjs/test:map 实测输出 |
| 约束须有用例或标欠账（校验规则生效） | 通过 | 输出如实列出 3 条无用例不变量 |
| UX-01 旅程连续性（对话/草稿/证据/专注往返） | 通过 | E2E-UX01 |
| 无整页跳转（CH-N2 连续工作区反目标的可检验化） | 通过 | UX01 探针断言 |
| Mock / fixture 可辨 | 通过 | 同既有口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#294：**8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `81445e7`（#294，run [45554642160](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45554642160)） | 记录时 **in_progress**；终态回填：（待回填） |
| nightly 11-25 | 未触发（收稿时点）；AR-5 已证收敛形态，本卡不变 |

## 4. 问题清单

### 延续（无新增业务缺陷）

| ID | 状态 |
|---|---|
| Q-AR5-01 / ADREAD | 仍未修（AR-6 第二段前置） |
| Q-AR3-01 / 第 3 线适配 | 已完成（AR-5 确认 24/25 绿） |
| Q-AR2-04 / rust 抖动 | 观察续记（467d6e0、6fbc83c 两连绿） |
| 体验地图欠账 | 3 条不变量无用例、CH-U4/CH-U6 未覆盖——**校验脚本已如实暴露**，按试点机制补用例即可，不立新编号 |

## 5. 相对目标的差距（主轴对照——宪章 v1.4 口径首报）

主轴 = **面向组织的 AI 工作桌面**（制造业为差异化方向）上的「观察 → 提方案 → 人批准 → 执行 → 核验」受控闭环。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **宪章→检查的机制首通**：反目标条款可校验、欠账透明（CH-N2 连续工作区已有 E2E 探针） | CH-U4/CH-U6 未覆盖、3 条不变量欠账（试点机制内补齐） |
| UX-01 旅程连续性（发现→委托→证据→专注→往返）浏览器实证 | 旅程仅 1 条（试点）；其余旅程按机制扩展 |
| 桌面主链 92/93 收敛维持（desktop 32 + REV/MOBILE 复绿） | **ADREAD**（第 3/1 线归属并案）；master rust 稳定性多推样本 |
| — | GR-4 例外评审（11-28）；远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替第 3 线修 ADREAD；未跑远程真站点
- 体验地图用例补齐归试点机制（第 1 线/映射作者），本线只验机制与欠账透明度
