# GR-4-NIGHTLY-E2E 到期评审简报（2026-11-28）

主线：**第 2 线 独立复跑与集成协调**（CI 与夜间 E2E 健康，2026-11-26 起从第 3 线移交，见[决定 2026-11-26-workstream-realignment](../decisions/2026-11-26-workstream-realignment.md)；本简报随移交由第 2 线维护，并补 11-25/11-26 两晚事实）。本文件是 [GUARDRAIL_REGISTRY.yaml](GUARDRAIL_REGISTRY.yaml) `GR-4-NIGHTLY-E2E` 例外条目到期（2026-11-28）的**评审准备材料**，只汇编可核对事实与选项，**不预作决定**；结论由用户在到期评审中作出。

## 1. 登记表现状（引用）

- id：`GR-4-NIGHTLY-E2E`（第四层）；目标「每晚在 master 上跑浏览器端到端」；enforcement：`.github/workflows/nightly-e2e.yml` 定时（UTC 10:30），不阻断。
- status：`pilot-warning`；exception：`持续失败未处理`（owner：用户；mitigation：「PR 级的 web 作业与本地 verify 仍在」；**expires_at：2026-11-28**；review_due：2026-11-28）。证据等级 VERIFIED。

## 2. 事实时间线（近五晚，全部 T2 可核对）

| 夜（UTC） | run | 结果 | 失败签名 |
|---|---|---|---|
| 11-20 | 44795474606 | 失败 | E2E-MOBILE 超时（自动开 issue #208） |
| 11-21 | 44931610796 | 失败 | 同上（Q-T65-01） |
| 11-22 | 45052408864 | 失败 | 同上 |
| 11-23 | 45277117267（22:10Z 触发） | 失败 | **TS6133 编译即死**（Q-T87-01；`verify` 在跑用例前失败）。已由 #259 修复，并把 e2e typecheck 纳入 PR 门（Q-T87-02 一并解决） |
| 11-24 | 45484039389（20:45Z 触发） | 失败 | verify **90 passed / 2 failed**：E2E-MOBILE（Q-T65-01 延续）+ **E2E-ADREAD**（`waitForResponse` 超时；与 [PR #278](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/278) 在 T-88 新壳标记的锚点回归候选相互印证——该例在旧壳同样失败，归属第 1 线桌面组复核） |
| 11-25 | 45691251266（20:03Z 触发） | 失败 | verify 2 failed：E2E-ADREAD + UX05（本地绿 → CI 抖动候选，Q-AR9-01）——第 5 晚连败 |
| 11-26 | 45885958121（19:45Z 触发，`664e330`） | **成功** | **修复后首个绿夜**：ADREAD（#349/#350）与 UXFIX 波修复兑现；三晚连绿窗口 **1/3**（口径见 §5） |

失败原因共三类：① Q-T65-01 窄屏发送（每轮复现，归属 Desktop/前端，第 9+ 轮未修）；② Q-T87-01 类型错误（**已修**）；③ E2E-ADREAD（新浮现，双壳复现，归属 Desktop/前端）。

**更正记录**：早前「11-23 全天未触发」的判断有误（[PR #240 评论](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/240#issuecomment-7144886592)）——该晚 22:10Z 触发，晚于当时监视截止 21:43Z。

## 3. 调度异常（独立于失败的第二个问题）

cron 仅 `30 10 * * *` 一档；实际触发常延迟数小时（11-20 18:43Z、11-21 17:43Z、11-22 17:58Z、11-23 22:10Z、11-24 20:45Z——~~GitHub 排队形态~~ **2026-11-27 更正：schedule 事件派发延迟，非 runner 排队**，证据与处置见 [NIGHTLY_DISPATCH_PLAN.md §0](NIGHTLY_DISPATCH_PLAN.md)：08:23Z 外部准点触发已上线、cron 兜底改 09:17Z、20h 重复运行守卫 [PR #471](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/471) 待批）。影响：①「每晚」的语义漂移；②升级链只覆盖失败、不覆盖**长期未触发**（11-23 若再晚 6 小时将跨日）；③三晚计数以「自然日晚」为单位在延迟形态下定义模糊。**此问题登记表例外条目未覆盖**，建议并入本次评审。

## 4. 到期评审的选项（供用户拍板，均需对应归属组）

| 选项 | 内容 | 代价 / 风险 |
|---|---|---|
| A 续期例外 | expires_at 顺延（建议 +2 周），mitigation 更新为「Q-T65-01/ADREAD 修复 + 调度异常并入」 | 维持现状；连续失败已 5 晚，无恶化即可续 |
| B 修调度 | cron 加第二档或 `workflow_dispatch` + 未触发 watchdog（L3，归 CI 组，先出方案）**（2026-11-27 更新：外部 `workflow_dispatch` 准点触发已上线；20h 守卫 + cron 非整点兜底在 [PR #471](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/471) 待批——B 主体已落地）** | 工程量小；解决「未触发无信号」缺口 |
| C 降级替代 | 以 PR 级 verify 替代夜间全量（#259 已把 typecheck 左移） | PR 门≠跨套件组合回归（G15 抓的正是端口/状态串扰类），**不建议** |
| D 关闭 GR-4 | 从登记表移除 | 失去唯一夜间全量层，**不建议** |

**助手建议**（待用户确认）：A + B 组合——续期例外两周，同时开 B 的调度修复卡；ADREAD 与 Q-T65-01 的修复进度作为下次续期的准出条件。

## 5. 监视交接

- **第 2 线已接管监视**（2026-11-26 起）：外部准点触发（08:23Z）+ cron 兜底（09:17Z，待 PR #471）+ 20h 守卫；失败自动开 issue 已标 `line:2`，由第 2 线分诊；结果记入 AR 验收序列。
- 三晚计数口径建议随评审一并明确：以「run 的 schedule 日期」还是「触发墙钟日」计（延迟形态下两者可不同日；准点触发落地后两口径应合一，11-26 起按触发墙钟日暂记）。

## 来源

GUARDRAIL_REGISTRY.yaml（GR-4 条目）；runs 44795474606 / 44931610796 / 45052408864 / 45277117267 / 45484039389；`docs/testing/evidence/t65|t66-cross-group-acceptance.md`；T-87/T-88/AR-1 记录（共享验收台账）；[PR #240 评论（更正与补记）](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/240#issuecomment-7144886592)；[PR #278（ADREAD 回归候选）](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/278)。
