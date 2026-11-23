# T-80 跨组场景独立验收（主线 #2：M3 收官复核 + 首页委派 + 记忆持久化）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+ 用户「继续主线下一步」指示（2026-11-23） |
| 验收人 | 本 worker（未参与 #224～#238 实现；T-67/T-79 为其他线自查轮，本卡为全量独立复验轮） |
| 基线 | `origin/master` `ba9a7c7`（含 #224～#238；#226 为本线 T-65/T-66 记录） |
| 检出方式 | 隔离 worktree `/tmp/t80-wt` @ `ba9a7c7`，独立 CARGO_TARGET_DIR 全新构建；web 依赖与 Playwright 版本未变（lockfile 无 delta），复用 node_modules |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点/真 DES = **NOT_VERIFIED**；T-75 真机麦克风与 T-76 远程 ASR 的真设备路径按其证据口径不宣称 |
| G15 / nightly | 最新仍为 11-22 失败（run 45052408864）；**11-23 定时收稿时点未触发**。三晚连过条件不满足，登记表**不改** |
| 证据等级 | 本地/PR CI/master push 所列为 T2；真设备与远程 live 为 NOT_VERIFIED |

## 0. 本轮范围（T-66 基线 `47d8557` → `ba9a7c7` 的新合入）

| PR | 内容 | 本轮复验入口 |
|---|---|---|
| #224 | M3-4 生产只读浏览器 E2E Green | `wa-prod-read.spec` 4 条（T-66 时为计划内 Red） |
| #225/#229 | T-74 麦计划 / T-75 真机麦 | E2E-ADMIC-01～04（真设备路径不宣称） |
| #227 | RT-TPL-12 join 交付修复（bugfix） | `template_cases` + `template_orch` + `rt_tpl_10_capacity` |
| #228 | WA M3-5 收尾清单 + `wa-m3-suite.sh` | 套件脚本全量执行 |
| #230 | M4-0 生产写门禁 | `mom_world_prod`（WRITE_DISABLED 保持） |
| #231 | WP3 代码流 E2E | `codeflow.spec` |
| #232 | T-76 远程 ASR | E2E-ADASR-01～02（remote-stub 口径） |
| #233 | 修复计划文档（r-20261121-m2/m5/m6） | docs，无业务断言 |
| #234 | T-77 双设备 D01 | E2E-ADD01-01～03（双 browser context） |
| #235/#238 | 首页 S1 真实委派切片（B1+GS3）+ 终态交付回发起会话 | desktop.spec 全量（含 E2E-HOMEREJ 等新用例） |
| #236/#237 | T-78 记忆持久化方案与实现（`desktop_memories` → Store） | `desktop_mem` 7 用例 + E2E-ADMEM/ADMEMW 回归 |

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 合同层（an-server 6 套件 + RT-TPL 3 套件） | **VERIFIED / T2** | **59 passed / 0 failed**（41+18；desktop_mem 增至 7，T-78 持久化） |
| 桌面浏览器全量 `desktop.spec` | **VERIFIED / T2** | **31 passed**（含委派切片、终态交付、麦克风、ASR、双设备、记忆、ADXR） |
| M3-4 生产只读浏览器 | **VERIFIED / T2** | `wa-prod-read` **4 passed**——T-66 计划内 Red 已转绿，独立确认 |
| WP3 代码流浏览器 | **VERIFIED / T2** | `codeflow.spec` 2 passed |
| features 全量 | **12 passed + 1 failed** | 唯一失败 = **E2E-MOBILE**（Q-T65-01 第 4 轮确认未修）；E2E-REV 等 12 条绿 |
| WA 写与 M0/M1 回归 | **VERIFIED / T2** | `wa-des-write` 4 passed；`wa-m2-regression` green；**`wa-m3-suite` 全绿**（M0→M3 四级一次通过） |
| PR CI #224～#238（13 个） | **VERIFIED** | 全部 8/8 pass |
| master push @ `ba9a7c7` | **VERIFIED** | run 45231143046 **success**（T-79 收稿时在途的 #237/#238 两笔现均已 success：45231108751 / 45231143046） |
| E2E-ADD01-02 顺序敏感 flake（T-67 观察） | **本轮未复现** | 整跑顺序执行 3 条双设备用例全绿；维持「间歇性、归第 1 线测试基建」观察 |

**总体**：T-66 以来新合入链路（M3 收官、M4-0 门禁、首页委派、记忆持久化、双设备、ASR、代码流）独立复验 **全绿**；唯一业务缺陷仍是 E2E-MOBILE（未修）。

---

## 1. 本地证据（worktree @ `ba9a7c7`，全新 target）

### 1.1 合同层（59/59）

```text
$ cargo test --locked -p an-server --test desktop_mem --test mom_world_prod \
    --test desktop_c2 --test desktop_accept --test desktop_replay --test mom_world_des_write
desktop_accept: 4   desktop_c2: 7   desktop_mem: 7（T-78 持久化 +1）
desktop_replay: 5   mom_world_des_write: 12   mom_world_prod: 6（M4-0 门禁保持）
$ cargo test --locked -p an-runtime --test template_cases --test template_orch \
    -p an-llm --test rt_tpl_10_capacity
template_cases: 10   template_orch: 7   rt_tpl_10_capacity: 1   （RT-TPL-12 join 交付修复 #227）
```

### 1.2 浏览器（正常桌面入口）

```text
$ npx playwright test e2e/desktop.spec.ts          → 31 passed (25.1s)
$ npx playwright test e2e/codeflow.spec.ts         → 2 passed
$ npx playwright test e2e/wa-prod-read.spec.ts     → 4 passed（M3-4 转绿）
$ npx playwright test e2e/wa-des-write.spec.ts     → 4 passed
$ npx playwright test e2e/features.spec.ts         → 12 passed, 1 failed（E2E-MOBILE）
```

双设备 D01 三条（#234）在整跑顺序执行下全绿；麦克风四条为 harness 授权桩口径（真设备不宣称，见 T-75 证据）。

### 1.3 套件脚本

```text
$ ./scripts/wa-m2-regression.sh → write 12 + read 12 + DES 4 + DESC 6 = green
$ ./scripts/wa-m3-suite.sh      → wa-m0/m1/m2/m3 suite 全部 green
  （含 RT-WA-01～09: 9 passed；DES*+WABIND+WACHANNEL 浏览器 8 passed；WADES 浏览器 3 passed；WA 写浏览器 4 passed）
```

### 1.4 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 记忆持久化落 Store 且接口语义不变 | 通过 | `desktop_mem` 7 用例（原 01～06 语义保持 + 持久化）；E2E-ADMEM/ADMEMW 回归绿 |
| 首页委派真实受理（B1）+ 交付回发起会话（通过/拒绝终态） | 通过 | desktop.spec 新用例 + E2E-HOMEREJ（T-79 口径复确认） |
| RT-TPL-12 join 三路交付 | 通过 | `template_orch`（#227 修复面） |
| 生产写门禁（M4 前关闭） | 通过 | `mom_world_prod` WRITE_DISABLED 用例 |
| 生产只读浏览器 provenance / 水位 / 续读去重 / as_of | 通过 | E2E-WAPROD/WAPISO/WAPREAD/WAPDELAY（#224 后转绿） |
| 双设备隔离与授权共享 | 通过 | E2E-ADD01-01～03、E2E-ADHAND |
| ASR 远程 stub 与无密钥回退 | 通过 | E2E-ADASR-01～02 |
| 代码流（WP3） | 通过 | E2E-codeflow 2 条 |
| Mock / fixture 可辨 | 通过 | harness world_mock；记忆=Store 真表（T-78 后非夹具）；生产=stub；麦克风/ASR 桩口径；真设备/远程不宣称 |
| 远程真站点 / 真 DES / 真设备 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#224～#238 中 13 个实现 PR（#226 为本线记录、#233 纯 docs）**全部 8/8 pass**（逐个 `gh pr checks` 核对，2026-11-23）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `ba9a7c7`（#238，run [45231143046](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45231143046)） | **success**（15m40s，全作业） |
| push @ #237 合并（run 45231108751） | **success**（T-79 收稿时在途，现已终态，可回填其 §3） |
| nightly | 最新仍 11-22 失败；**11-23 定时收稿时点未触发**；G15 / GR-4-NIGHTLY-E2E 状态不变 |

---

## 4. 问题清单

### Q-T65-01（延续，第 4 轮确认）— E2E-MOBILE 未修复

`ba9a7c7` 复跑仍 failed（features 全量中唯一失败）。#224～#238 与 #233（修复文档）均未触及。归属与建议不变（Desktop/前端，与 G15 根因合并处理）。**提示**：G15 要三晚连过必须先修此缺陷——它已是 nightly 11-22 失败的直接原因候选，且 11-23 起每晚都会踩。

### 观察（非新缺陷）

| 项 | 说明 | 归属 |
|---|---|---|
| E2E-ADD01-02 顺序敏感 flake | T-67 记录的批量顺序依赖，本轮整跑未复现（3 条双设备全绿）；维持观察 | 第 1 线测试基建 |
| T-79 待回填项 | #237/#238 master push CI 现已 success（见 §3），可回填 PR #239 与证据 §3 | 第 4 线 |

### 遗留

| ID | 状态 |
|---|---|
| Q-T60-02 / 远程真站点 live | 仍 NOT_VERIFIED（M4 真实写与试点窗口绑定为五线分工前提） |
| T-75 真机麦克风 / T-76 真实 ASR 服务 | 桩口径通过；真设备路径按各卡证据不宣称 |

---

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **M3 生产只读四级套件全绿**：合同 + 浏览器（provenance/水位/权限/续读/延迟/写关闭） | 远程真站点读入未接（试点窗口前提未落实，五线分工既定） |
| **首页委派切片接真**：S1 真实受理 + 终态交付回发起会话（通过/拒绝） | 委派切片的更完整首页工作流（GS3 之外）按第 1 线计划推进 |
| **记忆落真存储**：desktop_memories → Store，接口语义与浏览器链回归绿 | 分层记忆与注入仍未实现（第 5 线 T-78 后续）；画像/taste 设计未定稿 |
| 麦克风/ASR/双设备桌面体验链（桩口径） | 真设备与真实 ASR 服务路径 NOT_VERIFIED |
| M4-0 写门禁就位（WRITE_DISABLED） | M4 首个生产写动作未开工 |
| 全量回归：合同 59 + 浏览器 41 绿，套件脚本 M0→M3 全绿 | **E2E-MOBILE 未修 → nightly 无法三晚连过 → G15/G6 无法关闭**（当前最大短板） |

---

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务、契约正文、`features.json`/CASES 断言、GUARDRAIL_REGISTRY、PLAN
- 未修 E2E-MOBILE、未做真设备/真实 ASR/远程真站点验证、未宣称 M4
- 未复跑 T-67/T-79 已独立完成的自查范围全量（本卡以全量轮覆盖其共同基线 `ba9a7c7`）
