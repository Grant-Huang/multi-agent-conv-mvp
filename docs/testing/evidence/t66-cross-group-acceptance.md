# T-66 跨组场景独立验收（主线调整后：生产只读 + 个人记忆）

| 项目 | 内容 |
|---|---|
| 任务 | T-66 跨组场景独立验收（任务卡未给 ID，此为建议值，**待用户确认**） |
| 授权来源 | 用户对话任务卡（独立验收角色）；本轮范围按「主线计划有调整，对齐产品规划、设计和计划」指示，对新合入主线的内容复验 |
| 验收人 | 本 worker（未参与 #213～#220 实现） |
| 基线 | `origin/master` `47d8557`（含 #203～#220） |
| 检出方式 | **隔离 worktree `/tmp/t66-wt` @ `47d8557`**（主工作树有并行会话在途改动：T-73 read-cross 的 PLAN/CASES/features.json 未提交修改，本卡未触碰、未覆盖） |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；生产世界 = **stub 服务层**（测试控件默认关），远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 最新仍为 11-22 失败（run 45052408864，Q-T65-01）；11-23 定时未触发。状态**不改** |
| 证据等级 | 本地/PR CI 所列为 T2；生产只读浏览器 Red = 计划内（M3-4 未实现），见 §1.4 |

## 0. 主线对齐（本轮范围依据）

- **PLAN**（master）：M0/M1/M2 已准出；**M3-0/1/2 已勾**；M3-3（#220）已合入但 PLAN 未勾（作者证据 `wa-m3-3-green.md` 已存在，PLAN 勾选待同步——随 Q-T66-01 一并提示）；AI 桌面新增 T-70（preferences 接 B3）、T-71（记忆列表）、T-72（记忆纠错/撤回）已勾。
- **DESIGN 0.3**：`个人记忆、画像与 taste` 行仍为「设计中，未开工」→ **与实现不同步**（Q-T66-01）。
- **CASES**（冻结）：记忆 RT-ADMEM-01～06 + E2E-ADMEM/ADMEMW；生产只读 RT-WAPROD-01～06 + E2E-WAPROD/WAPISO/WAPREAD/WAPDELAY（浏览器 Green 属 M3-4）；WP2 E2E-REV（实现已 Green）。
- 本轮 = 独立复验以上新链 + 既有链回归 + 三层证据。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 记忆合同 RT-ADMEM-01～06 + E2E-ADMEM/ADMEMW | **VERIFIED / T2** | 合同 6 passed；浏览器 2 passed（独立复验） |
| Preferences T-70：E2E-ADPROF | **VERIFIED / T2** | 浏览器 1 passed |
| 生产只读合同 RT-WAPROD-01～06 | **VERIFIED / T2** | `mom_world_prod` 6 passed（01/02 + M3-3 守卫 03～06） |
| 生产只读浏览器 E2E-WAPROD×4 | **RED（计划内）** | 4 failed；UI `data-testid=desktop-home` 未接线（M3-4 范围）；合同层已绿，作者口径一致 |
| WP2 人工判定：E2E-REV | **VERIFIED / T2** | needs_human → 判定通过 → COMPLETED，浏览器 1 passed |
| 既有链回归（C2/Accept/Replay/WA 写） | **VERIFIED / T2** | 合同 28 passed；桌面 E2E 4；WA 写 E2E 4；`wa-m2-regression.sh` green |
| E2E-MOBILE | **仍 FAILED** | `47d8557` 上 1 failed → Q-T65-01 未修复，挂账 |
| PR CI #213～#220 | **VERIFIED** | 全 pass（抽查 #218/219/220 各 8 项 pass，其余无 fail） |
| master push | **VERIFIED** | #218/#219/#220（`47d8557`）全部 success（#220 终态见 §3） |
| 远程真站点 live | **NOT_VERIFIED** | M3 为 stub 服务层；真站点凭据/egress 本卡禁止且未配 |

**总体**：主线调整后的新链（记忆、生产只读合同与守卫、人工判定）独立复验 **PASS**；生产只读浏览器入口为**计划内 Red**（M3-4）；E2E-MOBILE 缺陷延续。

---

## 1. 本地证据（worktree @ `47d8557`，全新 target 构建）

### 1.1 合同层（6 套件 40/40）

```text
$ cargo test --locked -p an-server --test desktop_mem --test mom_world_prod \
    --test desktop_c2 --test desktop_accept --test desktop_replay --test mom_world_des_write
desktop_accept:      4 passed   desktop_c2:        7 passed
desktop_mem:         6 passed   desktop_replay:    5 passed
mom_world_des_write: 12 passed  mom_world_prod:    6 passed
合计 40 passed / 0 failed
```

新链覆盖：RT-ADMEM-01～06（列表/详情/404/PATCH 更新/DELETE 幂等/404）；RT-WAPROD-01（Bind 需登记权威源）/02（Observe/Subscribe 水位）/03（跨 scope 拒绝、无跨站泄露）/04（游标过期→恢复续读无漏无重）/05（延迟源 `as_of` 对齐）/06（生产写明确拒绝、Mock Propose 仍通）。

### 1.2 浏览器 E2E（正常桌面入口）

```text
$ npx playwright test e2e/desktop.spec.ts -g 'ADMEM|ADMEMW|ADPROF'
✓ E2E-ADPROF / E2E-ADMEM / E2E-ADMEMW        3 passed (6.2s)
$ npx playwright test e2e/features.spec.ts -g 'REV'
✓ E2E-REV: 审查待验收后判定通过              1 passed
$ npx playwright test e2e/desktop.spec.ts -g 'ADRESUME|ADAPPR|ADATTN|ADACCEPT'
4 passed (3.0s)
$ npx playwright test e2e/wa-des-write.spec.ts
4 passed (1.8s)
```

### 1.3 WA M2 回归脚本

```text
$ ./scripts/wa-m2-regression.sh
write 12 + read 12 + RT-DES 4 + RT-DESC 6 → wa-m2 regression green
```

### 1.4 生产只读浏览器 Red（计划内，非缺陷）

```text
$ npx playwright test e2e/wa-prod-read.spec.ts
✘ E2E-WAPROD / E2E-WAPISO / E2E-WAPREAD / E2E-WAPDELAY   4 failed (30s timeout)
```

- 失败点：HTTP bind 步骤（合同已绿）之后的页面断言 `page.getByTestId('desktop-home')` 超时；`web/src` 中**只有 `.desktop-home` CSS 类，无 `data-testid` 属性** → 生产世界入口未在桌面 UI 接线。
- 判定：与作者口径「浏览器 Green 属 M3-4」一致，属**计划内未实现**，不改断言、不记缺陷；M3-4 完成后需复跑转 Green。
- CASES 要求（`provenance=production` 可辨、跨站不泄露、续读去重、`as_of` 可辨）的**合同级等价断言**本轮已验（§1.1）。

### 1.5 E2E-MOBILE（Q-T65-01 跟踪）

```text
$ npx playwright test e2e/features.spec.ts -g 'MOBILE'   → 1 failed
```

`47d8557`（#213～#220 合入后）仍未修复；无相关修复提交。维持 Q-T65-01 归属与建议（Desktop/前端 + G15 根因合并处理）。

### 1.6 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 记忆写路径幂等与可辨 | 通过 | RT-ADMEM-05（DELETE 幂等）、夹具 `fixture/reset`（T-72 登记 Mock） |
| 记忆读权威与 404 语义 | 通过 | RT-ADMEM-01～03/06 |
| 生产 Bind 绑定权威源与协议版本 | 通过 | RT-WAPROD-01（`world_id`/`provenance=production`/`protocol_version`） |
| 观测回执含来源与水位 | 通过 | RT-WAPROD-02（`source`/`as_of`/`protocol_version`） |
| 权限拒绝不泄露 | 通过 | RT-WAPROD-03（ACCESS_DENIED，无跨站对象） |
| 断点续读无漏无重（event_id） | 通过 | RT-WAPROD-04（CURSOR_EXPIRED→恢复） |
| 延迟源 `as_of` 与水位对齐 | 通过 | RT-WAPROD-05 |
| 生产写关闭（M4 前不开放） | 通过 | RT-WAPROD-06（WRITE_DISABLED；Mock 通路不受影响） |
| 审查待人工独立判定 | 通过 | E2E-REV（needs_human + result → 判定 → COMPLETED） |
| Mock / fixture 可辨 | 通过 | 记忆=进程内夹具；生产=服务层 stub，`with_test_controls` 默认关（M3-3 登记）；`world_mock` 门控挂载；远程真站不宣称 |
| 远程真站点 | **NOT_VERIFIED** | 无凭据/环境 |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #213 | M3-1 正式 CASES + Red 空壳 | pass |
| #214/#215 | （依赖升级/小项） | pass |
| #216 | WP4 任务拆分模板方案（docs） | pass |
| #217 | T-71 记忆列表 | pass |
| #218 | M3-2 生产只读 Bind/Observe Green | pass（8/8） |
| #219 | T-72 记忆纠错/撤回 | pass（8/8） |
| #220 | M3-3 生产守卫 | pass（8/8） |

---

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ #218 合并 | **success**（run 45124934860） |
| push @ #219 合并 | **success**（run 45127072004） |
| push @ `47d8557`（#220） | **success**（run [45127464681](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45127464681)；rust 等全部作业通过，复跑确认于同会话收尾时） |
| nightly（11-23 UTC 17:43 前） | 未触发；最新仍为 11-22 失败（G15 状态不变，条件「三晚连过」未满足） |

---

## 4. 问题清单

### Q-T66-01 — 设计索引与 PLAN 同步滞后（轻微，L0）

| 字段 | 内容 |
|---|---|
| 提交 | `47d8557` |
| 现象 | ① DESIGN 0.3「个人记忆」仍为「设计中，未开工」，但 T-71/T-72 已合入且 PLAN 已勾；② PLAN M3-3 未勾（证据文件已入库）；③ DESIGN 0.3 脚注「无设计的四项」计数随之过时 |
| 影响 | 文档链 T1 一致性；不阻塞业务 |
| 建议归属 | 桌面组（记忆行随 T-72 更新权威设计入口或标注夹具范围）+ WA 组（M3-3 勾选）；随下个文档 PR 一并处理 |

> **提交前核对（2026-11-23，`540b1d4`/#223）**：①② 已由 #221～#223 解决——DESIGN 0.3 重构为五线分工（[2026-11-22-five-workstreams](../../decisions/2026-11-22-five-workstreams.md)），记忆行更新为「桌面记忆列表、纠错和撤回已接服务端夹具（T-71/72）」并挂特性档案 `features/conversation-memory/`；PLAN M3-3 已勾（#220）。本发现按验收时点（`47d8557`）保留存档。

### Q-T65-01（延续）— E2E-MOBILE 未修复

`47d8557` 复跑仍 failed。G15 三连败状态延续；今晚（11-23）定时预计再次失败。归属与建议不变（见 `t65-cross-group-acceptance.md` §4）。

### 遗留

| ID | 状态 |
|---|---|
| Q-T60-02（远程真 DES/站点 live） | 仍 NOT_VERIFIED；M3-4 与真站点接入另卡 |
| M3-4 浏览器生产读 Green | 计划内未做（本轮 4 Red）；完成后需独立复跑 |
| E2E-ADXR / T-73（read-cross） | 并行会话在途（主工作树未提交改动），未纳入本轮 |

---

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节）。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 桌面个人记忆：列表 / 详情 / 纠错 / 撤回全链（夹具权威）合同 + 浏览器双绿 | 记忆权威设计与持久化层（当前进程内夹具、无新表）——真存储与画像/taste 设计仍缺（宪章原则 10 待确认项） |
| 生产只读（M3）：Bind/Observe/水位/权限/续读/延迟/写关闭合同 6/6 绿 | **生产世界桌面入口未接线**（M3-4 浏览器 4 Red）；远程真站点 live 未验 |
| 审查「需人工」判定出口浏览器已通（E2E-REV） | WP2 剩余实现项按 WP2_IMPLEMENTATION_PLAN 推进（本轮未展开） |
| 桌面既有链（批准/提醒/受理/断线恢复）与 WA M2 写闭环回归全绿 | E2E-MOBILE 窄屏发送缺陷未修（Q-T65-01）；nightly 三连败（G15） |
| PR CI 全绿、master push 全绿（除最新一次在途） | 远程真 DES live（Q-T60-02）；生产写 M4 未开工 |

---

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务、契约正文、`features.json` 断言、GUARDRAIL_REGISTRY、PLAN/CASES（主工作树的并行在途改动未触碰）
- 未跑远程真站点/真 DES live；未做 M3-4 浏览器实现；未宣称生产写（M4）
- 未定位 E2E-MOBILE 引入提交（实现组根因分析）
- 验收记录开 PR 提交前待用户批准
