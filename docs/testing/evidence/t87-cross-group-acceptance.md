# T-87 跨组场景独立验收（主线 #2：首页快照 / 草稿上云 / 个人记忆生命周期）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+「继续」指示（2026-11-23） |
| 验收人 | 本 worker（未参与 #250～#255 实现） |
| 基线 | `origin/master` `e22208b`（含 #250～#255；#254 为本线 T-85，已合并） |
| 检出方式 | 隔离 worktree `/tmp/t87-wt` @ `e22208b`，独立 CARGO_TARGET_DIR 全新构建 |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED**；M3 live 单站点探针（#251）为验收脚本加强，live 结论仍归第 3 线 |
| G15 / nightly | **11-23 nightly 已触发（22:10Z，run 45277117267）并失败**——根因 TS6133（见 §3/§4）；**与 11-22 的 MOBILE 不同源**。四晚连败，登记表不改 |
| 证据等级 | 本地/PR CI/master push 为 T2；nightly 失败原因 T2（log 核对） |

## 0. 本轮范围（`1a9ae70` → `e22208b`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #250 | T-82 首页卡接真实快照（AD-G7 / `GET /snapshot`） | `desktop_snapshot` RT-ADSNAP-01～04 + desktop.spec |
| #251 | M3 live Observe 验收探针单站点模式 | `scripts/accept-m3-live-observe*`（脚本面，live 结论归第 3 线） |
| #252 | convmem 5.2a 个人记忆生命周期与隔离（POST 保存、**kind 补列**、按说话人作用域） | `personal_memory` RT-UMEM |
| #253 | T-84 第 4 线验收记录 | docs |
| #255 | T-86 draftStore 上云（草稿服务端权威 + 乐观版本） | `desktop_drafts` RT-ADDRAFTS-01～04 |

**任务号**：T-86 已被 #255 占用，本卡取 **T-87**（worktree/PR/PLAN 查重通过）。
**迁移核查**：`store.rs` 附 `ALTER TABLE desktop_memories ADD COLUMN kind TEXT NOT NULL DEFAULT 'fact'`——符合「表结构变更必须附迁移」约定（旧库默认 `kind='fact'`）。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 首页真实快照（#250） | **VERIFIED / T2** | `desktop_snapshot` **4 passed**：真实/模拟卡可辨（`simulated` 标记——宪章原则 8）、空库不报错不静默回退、reset 幂等、B1 受理后真任务入卡 |
| 草稿上云（#255） | **VERIFIED / T2** | `desktop_drafts` **4 passed**：PUT/GET 回读、**409 version_conflict 不覆盖**、删除幂等、跨重启持久 |
| 个人记忆生命周期（#252） | **VERIFIED / T2** | `personal_memory` **5 passed**：POST 201/纠错/撤回 410 revoked/列表隔离（按说话人作用域） |
| 会话资料注入回归 | **VERIFIED / T2** | `session_materials` 5 passed |
| 既有回归 | **VERIFIED / T2** | desktop_mem 9、desktop_c2 7、mom_world_prod 8 全绿；**合同合计 42/42** |
| 桌面浏览器全量 | **VERIFIED / T2** | desktop.spec **31 passed**（首页快照接线）；E2E-REV 绿 |
| E2E-MOBILE | **FAILED（第 7 轮本地确认）** | Q-T65-01 未修；注意 11-23 nightly 失败**非** MOBILE（verify 在 TS 阶段即死，见 §3） |
| PR CI #250～#255 | **VERIFIED** | 全部 8/8 pass |
| master push @ `e22208b` | **VERIFIED** | run 45288148204 **success** |
| nightly @ 11-23 | **FAILED（新根因）** | run [45277117267](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45277117267) 22:10Z，2m44s 死于 **TS6133**（#249 新文件），测试未执行——四晚连败但失败原因首次换源 |

**总体**：五条新链独立复验 **PASS**，无新业务缺陷；nightly 失败根因转移为 TS 编译卫生问题（Q-T87-01/02）。

---

## 1. 本地证据（worktree @ `e22208b`，全新 target）

```text
$ cargo test --locked -p an-server --test desktop_snapshot --test desktop_drafts \
    --test personal_memory --test session_materials --test desktop_mem \
    --test desktop_c2 --test mom_world_prod
desktop_snapshot: 4（RT-ADSNAP-01～04）   desktop_drafts: 4（RT-ADDRAFTS-01～04）
personal_memory: 5（RT-UMEM）             session_materials: 5
desktop_mem: 9   desktop_c2: 7   mom_world_prod: 8
合计 42 passed / 0 failed

$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts             → 31 passed (23.9s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → REV ✓；MOBILE ✗（同签名）
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 首页快照真实/模拟可辨（`simulated` 标记） | 通过 | RT-ADSNAP-01/02（宪章原则 8） |
| B1 受理真任务入快照卡 | 通过 | RT-ADSNAP-02 |
| 空库快照不报错、不静默回退静态卡 | 通过 | RT-ADSNAP-04 |
| fixture reset 幂等 | 通过 | RT-ADSNAP-03 |
| 草稿乐观版本：409 冲突不覆盖原内容 | 通过 | RT-ADDRAFTS-02 |
| 草稿删除幂等、跨重启持久、kind 非法 400 | 通过 | RT-ADDRAFTS-03/04 |
| 个人记忆 POST 201 / 纠错 / 撤回 410 revoked / 说话人作用域隔离 | 通过 | RT-UMEM-01～02+ |
| 记忆 kind 补列迁移存在（旧库默认 fact） | 通过 | `store.rs` ALTER TABLE 核查 |
| Mock / fixture 可辨 | 通过 | fixture reset 显式端点；快照 simulated 标记 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#250 / #251 / #252 / #253 / #255：全部 **8/8 pass**（逐个核对，2026-11-23）。

## 3. master push / nightly 证据

| 项 | 状态 |
|---|---|
| push @ `e22208b`（#255，run [45288148204](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45288148204)） | **success** |
| **nightly @ 11-23 22:10Z**（run [45277117267](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45277117267)） | **failure，2m44s**：`npm run verify` 在 TS 编译即死——`e2e/helpers/restartServer.ts(41,13)` 与 `e2e/restart.spec.ts(34,9)` **TS6133 未使用变量**（均为 #249 新增文件）；**vitest/playwright 未执行**。本晚失败与 MOBILE 无关 |
| G15 / GR-4-NIGHTLY-E2E | 四晚连败（11-20 AGCONNECT+OPSNOISE / 11-21 AGFIRST / 11-22 MOBILE / **11-23 TS6133**）；三晚连过条件不满足，登记表不改 |

## 4. 问题清单

### Q-T87-01 — #249 新增 e2e 文件 TS6133 致 nightly 编译失败（高，G15 直接原因）

| 字段 | 内容 |
|---|---|
| 提交 | `5b07587`（#249）文件 `web/e2e/helpers/restartServer.ts` L41、`web/e2e/restart.spec.ts` L34 |
| 现象 | `npm run verify` 的 TS 编译报 TS6133（未使用变量 `goal`/`api`），exit 1，测试未执行 |
| 期望 | verify 编译干净并跑完全量 |
| 建议 | 删除两处未使用变量（分钟级修复）；归属 **第 4 线**（文件作者链） |
| 影响 | G15 第 4 晚失败的直接原因；不修则后续每晚编译即死，MOBILE 等运行期缺陷反而被遮蔽 |

### Q-T87-02 — PR CI 与 nightly verify 的 TS 检查范围不一致（系统性，工程/CI）

| 字段 | 内容 |
|---|---|
| 现象 | #249 PR CI web 作业 8/8 pass（含 typecheck），合入后 nightly verify 的 TS 编译即红——两道门的 TS 项目/范围不同（PR typecheck 未覆盖 `web/e2e/**` 或配置不同） |
| 影响 | 运行期之外的质量门在 PR 层漏放，nightly 才拦住——违反「左移」原则，也让 nightly 长期背编译卫生债 |
| 建议 | 统一：PR web 作业跑与 verify 相同的 TS 编译范围（或 verify 拆出 compile 步进 PR）；归属 **工程/CI**，随 G15 修复一并处理 |

### Q-T65-01（延续，第 7 轮本地确认）— E2E-MOBILE 未修复

`e22208b` 本地同签名失败。注意：11-23 nightly 失败**不**由 MOBILE 引起（verify 未跑到运行期）；G15 关闭需先修 Q-T87-01（编译）+ Q-T65-01（运行期），两缺陷缺一不可。归属不变。

### 遗留

Q-T60-02（远程真站点 live）NOT_VERIFIED；#251 单站点探针的 live 结论归第 3 线；任务号竞争延续（T-86 被 #255 占用，本卡 T-87）。

---

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **首页快照读模型**：真实/模拟可辨、空库不回退——原则 5/8 在首页落地 | 首页快照的实时刷新/游标续读（后续卡） |
| **草稿上云**：服务端权威 + 乐观版本 + 跨设备/重启持久——多端协作基础 | 真多端并发冲突的浏览器级用例（现合同层覆盖） |
| **个人记忆生命周期**：用户自主保存/纠错/撤回（201/410），按说话人隔离——原则「个性化只改怎么说」的数据底座 | 记忆在回答中的生效路径（S2+，第 5 线）；画像/taste 设计未定稿 |
| 首页快照与委派/受理链（B1/GS3）贯通 | **G15 四连败**：编译卫生（Q-T87-01）+ 运行期（Q-T65-01）两缺陷都修才能三晚连过 |
| — | 远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、契约正文、断言、登记表、PLAN；未修 Q-T87-01/Q-T65-01（属实现线）
- 未跑远程真站点 live（#251 探针的 live 结论归第 3 线）；未复核 T-84/T-86 作者自验细节（本卡以全量轮覆盖共同基线）
