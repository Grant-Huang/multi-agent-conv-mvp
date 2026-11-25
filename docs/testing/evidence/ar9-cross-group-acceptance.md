# AR-9 跨组场景独立验收（主线 #2：S5 路由资料 + T-99 审计云 + nightly 形态首采）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「318 已合，继续」指示（2026-11-25）；GR-4 简报 §5 监视交接（11-25 起 nightly 结果归本线 AR 序列） |
| 验收人 | 本 worker（未参与 #319/#320/#323/#324/#326 实现） |
| 基线 | `origin/master` `25caa2b`（#319/#320/#323/#324/#326 全部合入） |
| 检出方式 | 隔离 worktree `/tmp/ar9-wt` @ `25caa2b`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | **11-25 nightly 已采（run 45691251266，20:07Z）：failure，2 红 = ADREAD（持续）+ UX05（新，本地绿→CI 抖动候选）；MOBILE/AGFIRST nightly 首次绿** |
| 证据等级 | 本地/PR CI/master push 为 T2；nightly/Issue 标签为 T2（run 与 Issue 页核对） |

## 0. 本轮范围（`75f55cb` → `25caa2b`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #326 | **convmem S5-route：路由路径会话资料线索**（ctx 追加极简清单，router 零改动） | 新套件 `route_materials`（RT-RMAT/JMAT） |
| #319 | T-99 链接审计上云（服务端权威审计） | 新套件 `desktop_link_audit`（RT-ADLAUD-01～04） |
| #320/#323/#324 | 宪章/治理/UXFIX + 体验地图扩展（UX04/UX05 新旅程） | experience.spec（5 旅程） |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| S5 路由资料（#326） | **VERIFIED / T2** | `route_materials` **3 passed**（RT-RMAT-01 路由请求含资料线索段+任务 id；JMAT-05 清单 ≤40 字截断；router 零改动口径经 diff 核查）——会话资料三路（回答 S1/判断 S4/路由 S5）**全部合同覆盖** |
| T-99 审计云（#319） | **VERIFIED / T2** | `desktop_link_audit` **4 passed**：服务端生成 audit.id、created_at 升序、task 过滤互不可见、**重启持久 + 全新库为空不播种**（H4 同族探针） |
| 体验地图扩展（#320/323/324） | **VERIFIED / T2** | experience **5/5 passed**（新增 UX04 任务详情不打断输入/草稿、UX05 提醒不抢焦点+稍后直达）；test:map：98 cases / **5 journeys** / 17 条欠账透明（CH-U6/CH-U7 未覆盖） |
| 回归 | **VERIFIED / T2** | desktop 32 + REV/MOBILE 2 全绿；合同合计 **46/46**（8 套件） |
| PR CI #319/#320/#323/#324/#326 | **VERIFIED** | 全部 8/8 pass |
| **master push @ `25caa2b`** | **VERIFIED** | run 45686145130 **success**——**rust 连续第五推绿** |
| **nightly 11-25（20:07Z）** | **FAILED（2 红）——形态质变** | ① **E2E-MOBILE/AGFIRST nightly 首次绿**（不在失败清单）② ADREAD 持续（11.7s）③ **UX05 新增红**（15.6s；本地同树 5.7s 通过 → CI 抖动候选）④ **派工机制首战成功**：Issue [#330](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/330) 自动创建，标签 `dispatch, line:3, nightly-e2e` 流向正确 |

**总体**：三条新链独立复验 **PASS**；nightly 从「五晚多因混战」收敛到「2 红 2 因」（ADREAD 持续 + UX05 抖动候选）；**派工标签机制实战通过**。

---

## 1. 本地证据（worktree @ `25caa2b`）

```text
$ cargo build --locked --bin agentnexus --bin an-mock-acp → Finished
$ cargo test --locked -p an-server --test route_materials --test desktop_link_audit \
    --test judge_materials --test session_materials --test introspection_materials \
    --test desktop_accept_payload --test desktop_read --test restart_semantics
route_materials: 3（RT-RMAT-01/JMAT-05 等）  desktop_link_audit: 4（RT-ADLAUD-01～04）
desktop_read: 6  desktop_accept_payload: 1  judge/session/introspection: 5/5/5
restart_semantics: 11
合计 46 passed / 0 failed

$ cd web && npm run build → 通过；npm run test:map → valid（98 cases / 5 journeys / 欠账 17 透明）
$ npx playwright test e2e/experience.spec.ts → 5 passed（UX01～UX05）
$ npx playwright test e2e/desktop.spec.ts    → 32 passed
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 路由请求含「会话资料线索」段与任务 id（出窗后仍相关） | 通过 | RT-RMAT-01（#326，router 零改动口径） |
| 超长清单行 ≤40 字截断+标记 | 通过 | RT-JMAT-05 |
| 链接审计服务端权威（服务端生成 id、升序、字段一致） | 通过 | RT-ADLAUD-01 |
| 审计 task 过滤互不可见 | 通过 | RT-ADLAUD-02 |
| 非法 action 400 | 通过 | RT-ADLAUD-03 |
| 审计重启持久 + 全新库空（不播种、不跨库） | 通过 | RT-ADLAUD-04（H4 同族探针） |
| UX04/UX05 新旅程（详情不打断输入、提醒不抢焦点+直达+返回仍在） | 通过 | experience 5/5 |
| Mock / fixture 可辨 | 通过 | 同既有口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#319 / #320 / #323 / #324 / #326：全部 **8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `25caa2b`（#326，run [45686145130](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45686145130)） | **success**——rust 连续第五推绿（467d6e0/6fbc83c/03373f2/75f55cb/25caa2b；Q-AR2-04 观察：抖动自 81445e7 后未再复现） |
| **nightly @ 11-25 20:07Z**（run [45691251266](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45691251266)） | **failure（2 红）**：E2E-ADREAD（11.7s，持续）+ E2E-UX05（15.6s，**本地同树通过 → CI 抖动候选**）；MOBILE/AGFIRST **nightly 首次绿**；**Issue #330 自动创建，标签 `dispatch, line:3, nightly-e2e` ——派工标签机制实战通过** |

## 4. 问题清单

### Q-AR9-01 — E2E-UX05 CI 抖动候选（中，体验地图作者线/分诊）

| 字段 | 内容 |
|---|---|
| 事实 | nightly 15.6s 失败；本线同树（25caa2b）本地 5.7s **通过**（AR-9 §1） |
| 建议 | 分诊确认归属（体验地图线）；若复现确认抖动，参照 Q-AR2-04 机制建议（复跑/稳定性加固） |
| 备注 | Issue #330 目前单 `line:3` 标签（ADREAD 主导）；UX05 归属请人工分诊补标 |

### Q-AR5-01（延续）— E2E-ADREAD

nightly 持续红（11.7s）。第 1 线桌面组修复中（双壳复现，#278 印证）。

### 延续台账

M1/M2（#298 审查 P2，修复未进场）；Q-AR2-04（rust 五连绿，观察转好）；FIX-A（用户决策中）；GR-4 评审 11-28（本卡 §3 即证词更新）。

## 5. 相对目标的差距（主轴对照——宪章 v1.4 口径）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **会话资料三路闭环**：回答 S1 / 判断 S4 / 路由 S5 全部合同覆盖且零 router 改动 | M1（门关闭泄漏）修复+补门关闭断言 |
| **审计上云**（T-99）：服务端权威、隔离、持久——「可追查」的操作面收口 | — |
| **体验地图 5 旅程** + nightly 形态质变（MOBILE/AGFIRST nightly 首绿） | UX05 抖动分诊；ADREAD 修复；11 条不变量欠账 |
| **派工标签机制实战通过**（#330 流向正确） | GR-4 评审 11-28（今晚 2 红形态入证词）；FIX-A 决策；远程真站点；真设备；M4 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未修 ADREAD/UX05/M1/M2（各归实现线）；未跑远程真站点
- nightly 失败 Issue 的分诊（UX05 补标）归值班线，本线只记录与建议
