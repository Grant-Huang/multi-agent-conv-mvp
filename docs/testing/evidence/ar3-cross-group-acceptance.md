# AR-3 跨组场景独立验收（主线 #2：适配阶段收敛——第 1/4/5 线面确认）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「已合并。你按计划继续」指示（2026-11-24）；AR-2 遗留的收敛承诺（适配面逐线确认） |
| 验收人 | 本 worker（未参与 #276～#279 实现） |
| 基线 | `origin/master` `e28a5a5`（含 #276 审查修复方案 docs、#277 第 4 线 T-91（FEEDBACK 适配收尾）、#279 第 5 线 intro 适配、#280 本线 AR-2） |
| 检出方式 | 隔离 worktree `/tmp/ar3-wt` @ `e28a5a5`，独立 CARGO_TARGET_DIR；安静窗口（18789 空闲） |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点）；当前 master 浏览器形态 = **35 红**（第 3 线 25 + AGFIRST 1 + 级联 9），nightly 将持续红直至清账 |
| 证据等级 | 本地/PR CI 为 T2 |

## 0. 本轮定位

AR-2 揭示的适配欠账（29 红）在**第 3 线（25 例）尚未适配**时无法做全量收敛；本卡为**阶段收敛轮**：确认已合入的第 1/4/5 线适配面真实有效、复核 AGFIRST 与 rust 抖动现状，为 AR-4（第 3 线适配后的全量收敛轮）划定基线。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| 第 5 线 intro 适配（#279） | **VERIFIED / T2** | intro.spec **4 passed**（含适配的 INTROVIEW/INTROLABEL 经空会话进内省页签，断言不变） |
| 第 4 线 FEEDBACK 还账（#277） | **VERIFIED / T2** | codeflow.spec **2 passed**（含 FEEDBACK 新壳右栏路径）+ restart 1 passed |
| AGFIRST（Q-AR2-02） | **仍 FAILED（二次确认，第 1 线）** | agents.spec 1 failed + **10 did not run**（串行级联）；自 AR-2 以来无修复合入 |
| rust 抖动观察（Q-AR2-04） | 本地 3/3 绿 | `mcp.rs expired_http_session` 连跑 3 次通过——仍为 CI 专属抖动，第 4 线/工程处置中 |
| 合同抽查 | **VERIFIED / T2** | restart_semantics + desktop_c2 + introspection_materials = 23 passed |
| 桌面主链（desktop.spec） | **VERIFIED / T2** | 31 passed（AR-2 全量内已证，本卡未重跑全量） |

**总体**：已合入的适配面（第 4/5 线）独立确认 **有效**；收敛欠账=第 3 线 25 例 + AGFIRST（第 1 线）。

---

## 1. 本地证据（worktree @ `e28a5a5`）

```text
$ cd web && npm run build → 通过（全新 dist）
$ npx playwright test e2e/intro.spec.ts     → 4 passed (7.4s)   ← #279 适配面
$ npx playwright test e2e/codeflow.spec.ts  → 2 passed          ← FEEDBACK 欠账清零（#277）
$ npx playwright test e2e/restart.spec.ts   → 1 passed
$ npx playwright test e2e/agents.spec.ts    → 1 failed（AGFIRST）+ 10 did not run（串行级联）
$ for i in 1 2 3; do cargo test -p an-agents --test mcp expired_http_session; done → 3/3 passed（CI 抖动未本地复现）
$ cargo test -p an-server --test restart_semantics --test desktop_c2 --test introspection_materials
→ 23 passed / 0 failed
```

## 2. PR CI 证据

#276 / #277 / #279：全 pass（gh pr checks 逐个核对）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| master push @ `e28a5a5`（#280） | 收稿时未见终态 run（同推多合并取消形态）；以 CI 页面为准 |
| nightly 11-25 | 未触发（收稿时点）；预计在 35 红形态下运行 |

## 4. 问题清单

### Q-AR3-01 — 第 3 线 25 例适配仍未合入（高，收敛阻塞）

AR-2 提醒已发（#270 逐线清单）；截至本卡基线无第 3 线适配 PR。25 例清单见 AR-2 §1.1 / #270 提醒评论。**AR-4（全量收敛轮）前置条件**。

### Q-AR2-02（延续，二次确认）— AGFIRST 未修

`e28a5a5` 仍红 + 级联 10 未跑。第 1 线。

### Q-AR2-04（延续）— rust CI 抖动

本地观察仍 0 复现；等待第 4 线/工程的机制或用例修复。

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 适配欠账从 31 → 剩 26（第 4 线 3 例 + 第 5 线 2 例已清并实测） | **第 3 线 25 例**（AR-4 前置）+ AGFIRST（第 1 线） |
| FEEDBACK 欠账清零（#277） | FEEDBACK 之外无新增欠账 |
| 桌面主链与记忆/内省/重启合同面持续全绿 | master rust CI 稳定性（Q-AR2-04）；远程真站点；真设备；M4 |

## 6. 非目标（本卡遵守）

- 未替第 3 线适配（各线自适配约定）；未改 crates/web 业务、断言、登记表、PLAN
- 未跑全量 verify（第 3 线未适配，全绿不可能——避免中间态噪音，收敛轮留在 AR-4）
