# AI Desktop 特性清单 → Feature / Case 追溯（A0）

依据 [agentnexus_ai_desktop_design_checklist-2.md](agentnexus_ai_desktop_design_checklist-2.md) 与 [FEATURE_MAP.md](FEATURE_MAP.md)。  
**规则**：开发前有 Case；浏览器 Case ID 仅 `E2E-[A-Z]+`（check-map）；模拟与真实证据分列。  
状态：`草案` = 已映射未冻结断言；`已登记` = 已进 CASES/features.json；`Red/Green` = 有证据。

## 章节覆盖索引

| 清单章节 | Feature | Case（首批） | 覆盖层 | 状态 |
|---|---|---|---|---|
| 1 产品定位、2 S1/S2/S3 | AD-NAV、AD-LAYOUT、AD-S1 | E2E-ADNAV、E2E-ADLAYOUT、E2E-ADASK | browser / Mock | E2E-ADNAV/ADLAYOUT **已登记**（A1）；E2E-ADASK **已登记**（A5.1） |
| 3 数据、4 上下文 | AD-DRAFT、AD-SCOPE、AD-ACCEPT、AD-READ | E2E-ADDRAFT、E2E-ADSCOPE、E2E-ADACCEPT、E2E-ADREAD；RT-ADACCEPT-01～04；RT-ADLINK-01～04 | browser+HTTP | E2E-ADDRAFT/ADSCOPE **已登记**（A2）；E2E-ADREAD **已登记**（A2.1）；**E2E-ADACCEPT 真实 HTTP（T-61）**；**RT-ADACCEPT Green（B1）**；**RT-ADLINK Green（B3）** |
| 5 历史协作、6 布局 | AD-HISTORY、AD-SOURCES、AD-INPUT | E2E-ADHISTORY、E2E-ADSOURCES、E2E-ADINPUT | browser | E2E-ADHISTORY **已登记**（A3）；E2E-ADINPUT **已登记**（A4）；E2E-ADSOURCES **已登记**（C1 真实 HTTP） |
| 7–9 刷新/分工/风控 | AD-RESUME、AD-ATTENTION、AD-EVIDENCE、AD-APPROVE | E2E-ADRESUME、E2E-ADATTN、E2E-ADEVID、E2E-ADAPPR；RT-ADREPLAY-01～05 | browser+HTTP | E2E-ADEVID **已登记**（A3）；E2E-ADATTN **已登记**（A5.1）；E2E-ADAPPR **真实**（C2）；**E2E-ADRESUME 真实 SSE（T-64）**；**RT-ADREPLAY Green（B2）** |
| 10–13 阶段/开放/指南/参考 | 里程碑与决议 | — | docs | 见 PLAN / DECISIONS 61 |
| 14–15 助理/语音跨端 | AD-PROFILE、AD-VOICE、AD-HANDOFF | E2E-ADPROF、E2E-ADMEM、E2E-ADMEMW、E2E-ADXR、E2E-ADVOICE、E2E-ADHAND；RT-ADPREF-01～03；RT-ADMEM-01～06；RT-ADXR-01～04；RT-ADVOICE-01～04；RT-ADHAND-01～03 | browser+HTTP | **E2E-ADPROF（T-70）**；**E2E-ADMEM/ADMEMW + RT-ADMEM（T-71/T-72 夹具）**；**E2E-ADXR + RT-ADXR（T-73 已读跨端夹具）**；**RT-ADPREF Green**；**RT-ADVOICE/ADHAND（D fixture）**；真机媒体另卡 |
| 16–18 异步/主动/设置 | AD-S1、AD-ACCEPT、AD-ATTENTION、AD-PROFILE | E2E-ADASK、E2E-ADATTN、E2E-ADACCEPT | browser+HTTP | E2E-ADASK/ADATTN **已登记**（A5.1）；**E2E-ADACCEPT 真实 HTTP（T-61）** |
| 19–21 连续性/导航/回归 | AD-NAV、AD-LAYOUT、AD-DRAFT、AD-EVIDENCE、AD-INPUT | （同上） | browser | 随 A 增量；**A5 整体** `desktop.spec` 15/15（证据 `testing/evidence/ai-desktop-a5.md`） |

开放问题（清单 §11）决议前不写死 Case 断言；落地条款确认后补行。

## A5 出口核对（2026-11-20）

| Mock | Case（浏览器） | 证据 |
|---|---|---|
| M1 受理 | E2E-ADACCEPT；RT-ADACCEPT | a5.4 + a5 整体；**T-61** `ai-desktop-t61-accept.md` |
| M2 Run 恢复 | E2E-ADRESUME；RT-ADREPLAY-01～05 | a5.2 + a5 整体；**B2** `ai-desktop-b2.md` |
| M3 助理/设置 | E2E-ADPROF、E2E-ADINPUT | a4；a5 整体复跑 |
| M4 频道/权限 | E2E-OPS* / E2E-WA*（混合真实）；证据 ACL：RT-ADACL + E2E-ADSOURCES（C1） | 见 MOCK_REGISTRY M4 |
| M5 审批/提醒 | E2E-ADAPPR、E2E-ADATTN、E2E-ADASK | a5.1/a5.2 + a5 整体 |
| M6 语音/跨端 | E2E-ADVOICE、E2E-ADHAND；RT-ADVOICE/ADHAND | a5.3 + a5 整体；**D** `ai-desktop-d.md` |

## A1 必验条款（本里程碑）

| 原文锚点（章节） | 可观察断言 | Case |
|---|---|---|
| §2 / §6 默认进入桌面；三区 | 打开 `/` 见「我的 AI 桌面」与「我的工作」「Agent 工作」「为你关注」 | E2E-ADNAV |
| §2 / §20 SMT-03 多入口同 Task | 首页卡打开 `TASK-2026-018`；返回后仍在桌面；侧栏历史可再打开同一 ID | E2E-ADNAV |
| §2 S2↔S3 | 工作区可切换分屏/专注并恢复；刷新后布局偏好保留 | E2E-ADLAYOUT |
| §6 / 运营入口 | 「运营桌面」可进或明确「未启用」 | E2E-ADNAV |

完整条款级拆分在实现 A2+ 前按章节继续填本表（每条一行），不得只保留章节索引。
