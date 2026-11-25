# AR-8 跨组场景独立验收（主线 #2：UX-03 旅程 + 派工机制升级）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「检查pr状态，继续下一步」+「#297 已合，你看下一步如何继续」指示（2026-11-25） |
| 验收人 | 本 worker（未参与 #300/#311 实现） |
| 基线 | `origin/master` `75f55cb`（#300 UX-03、#311 派工机制） |
| 检出方式 | 隔离 worktree `/tmp/ar8-wt` @ `75f55cb`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点 17:4xZ，预计 ~20:45Z）；**#311 起失败 Issue 自动打 `dispatch + line:N` 派工标签**——ADREAD 若今晚红，标签流向即为派工机制首次实战 |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`03373f2` → `75f55cb`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #300 | **UX-03 一句话委派后台运行**（旅程/E2E-UX03/基线审查 INTENT-014～016；审查发现直接派研发线） | experience.spec（3 旅程） |
| #311 | **派工机制升级**：研发线工作经 GitHub Issue（`dispatch + line:N`）派发；nightly 失败 Issue 自动打派工标签（CI 变更） | workflows diff 核查 + 今晚 nightly 实战观察 |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| UX-03 旅程（#300） | **VERIFIED / T2** | experience.spec **3/3 passed**（UX03：一句话委派后**留在桌面**、后台运行可做他事、完成后「我的工作」可见——CH-N2/N3 第三次实证）；test:map valid：96 browser cases / 3 journeys / 11 条不变量欠账如实报告 |
| 派工机制（#311） | **VERIFIED / T2（静态面）** | workflows diff 核查：nightly 失败 Issue 自动加 `dispatch + line:N`；**实战首采=今晚 ADREAD（若红）**，标签流向记录入 AR-9 |
| 回归 | **VERIFIED / T2** | desktop 32 + REV/MOBILE 2 全绿 |
| PR CI #300/#311 | **VERIFIED** | 各 8/8 pass |
| master push @ `75f55cb` | in_progress → 终态回填 §3 | run 45666350348 |

**总体**：UX-03 与派工机制独立复验 **PASS**；ADREAD 仍未修（不变）。

---

## 1. 本地证据（worktree @ `75f55cb`）

```text
$ cargo build --locked --bin agentnexus --bin an-mock-acp → Finished
$ cd web && npm run build → 通过
$ npm run test:map
Feature map valid: 33 features, 96 browser cases
Experience map valid: 3 journeys, 11 invariant(s) without case (debt); not yet covered: CH-U4, CH-U6
$ npx playwright test e2e/experience.spec.ts → 3 passed（UX01/UX02/UX03）
$ npx playwright test e2e/desktop.spec.ts    → 32 passed
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
```

## 2. PR CI 证据

#300 / #311：各 **8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `75f55cb`（#311，run [45666350348](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45666350348)） | 记录时 **in_progress**；终态回填：（待回填） |
| nightly 11-25 | 未触发（收稿时点）；**派工标签首采窗口**：若失败，核对 Issue 标签 `dispatch + line:N` 是否按分诊规则流向 |

## 4. 问题清单

### 延续（无新增业务缺陷）

| ID | 状态 |
|---|---|
| Q-AR5-01 / ADREAD | 仍未修（**今晚 nightly 红时的派工标签为机制首次实战**） |
| Q-AR2-04 / rust 稳定性 | 03373f2 绿续记，多推样本 |
| M1/M2（#298 审查） | open，修复未进场 |
| UXFIX 卡（#295 派卡）+ INTENT-014～016 | 实现落地后进验收队列 |

## 5. 相对目标的差距（主轴对照——宪章 v1.4 口径）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **一句话委派后台运行**（UX-03）：委派不锁人、完成可见——CH-N2/N3 第三旅程实证，桌面「委派后自由」体验成形 | 体验地图不变量欠账 11 条（随旅程透明增长，按机制补齐） |
| **派工机制制度化**：跨线工作/nightly 失败经 Issue+标签派发（替代临时评论协调） | ADREAD 修复（首个实战标签样本）；M1/M2；FIX-A 决策 |
| 桌面主链收敛维持（desktop 32 + 体验 3 + REV/MOBILE 全绿） | master rust 稳定性多推样本；GR-4 例外评审（11-28，证词已备） |
| — | 远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未修 ADREAD/M1/M2；未跑远程真站点
- 派工机制的实战有效性以今晚 nightly 标签流向为准（AR-9 记录）
