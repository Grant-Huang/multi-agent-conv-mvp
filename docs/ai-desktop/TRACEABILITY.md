# AI Desktop 特性清单 → Feature / Case 追溯（A0）

依据 [agentnexus_ai_desktop_design_checklist-2.md](agentnexus_ai_desktop_design_checklist-2.md) 与 [FEATURE_MAP.md](FEATURE_MAP.md)。  
**规则**：开发前有 Case；浏览器 Case ID 仅 `E2E-[A-Z]+`（check-map）；模拟与真实证据分列。  
状态：`草案` = 已映射未冻结断言；`已登记` = 已进 CASES/features.json；`Red/Green` = 有证据。

## 章节覆盖索引

| 清单章节 | Feature | Case（首批） | 覆盖层 | 状态 |
|---|---|---|---|---|
| 1 产品定位、2 S1/S2/S3 | AD-NAV、AD-LAYOUT、AD-S1 | E2E-ADNAV、E2E-ADLAYOUT | browser / Mock | E2E-ADNAV/ADLAYOUT **已登记**（A1） |
| 3 数据、4 上下文 | AD-DRAFT、AD-SCOPE、AD-ACCEPT | E2E-ADDRAFT、E2E-ADSCOPE、E2E-ADACCEPT | browser+HTTP | E2E-ADDRAFT/ADSCOPE **已登记**（A2）；ACCEPT 待 B1 |
| 5 历史协作、6 布局 | AD-HISTORY、AD-SOURCES、AD-INPUT | E2E-ADHISTORY、E2E-ADSOURCES、E2E-ADINPUT | browser | E2E-ADHISTORY **已登记**（A3）；E2E-ADINPUT **已登记**（A4）；SOURCES 待 C |
| 7–9 刷新/分工/风控 | AD-RESUME、AD-ATTENTION、AD-EVIDENCE、AD-APPROVE | E2E-ADRESUME、E2E-ADATTN、E2E-ADEVID、E2E-ADAPPR | browser+RT | E2E-ADEVID **已登记**（A3）；其余待 A5/C |
| 10–13 阶段/开放/指南/参考 | 里程碑与决议 | — | docs | 见 PLAN / DECISIONS 61 |
| 14–15 助理/语音跨端 | AD-PROFILE、AD-VOICE、AD-HANDOFF | E2E-ADPROF、E2E-ADVOICE、E2E-ADHAND | browser / Mock→D | E2E-ADPROF **已登记**（A4）；VOICE/HANDOFF 待 A5/D |
| 16–18 异步/主动/设置 | AD-S1、AD-ACCEPT、AD-ATTENTION、AD-PROFILE | （同上） | browser | 草案 |
| 19–21 连续性/导航/回归 | AD-NAV、AD-LAYOUT、AD-DRAFT、AD-EVIDENCE、AD-INPUT | （同上） | browser | 随 A 增量 |

开放问题（清单 §11）决议前不写死 Case 断言；落地条款确认后补行。

## A1 必验条款（本里程碑）

| 原文锚点（章节） | 可观察断言 | Case |
|---|---|---|
| §2 / §6 默认进入桌面；三区 | 打开 `/` 见「我的 AI 桌面」与「我的工作」「Agent 工作」「为你关注」 | E2E-ADNAV |
| §2 / §20 SMT-03 多入口同 Task | 首页卡打开 `TASK-2026-018`；返回后仍在桌面；侧栏历史可再打开同一 ID | E2E-ADNAV |
| §2 S2↔S3 | 工作区可切换分屏/专注并恢复；刷新后布局偏好保留 | E2E-ADLAYOUT |
| §6 / 运营入口 | 「运营桌面」可进或明确「未启用」 | E2E-ADNAV |

完整条款级拆分在实现 A2+ 前按章节继续填本表（每条一行），不得只保留章节索引。
