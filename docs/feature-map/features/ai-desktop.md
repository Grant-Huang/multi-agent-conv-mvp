# ai-desktop：组织级 AI 工作桌面（V1.7）

规格：[docs/ai-desktop/DESIGN.md](../../ai-desktop/DESIGN.md)；计划与 Mock：[PLAN.md](../../ai-desktop/PLAN.md)、[MOCK_REGISTRY.md](../../ai-desktop/MOCK_REGISTRY.md)；追溯：[TRACEABILITY.md](../../ai-desktop/TRACEABILITY.md)。

## Sub-features（本阶段）

| 子功能 | 说明 |
|---|---|
| 桌面首页 | 默认入口；「我的工作 / Agent 工作 / 为你关注」；Mock 卡片含 SMT-03 |
| 工作区 S2/S3 | 同一 Task 分屏与专注；布局偏好按设备本地保存 |
| 模块入口 | 会话（真实 API）、运营桌面（有开关则真，否则明确未启用） |

## How to get to it

打开主站 `/` → 默认「我的 AI 桌面」。点 SMT-03 卡进入工作区；侧栏「＋ 新会话」进入会话；「运营桌面」进入运营模块或未启用说明。

## Driving it with Playwright

```ts
await page.goto('/');
await expect(page.getByTestId('desktop-home')).toBeVisible();
await page.getByTestId('card-smt-03').click();
await expect(page.getByTestId('workspace')).toContainText('TASK-2026-018');
```

用例：`E2E-ADNAV`、`E2E-ADLAYOUT`（A1）；`E2E-ADDRAFT`、`E2E-ADSCOPE`（A2）；`E2E-ADREAD`（A2.1，含运营频道）；`E2E-ADEVID`、`E2E-ADHISTORY`（A3）；`E2E-ADPROF`（A4→**T-70 真 B3**）、`E2E-ADMEM`（**T-71 记忆列表**）、`E2E-ADMEMW`（**T-72 纠错/撤回**）、`E2E-ADXR`（**T-73 已读跨端**）、`E2E-ADINPUT`（A4）；`E2E-ADASK`、`E2E-ADATTN`（A5.1）；`E2E-ADAPPR`、`E2E-ADRESUME`（A5.2）；`E2E-ADVOICE`、`E2E-ADHAND`（A5.3→**D fixture**）；`E2E-ADACCEPT`（A5.4）；`E2E-ADSOURCES`（C1 真实 ACL）。**A5 整体收口**（2026-11-20）：证据 `docs/testing/evidence/ai-desktop-a5.md`。**C1**（2026-11-20）：RT-ADACL-01～05 + E2E-ADSOURCES，证据 `docs/testing/evidence/ai-desktop-c1.md`。**D**（2026-11-21）：RT-ADVOICE/ADHAND + E2E 真实 HTTP，证据 `docs/testing/evidence/ai-desktop-d.md`。**T-70**（2026-11-22）：profileStore→B3 preferences，证据 `docs/testing/evidence/ai-desktop-t70-preferences.md`。**T-71**（2026-11-22）：记忆列表→`GET /memories`，证据 `docs/testing/evidence/ai-desktop-t71-memory-list.md`。**T-72**（2026-11-22）：纠错/撤回写路径，证据 `docs/testing/evidence/ai-desktop-t72-memory-write.md`。**T-73**（2026-11-22）：已读跨端夹具，证据 `docs/testing/evidence/ai-desktop-t73-read-cross.md`。**T-81 / WP7**（2026-11-23）：E2E-RESTART（GS6 重启分支：执行中重启→FAILED+失败交付；待验收重启→判定通过）+ RT-RESTART-01/02/06，证据随 T-81 PR。**T-82**（2026-11-23）：首页三区卡片接真实快照 `GET /api/desktop/v1/snapshot`（RT-ADSNAP-01～04），证据 `docs/testing/evidence/ai-desktop-t82-home-snapshot.md`。**T-86**（2026-11-23）：工作区草稿接服务端权威 `GET/PUT/DELETE /api/desktop/v1/drafts/{kind}/{id}`（RT-ADDRAFTS-01～04），证据 `docs/testing/evidence/ai-desktop-t86-drafts-cloud.md`。**T-85 / WP7 方案 C**（2026-11-23）：RT-RESTART-07～11（QUEUED/CREATED 单任务重启重排；RUNNING/VERIFYING 仍失败；`resume_queued_on_restart` 开关回滚；模板家族不重驱），证据随 T-85 PR。**T-88**（2026-11-23）：首页与旧壳整治（A1 欠账；E2E-ADHOME；desktop.spec 32/32）。**T-96**（2026-11-24）：助理消息已读持久化（`desktop_assistant_read_ids`；RT-ADXR-05/06），证据 `docs/testing/evidence/ai-desktop-t96-assistant-reads-persist.md`。其余 AD-* / 真实替换见 FEATURE_MAP 与 MOCK_REGISTRY。

## B1 完整载荷幂等（R-20261121-M5）

RT-ADACCEPT-05～08：同键改变会话、scope、task_id 或正文返回冲突；相同载荷只重放库中记录，并发和同库重启不重复插入消息。授权与取舍见[决定](../../decisions/2026-11-24-desktop-accept-payload.md)，实测状态见[证据](../../testing/evidence/r-20261121-m5.md)。这不表示 M2 分流恢复或 M6 事件恢复已完成。