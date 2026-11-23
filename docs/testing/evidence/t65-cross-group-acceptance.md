# T-65 跨组场景独立验收

| 项目 | 内容 |
|---|---|
| 任务 | T-65 跨组场景独立验收（任务卡未给 ID，此为建议值，**待用户确认**） |
| 授权来源 | 用户对话任务卡（独立验收角色：输入任务卡/PR/Feature Map/CASES/证据，输出验收记录与可复现问题清单） |
| 验收人 | 本 worker（未参与 #203～#212 实现） |
| 基线 | `origin/master` `367d85d`（含 #203～#212） |
| 本地检出 | `2855cd4`（与 `367d85d` 的 diff 经核实**仅 docs/**，15 文件无 crates/web 改动，代码等价） |
| 日期 | 2026-11-22（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真 `des_mcp` = **NOT_VERIFIED** |
| G15 / nightly | 连续第 3 晚失败（run 45052408864）。「三晚连续通过」条件**不满足**，登记表**不改** |
| 证据等级 | 本节所列均为 T2（真实执行、通过/失败数可核对）；G15 根因与远程真厂为 NOT_VERIFIED |

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| Desktop T-64 新链：RT-ADREPLAY-01～05 + E2E-ADRESUME | **VERIFIED / T2** | 合同 5 passed；桌面入口真实 SSE E2E 1 passed（独立复验，作者证据之外首次） |
| Desktop 回归：RT-ADAPPR/ADATTN/ADACCEPT + E2E | **VERIFIED / T2** | 合同 11 passed；E2E 3 passed |
| WA 写路径：RT-WADES-09～19 + E2E-WA* + regression | **VERIFIED / T2** | 合同 12 passed；E2E 4 passed；`wa-m2-regression.sh` write12+read12+DES4+DESC6 green |
| E2E-MOBILE（窄屏会话抽屉） | **FAILED（可复现）** | master `8debd3e` nightly 失败；本地 `34fb1ab`/`afb69d6`/`2855cd4` 三个提交均确定性失败 → 见 Q-T65-01 |
| PR #212 CI | **VERIFIED** | 全 pass，含 `security`（`url` ≥2.5.9 后 `cargo audit` 转绿） |
| master push CI @ `367d85d`（run 45078271876） | **VERIFIED** | rust 9m24s / web / security / python-* 全 success；change-policy skipped（push 事件） |
| nightly-e2e @ `8debd3e`（run 45052408864） | **FAILED** | E2E-MOBILE 超时；升级链生效（自动开 issue #208）；G15 状态不变 |
| 远程真 DES live | **NOT_VERIFIED** | `AN_MOM_DES_MCP` unset（Q-T60-02 遗留，本轮非目标） |

**总体**：桌面入口成功链与失败/恢复链、WA 写闭环独立复验 **PASS**；T-64 新链独立验收 **通过**；**E2E-MOBILE 确认可复现缺陷**（与 G15 三晚失败同源候选），交实现组修复。

---

## 1. 本地证据（本会话复跑）

环境：本机 macOS；`CARGO_TARGET_DIR=target`；分支检出 `2855cd4`（代码 = master `367d85d`）。

### 1.1 合同层（成功 + 失败/恢复链）

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --locked \
    --test desktop_replay --test desktop_c2 --test desktop_accept --test mom_world_des_write

desktop_accept:        test result: ok. 4 passed; 0 failed   （RT-ADACCEPT-01～04）
desktop_c2:            test result: ok. 7 passed; 0 failed   （RT-ADAPPR-01～05、RT-ADATTN-01～02）
desktop_replay:        test result: ok. 5 passed; 0 failed   （RT-ADREPLAY-01～05）
mom_world_des_write:   test result: ok. 12 passed; 0 failed  （RT-WADES-09～19、m25）
合计 28 passed / 0 failed
```

失败/恢复覆盖：RT-WADES-11（取消待发不投递）、15（关联键恢复）、16（部分回执补齐）、17（工厂重启不误投）；RT-ADREPLAY-02（重复事件去重）、04（SQLite 重启对账）；RT-ADAPPR-03（拒绝后不执行）/05（unknown→ack）。

### 1.2 桌面入口浏览器 E2E（正常入口）

```text
$ cd web && PLAYWRIGHT_BROWSERS_PATH=… npx playwright test e2e/desktop.spec.ts -g 'ADRESUME|ADAPPR|ADATTN|ADACCEPT'
✓ E2E-ADATTN / E2E-ADAPPR / E2E-ADRESUME / E2E-ADACCEPT
4 passed (4.8s)
```

E2E-ADRESUME 含失败/恢复语义：任务 A **断线重放（含重复事件）**、双任务不串写、任务 B waiting_input→running；SSE 为真实 `/api/desktop/v1/events` 流（T-64 实现），种子事件经 harness fixture `bus.publish` 注入（登记过的 Mock，见 MOCK_REGISTRY M2）。

### 1.3 WA 写闭环浏览器 E2E（含恢复链）

```text
$ npx playwright test e2e/wa-des-write.spec.ts
✓ E2E-WAEXEC / E2E-WARECV / E2E-WADYN / E2E-WALOOP
4 passed (1.8s)
```

E2E-WARECV = 失败/恢复链：丢回执恢复 + 工厂重启不误投。

### 1.4 WA M2 回归脚本

```text
$ ./scripts/wa-m2-regression.sh
== RT-WADES write == 12 passed   == RT-WADES read == 12 passed
== RT-DES-08～11 == 4 passed     == RT-DESC connector == 6 passed
== wa-m2 regression green ==
```

### 1.5 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 审批版本绑定与升版失效 | 通过 | RT-ADAPPR-02；E2E-ADAPPR |
| 拒绝不执行 / 恰好一次 / unknown→ack | 通过 | RT-ADAPPR-03～05 |
| Propose 绑定 plan_revision | 通过 | RT-WADES-09 |
| Execute 幂等；取消守卫 | 通过 | RT-WADES-10/11；E2E-WAEXEC |
| 丢回执 Confirm / 关联键恢复 / 部分回执 | 通过 | RT-WADES-12/15/16；E2E-WARECV |
| 工厂实例变化拒陈旧 / 重启不误投 | 通过 | RT-WADES-13/17 |
| SSE 游标契约（event_id / cursor） | 通过 | RT-ADREPLAY-01 |
| 断线重放去重 / 重启对账 | 通过 | RT-ADREPLAY-02/04 |
| 双任务不串写 / 状态推进 | 通过 | E2E-ADRESUME |
| 受理幂等（同操作 ID 冲突可辨） | 通过 | RT-ADACCEPT-04；E2E-ADACCEPT |
| Mock / fixture 可辨 | 通过 | harness `world_mock` + 推理 Mock；MOCK_REGISTRY M2（T-64 fixture 注入）；WA 服务层；不宣称远程真厂 |
| 远程真 des_mcp | **NOT_VERIFIED** | 环境无 `AN_MOM_DES_MCP` |

### 1.6 E2E-MOBILE 本地复现与二分（Q-T65-01 证据）

| 提交 | 背景 | 本地结果 |
|---|---|---|
| `2855cd4`（= master 代码） | 本周窗口 | **failed ×3** |
| `afb69d6`（T-63 基线） | #206 后 | **failed ×1**（重建 binary+dist 后） |
| `34fb1ab` | 11-21 nightly 通过的提交 | **failed ×2**（重建 binary+dist 后） |

同环境对照：E2E-TICKER（同一 `send()` 助手、默认视口）在 `2855cd4` 本地 **passed** → 发送链路本身在本地可用，失败为**窄屏视口特有**。

trace 证据（`--trace on`，已解包分析）：点击「发送」后**网络层无任何 `POST /api/conversations/*/messages`**（仅有 GET 历史回读）；console 仅一条与本用例无关的 404（`/api/mom/desk/unread`，harness 未启用运营台路由的环境噪音）。

二分口径说明：三次检出的 `target/debug/agentnexus` 与 `web/dist` 均按该提交重建；`target/debug/an-mock-acp` 未随提交重建（常量），但 E2E-TICKER 用同一二进制组合通过发送全链，故不影响「窄屏特有、失败早于本周窗口」的结论。更早的引入提交定位留给实现组根因分析（GAP_ANALYSIS 路线图第 2 项）。

---

## 2. PR CI 证据（合入前）

| PR | 结果 | 说明 |
|---|---|---|
| [#212](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/212)（T-64） | 全 pass | 含 `security` pass（`cargo audit` 绿）；change-policy、shadow-review pass |

PR CI 不跑 Playwright（同 T-60/T-63 口径）；浏览器 E2E 以本会话本地为准。

---

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push CI @ `367d85d`（run [45078271876](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45078271876)） | **success**：rust 9m24s、web、security、python-* 全绿；change-policy skipped（push 事件） |
| nightly @ `8debd3e`（run [45052408864](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45052408864)，2026-11-22） | **failure**：E2E-MOBILE 30s 超时（`waitForResponse` / `locator.click`）；自动开 issue #208（升级链生效） |
| nightly 11-21（run 44931610796 @ `34fb1ab`） | failure：E2E-AGFIRST（与 MOBILE 不同用例） |
| nightly 11-20（run 44795474606 @ `2cf5557`） | failure：E2E-AGCONNECT + E2E-OPSNOISE |
| G15 / `GR-4-NIGHTLY-E2E` | **不改状态**：三晚均为失败，不满足「连续三晚通过」更新条件；失败跟进链已接（issue #208），根因候选见 Q-T65-01 |

---

## 4. 问题清单（可复现）

### Q-T65-01 — E2E-MOBILE 窄屏抽屉下「发送」不触发 POST（G15 根因候选）

| 字段 | 内容 |
|---|---|
| 提交 | CI：`8debd3e`（nightly 45052408864）；本地复现：`34fb1ab`、`afb69d6`、`2855cd4` |
| Case | E2E-MOBILE（`docs/testing/CASES.md` L12，navigation） |
| 步骤 | `npx playwright test e2e/features.spec.ts -g 'MOBILE'`；390×844 视口 → 打开「☰ 会话」抽屉 → 「＋ 新会话」→ 填「输入消息」→ 点「发送」 |
| 期望 | 发出 `POST /api/conversations/{id}/messages`，助手回「你好，验证成功」 |
| 实际 | 点击成功但**无任何 POST**（trace network 仅 GET）；`waitForResponse` 30s 超时；失败率：本地 6/6（跨三提交）、CI 11-22 失败、CI 11-21 同码通过 → 环境敏感的窄屏缺陷 |
| 排除项 | 非本周窗口引入（`34fb1ab` 早于 T-61/#206/T-64，本地已失败）；非本地发送链路损坏（TICKER 同链通过）；console 无 JS 异常（仅无关 404） |
| 建议归属 | Desktop / 前端（features 窄屏抽屉与发送链路）；与 G15 根因分析（GAP_ANALYSIS 路线图第 2 项）合并处理 |
| 严重度 | 高（阻塞 G15 关闭；移动入口发送不可用） |
| 线索 | spec 近期改动 `82069ae`/`1198448`/`3cf01d8`；曾有一次夜间修复 `afaabf3`（#197） |

### Q-T65-02 — nightly 升级 issue 建成后无标签（轻微）

| 字段 | 内容 |
|---|---|
| 提交 | workflow（nightly-e2e.yml） |
| 现象 | `could not add label: 'nightly-e2e' not found` → issue #208 无标签；按标题去重的搜索依赖标签 |
| 建议 | 建仓库 label `nightly-e2e` 或改用无标签去重策略；归属工程/CI，随 G15 修复一并处理 |

### 遗留与关闭

| ID | 状态 | 说明 |
|---|---|---|
| Q-T60-02（远程真 DES live） | **仍 NOT_VERIFIED** | 本轮仍无 `AN_MOM_DES_MCP`；WA 组另卡 |
| Q-T63-01（master `cargo audit` 红） | **本轮关闭** | `url` ≥2.5.9 后：#212 PR CI security pass + master push `367d85d` security pass |

---

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节）。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 桌面入口四链（批准 / 提醒 / 受理 / 断线恢复）合同 + 浏览器双绿 | **窄屏移动入口发送缺陷**（Q-T65-01）——桌面入口的移动形态不可用 |
| 桌面→仿真工厂写闭环（WA M2）全绿，含取消/恢复/重启不误投 | 远程真 DES live 未验（Q-T60-02）；生产 MES 未接（WA M3 进行中，#213 Red 空壳在途，未纳入本轮） |
| T-64 断线恢复从 Mock 升级为真实 SSE，独立复验通过 | T-64 非目标部分：独立 Run 表 / orchestrator 续跑（作者证据「非本卡」项，尚未立项验证） |
| 依赖审计转绿（Q-T63-01 关闭） | nightly 三连败（G15 未闭，Q-T65-01 根因待修复后三晚连过才能改状态） |
| 失败跟进链生效（nightly 自动开 issue #208） | 真机语音、质检「需人工」出口（WP2 方案已批未实施）、DLP 只查登记出口（宪章既有差距，本轮未涉） |

---

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务、契约正文、`features.json` 断言、GUARDRAIL_REGISTRY
- 未纳入在开 PR #213（WA M3-1 Red 空壳，未合入）
- 未跑远程真 DES；未宣称真机语音或生产 MES
- 未定位 E2E-MOBILE 的引入提交（属实现组根因分析）
