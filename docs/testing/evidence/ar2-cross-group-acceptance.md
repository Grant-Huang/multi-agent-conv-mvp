# AR-2 跨组场景独立验收（主线 #2：#270 全量 verify 指派 + MOBILE 收官）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 + #270 知会指派（「第 2 线在安静窗口复跑全量确认全绿」）+「270 已合，建议你在安静窗口复跑全量」指示（2026-11-24） |
| 验收人 | 本 worker（未参与 #270～#274 实现） |
| 基线 | `origin/master` `fa9970d`（#270 首页/旧壳整治 + 冲突解法、#273 convmem 5.3、#274 第 4 线入口适配，全部合入） |
| 检出方式 | 隔离 worktree `/tmp/ar2-wt` @ `fa9970d`，独立 CARGO_TARGET_DIR；**全量 verify 由 `verification/run.mjs` 全新构建**（npm run build + cargo bins，见 run.mjs:7-8），dist 无陈旧问题 |
| 窗口 | 安静窗口（18789/18787 全程本线独占；首跑曾遇他人 harness 占用，等待释放后执行） |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | **11-24 nightly 已触发（20:45Z）并失败——但编译关已过，失败归因首次干净**：E2E-MOBILE（旧壳树，30.1s 同签名）+ E2E-ADREAD（中间态：检出于 #274 合入前 3 分钟）。五晚连败，登记表不改 |
| 证据等级 | 本地/PR CI 为 T2；results.json 机读核对 |

## 0. 本轮范围（`33b2d05` → `fa9970d`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #270 | T-88 首页/旧壳整治（A1 欠账 + 生产界面去测试控制 + 入口迁移侧栏/设置） | **全量 verify（本卡指派）** |
| #273 | convmem 5.3 内省发现按需进入会话（确定性相关性门 + 待核实标注 + 只读） | `introspection_materials`（新套件） |
| #274 | 第 4 线入口适配（restart/CODEFLOW 过；FEEDBACK 登记欠账） | restart/codeflow 套件结果 |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| **Q-T65-01（E2E-MOBILE）** | **修复确认 → 建议关闭** | 全量 run.mjs（全新构建）`expected/passed` + 单独复跑 `1 passed`——**11 轮跟踪收官**：窄屏发送缺陷被 #270 新壳顺带修复（面板/抽屉语义重构），非针对性修复 |
| 指派任务：全量 verify | **执行完毕，未达全绿** | `npm run verify`（run.mjs）：**93 用例 = 54 passed / 29 failed / 10 skipped（agents 串行级联）**，18.8m |
| 29 个失败的构成 | **与 #270 评审预警一致** | 第 3/5 线未适配 27（des6+intro2+mom1+workbench1+ops6+remote3+write3+integration1+prod4）+ AGFIRST 1（Q-AR2-02）+ FEEDBACK 1（第 4 线登记欠账）；已适配面（desktop 32、agents 其余、codeflow CODEFLOW、restart、WARECV）全绿 |
| AGFIRST（Q-AR2-02） | **FAILED（第 1 线）** | 知会声称「已自修」，实测 `fa9970d` 仍红且**级联跳过同文件 9 个用例**（串行套件） |
| 合同层 | **VERIFIED / T2** | 9 套件 **64/64**（introspection_materials 5 新增；personal_memory 10——含 #265 的 5.2b） |
| PR CI #270/#273/#274 | **VERIFIED** | 全 pass |
| master push @ `fa9970d` | **FAILED（Q-AR2-04）** | rust 红（mcp.rs 用例，3m8s）；web/python-* 绿 |
| nightly 11-24 | **FAILED（归因干净）** | 编译关过 ✓；MOBILE（旧壳树）+ ADREAD（适配中间态）；**明晚起 nightly 将转入 29 例红形态，直至第 3/5 线适配合入** |

**总体**：本线指派任务**如实执行并交付数据**；全绿未达成的原因**全部在实现线适配欠账**（27 例未适配 + AGFIRST 回归 + FEEDBACK 欠账），非本线复验缺陷。**Q-T65-01 关闭是本轮最重要的正面结论**。

---

## 1. 本地证据（worktree @ `fa9970d`，run.mjs 全新构建）

### 1.1 全量 verify（指派任务，93 用例 / 18.8m）

```text
$ npm run verify   # = node verification/run.mjs（npm run build → cargo build bins → vitest → playwright）
stats: expected 54 / unexpected 29 / skipped 10 / flaky 0（results.json 机读核对）
```

失败 29 例（按套件）：des-experiment 6（DESTRIAL/DESSTART/DESOPS/DESPAUSE/DESISO/DESLOOP）、intro 2（INTROVIEW/INTROLABEL）、mom 1（MOMDYNAMIC）、mom-workbench 1（WABIND）、ops 6（OPSDESK/OPSDRAWER/OPSCHANNEL/OPSNOISE/OPSAPPROVE/ADREAD）、wa-des-remote 3（WADES/WAREAD/WAOBS）、wa-des-write 3（WAEXEC/WADYN/WALOOP）、wa-integration 1（WACHANNEL）、wa-prod-read 4（WAPROD/WAPISO/WAPREAD/WAPDELAY）、agents 1（AGFIRST）、codeflow 1（FEEDBACK，登记欠账）。

skipped 10 = agents.spec 串行文件 AGFIRST 之后的全部（AGLIST/AGTOML/AGGUIDE/AGCONNECT/AGUNDO/AGDISABLE/AGCHAT/AGCHOICE/AGDIAG/AGHELP）。

### 1.2 E2E-MOBILE 收官

```text
全量内：MOBILE expected/passed（results.json）
单独复跑：npx playwright test e2e/features.spec.ts -g 'MOBILE' → 1 passed (1.9s)
```

判定：#270 的面板/抽屉语义重构顺带修复窄屏发送缺陷（原症状：390×844 下点「发送」不发 POST）；**Q-T65-01 建议关闭**，归属第 1 线的修复载体为 #270。

### 1.3 合同层（9 套件 64/64）

```text
desktop_snapshot 4  desktop_drafts 4  personal_memory 10  session_materials 5
introspection_materials 5（#273 新）  desktop_mem 9  restart_semantics 11  mom_world_prod 9  desktop_c2 7
```

### 1.4 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 窄屏发送缺陷不再复现（新壳） | 通过 | 全量 + 单独复跑双绿 |
| 内省发现按需进会话（相关性门/待核实标注/只读） | 通过 | introspection_materials 5 |
| 个人记忆 5.2b 风格/画像隔离 | 通过 | personal_memory 10 |
| 已适配入口（desktop/agents-除 AGFIRST/codeflow-CODEFLOW/restart）真实可用 | 通过 | 全量内对应用例 passed |
| 测试注入改 API（attention/messages fixture） | 通过 | desktop.spec 适配面 passed |
| Mock / fixture 可辨 | 通过 | 同既有口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#270 / #273 / #274：全 pass（#274 收录时 8/8）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `fa9970d`（#274，run [45484566535](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45484566535)） | **FAILED**：`rust` 红——`expired_http_session_does_not_replay_tool_calls`（agents/tests/mcp.rs:350）FAILED，3m8s 早段；与 `cf8d793` 的 rt_restart_09 **为两不同用例连续两推两红 → 升级为 Q-AR2-04（master rust CI 系统性不稳定）** |
| **nightly @ 11-24 20:45Z**（run [45484039389](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45484039389)） | **failure，运行期归因首次干净**：E2E-MOBILE（旧壳树 b09a949——经核对 **#270 不在其祖先**，检出于 20:45Z 而 #270 主体合入更晚）+ E2E-ADREAD（适配中间态）。五晚连败；编译关（#259）经受住首次检验 |

## 4. 问题清单

### Q-AR2-01 — #270 合并先于第 3/5 线适配完成，master 浏览器 29 红（高）

| 字段 | 内容 |
|---|---|
| 现象 | 约定流程为「各线适配 → 本线全量复跑 → 合并 #270」；实际 #270 与 #274（仅第 4 线）先行合入，第 3 线 25 例 + 第 5 线 2 例未适配即上 master |
| 影响 | **明晚起 nightly 转 29 例红形态**（评审预警成真）；master 浏览器质量门失真，掩盖新回归 |
| 建议 | 第 3/5 线按 `/tmp/t88-notice.md` 适配（断言不变）；适配合入后本线再跑一轮全量确认收敛；流程上「适配完成」应以本线复跑记录为合并前置（本卡即该记录，但合入发生在复跑前） |
| 归属 | 第 3 线 27 例、流程时点归用户/舰队约定 |

### Q-AR2-02 — AGFIRST 声称已修实测仍红，且串行级联跳过 9 例（中，第 1 线）

| 字段 | 内容 |
|---|---|
| 现象 | 知会称「主线已自修 AGFIRST」；`fa9970d` 全量实测 FAILED，同文件 10 例中 9 例 skipped |
| 建议 | 第 1 线复核修复在当前 master 的有效性（或受 #273/#275 邻近改动影响）；串行文件考虑失败不级联的隔离策略（如 test.describe.configure）——不改断言 |

### FEEDBACK（已知欠账，第 4 线登记）

#274 明确登记，本卡如实计入 29 例；不另立编号。

### Q-AR2-04 — master rust CI 系统性不稳定：连续两推两红、两不同用例（高，master 阻塞）

| 字段 | 内容 |
|---|---|
| 事实 | ① `cf8d793`（AR-1）：rt_restart_09（restart_semantics.rs:584）FAILED；② `fa9970d`（本卡）：expired_http_session_does_not_replay_tool_calls（agents/tests/mcp.rs:350）FAILED——两次失败用例不同，均 rust cargo test，fmt/clippy 绿 |
| 本地 | 两用例本地均通过（rt_restart_09 曾 10/10；mcp 套件本卡未单跑但 PR rust 作业同码通过） |
| 判定 | CI 并行资源竞争下的时序敏感测试类问题（会话/超时/异步语义类用例集中），**非单点**——master 绿灯失去指示性 |
| 建议 | 工程/CI：① 复跑机制（同 run 自动重试一次并标记 flaky）或拆分时序敏感套件限并行；② 实现线排查两类用例的固定时序假设（超时/ sleep / 端口）；③ 修复前 master 红不阻塞合并判断，但需在 PR 说明 |
| 归属 | 工程/CI（机制）+ 第 4 线（两个用例分属 restart/mcp 链） |

### 已关闭

**Q-T65-01（E2E-MOBILE）：修复确认（#270 载体），本卡按证据关闭。** G15 运行期阻塞转入 Q-AR2-01 的适配欠账。

---

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **窄屏发送缺陷修复**：跟踪 11 天（T-65→AR-2）的 G15 运行期阻塞清零——桌面入口移动形态恢复可用 | 入口适配欠账 27 例（第 3/5 线）——nightly 转红形态的根治 |
| 首页/旧壳整治落地且真实/模拟、测试控件收编可辨（desktop 32 绿） | AGFIRST 回归（第 1 线）；FEEDBACK 欠账（第 4 线） |
| 内省发现进会话（5.3）与记忆 5.2b 合同全绿 | 分层记忆 REQ-03/04（预览与生效闸门）；画像/taste 设计定稿 |
| rt_restart_09（Q-AR1-01）在本轮合同 11/11 中未复现（CI 抖动待修，已并入 Q-AR2-04 系统性结论） | **master rust CI 两连败（Q-AR2-04）**；master push 全绿缺失；远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替任何线适配其套件（知会明确各线自适配）
- 未宣称全量全绿（实际 29 红，如实记录）；远程真站点未跑
