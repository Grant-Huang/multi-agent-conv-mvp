# T-67 工业业务闭环链路独立复验（WA M3 桩路径 + M4 门禁 + DES/MOM/运营处置回归）

| 项目 | 内容 |
|---|---|
| 任务 | T-67 工业业务闭环独立复验（任务卡未给 ID，此为建议值，**待用户确认**） |
| 主线 | **第 3 线：工业业务闭环**（DES、MOM、运营处置与 World Adapter；业务来源核验）。用户 2026-11-23 明确主线编号；桌面入口链只记跨组观察，不作为本线验收对象 |
| 授权来源 | 五线分工决定（2026-11-22 用户确认）+ 集成计划第 3 线职责 + 角色任务卡 + T-60/63/65/66 先例（#226 已合入） |
| 验收人 | 本 worker（未参与 #224～#236 实现） |
| 基线 | `origin/master` `2c59a3a`（含 #224～#236）。**注**：#237/#238 于本轮进行中合入（`ba9a7c7`，2026-11-23 17:45Z，属第 1/4/5 线交付），不在本轮基线，留待下一轮复验 |
| 检出方式 | 隔离 worktree `/tmp/t67-wt` @ `2c59a3a`（主树有并行会话在途，本卡未触碰） |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；生产世界 = **stub 服务层**（`with_test_controls` 默认关）；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-20/21/22 三晚失败（E2E-MOBILE，Q-T65-01）；11-23 定时截至本卡提交未触发（见 §3）。状态**不改**（三晚连过条件未满足） |
| 证据等级 | 本节所列本地/CI 均为 T2（真实执行、通过/失败数可核对）；远程真站点 NOT_VERIFIED |

## 0. 范围（第 3 线）

T-66（`47d8557`）之后新合入的工业线内容 + 既有链回归：

| # | 复验对象 | 来源 | 结论位置 |
|---|---|---|---|
| 1 | **M3-4 转绿**：生产只读浏览器 E2E-WAPROD×4（T-66 时为计划内 Red） | #224 | §1.2 / §1.3 |
| 2 | **M3 全量回归**：`wa-m3-suite`（嵌套 M2/M1/M0；cargo RT-WADES/DES/DESC/WA + Playwright） | #228 后基线 | §1.3 |
| 3 | **M4-0 门禁保持**：`WRITE_DISABLED` 不弱化；准入清单 G1～G8 与台账一致 | #230 | §1.4 |
| 4 | **WP4 汇合修复回归**（#227 改 `orchestrator.rs`，高风险文件）：RT-TPL + E2E-TPL | #227 | §1.1 / 跨组观察 |
| 5 | **工业六 spec 独立复验**：运营处置 / MOM / WA 工作台 / DES 实验 / 远程 DES / 频道集成 | 既有链 | §1.2 |
| 6 | 来源核验：provenance、回执/水位、版本绑定、Mock 标记、可追踪性 | — | §1.5 |

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 生产只读浏览器 E2E-WAPROD×4 | **VERIFIED / T2** | **4 passed——M3-4 转绿确认**（T-66 计划内 Red 已消除；独立跑 + 套件内双轮均绿） |
| `wa-m3-suite` 全量（M3+M2+M1+M0） | **VERIFIED / T2** | 干净第 3 轮 exit 0：cargo 55 passed、Playwright 19 passed；第 2 轮 M0 段 des-experiment 4 条超时 → Q-T67-02（间歇，测试基建） |
| 工业六 spec 独立复验 | **VERIFIED / T2** | 21 passed / 0 failed（ops / mom / mom-workbench / des-experiment / wa-des-remote / wa-integration） |
| 生产写合同 RT-WADES（12）+ 读 mom_world_des（12）+ RT-DES-08～11（4）+ RT-DESC 连接器（6） | **VERIFIED / T2** | 套件内全绿；含丢回执恢复、关联号恢复、幂等、终态保护、工厂重启不误投 |
| 生产只读合同 RT-WAPROD-01～06 | **VERIFIED / T2** | 6 passed；**06 写关闭保持**（M4 门禁 G8） |
| M4-0 门禁静态核对 | **VERIFIED / T2** | `mom_world_prod.rs:661/690` Propose/Execute 均 403 `WRITE_DISABLED`；WAM-04/05/07 写侧「未开」与准入 G5/G6 一致；G1～G7 维持未满足、禁止写实现 |
| WP4 模板链（#227 orchestrator 修复后） | **VERIFIED / T2** | `template_orch` 7 passed；E2E-TPL passed |
| 远程真站点 live | **NOT_VERIFIED** | M3 为 stub；真站点凭据/egress 禁止且未配（G1～G3 未满足，M4-1 待授权） |

**总体**：工业闭环链在 `2c59a3a` 独立复验 **PASS**；M3-4 计划内 Red 转绿；M4 写门禁无弱化；无新增产品缺陷。两项非产品问题挂账（Q-T67-01 文档同步、Q-T67-02 测试基建 flake）。

---

## 1. 本地证据（worktree @ `2c59a3a`，全新 target 构建）

### 1.1 合同层（7 套件 47/47）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test template_orch
  test result: ok. 7 passed; 0 failed          ← #227 汇合先交付再 COMPLETED 回归
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_mem --test mom_world_prod --test desktop_c2 \
    --test desktop_accept --test desktop_replay --test mom_world_des_write
desktop_accept:      4 passed    desktop_c2:        7 passed
desktop_mem:         6 passed    desktop_replay:    5 passed
mom_world_des_write: 12 passed   mom_world_prod:    6 passed
合计 47 passed / 0 failed
```

新覆盖：RT-WAPROD-06（生产 Propose/Execute 拒绝、Mock 通路不受影响）；RT-WADES-09～19（提案绑定 plan revision、执行幂等、取消、丢回执恢复、按关联号恢复、工厂重启不误投、干预改变未来事件、部分回执、同步重试）。`desktop_mem/accept/c2/replay` 为环境健全性回归（主树并行线交付共用底座）。

### 1.2 浏览器层（工业线）

```text
$ npx playwright test e2e/wa-prod-read.spec.ts
✓ E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY   4 passed（M3-4 转绿）
$ npx playwright test e2e/ops.spec.ts e2e/mom.spec.ts e2e/mom-workbench.spec.ts \
    e2e/des-experiment.spec.ts e2e/wa-des-remote.spec.ts e2e/wa-integration.spec.ts
21 passed / 0 failed
  （E2E-OPSAPPROVE/OPSCHANNEL/OPSDESK/OPSDRAWER/OPSNOISE/OPSOFF/ADREAD、
    E2E-MOM/MOMDYNAMIC/COMPLAINT、E2E-WA-01/WABIND、E2E-DES*、
    E2E-WADES/WAREAD/WAOBS、E2E-WACHANNEL）
$ npx playwright test e2e/wa-des-write.spec.ts
✓ E2E-WAEXEC / WARECV / WADYN / WALOOP   4 passed
```

### 1.3 `wa-m3-suite` 三轮记录（flake 定性）

| 轮 | 条件 | 结果 |
|---|---|---|
| 1 | 与前台 B 组 harness **端口冲突（我方操作失误）** | E2E-WAPROD failed；**作废未采信** |
| 2 | 干净、无并发 | m3/m2/m1 段全绿；**M0 段 des-experiment 4 条 30.1s 超时**（Q-T67-02） |
| 3 | 干净、无并发 | **exit 0 全绿**：cargo 55 passed、Playwright 19 passed |

Q-T67-02 失败形态（轮 2）：首批 4 用例（DESTRIAL/DESSTART/DESOPS/DESPAUSE）均在首页等待导航链接超时，随后 DESISO（2.2s）/DESLOOP（1.0s）**同一导航通过**——服务冷启动/负载下首页数据未就绪 ~2 分钟后自行恢复；独立跑同批用例 7/7 绿。定性：**间歇、顺序/负载敏感，非产品断言失败**。

### 1.4 M4-0 门禁静态核对（写实现前置准入保持）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 生产写保持关闭 | 通过 | `crates/server/src/mom_world_prod.rs:661/690`：Propose/Execute 双路由 403 `WRITE_DISABLED`，文案指向 Mock/DES |
| 能力发现不谎称可写 | 通过 | RT-WAPROD-06 + E2E-WAPROD（写入口关闭断言） |
| 台账与准入一致 | 通过 | WAM-04/05/07 标注「写侧未开」+ G5/G6 引用；[WA_M4_TASKS](../../mom/WA_M4_TASKS.md) §1 G1～G7 未满足、G8 满足须保持 |
| 决策在库 | 通过 | `docs/decisions/2026-11-23-wa-m4-write-entry-gates.md`（未授权卡不得接 MES 真写） |
| M4-1 未被越权开工 | 通过 | 仓库无候选站只读实现/凭据接入痕迹（本卡亦不开） |

### 1.5 来源核验核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 生产 Bind 权威源与协议版本 | 通过 | RT-WAPROD-01（`world_id`/`provenance=production`/`protocol_version`） |
| 观测回执含来源与水位 | 通过 | RT-WAPROD-02（`source`/`as_of`/`protocol_version`） |
| 跨站拒绝不泄露 | 通过 | RT-WAPROD-03 + E2E-WAPISO |
| 断线续读无漏无重 | 通过 | RT-WAPROD-04 + RT-WADES-15（关联号恢复）+ E2E-WAPREAD/WAREAD |
| 延迟源 `as_of` 可辨 | 通过 | RT-WAPROD-05 + E2E-WAPDELAY |
| 丢回执 / 滞后恢复语义 | 通过 | RT-WADES-12/15/17（丢回执、关联号、重启不误投）+ E2E-WARECV |
| 执行幂等与终态保护 | 通过 | RT-WADES-10/16（幂等、部分回执） |
| 仿真/生产/标注可辨 | 通过 | E2E-WAPROD（`provenance=production`）；DES 内容带仿真标注；无标准答案泄漏（RT-WADES-18 + E2E-WAOBS/DESOPS） |
| Mock 标记 | 通过 | 生产=stub（`with_test_controls` 默认关）；`world_mock` 门控挂载；WAM 台账与实际路径一致；远程真站不宣称 |
| 审批链路（运营处置） | 通过 | E2E-OPSAPPROVE/DESOPS/WACHANNEL（桌面内批准、频道卡、跨运行拒绝 DESISO） |

---

## 2. PR CI 证据（#224～#236）

| PR | 内容 | PR 检查 | master push |
|---|---|---|---|
| #224 | M3-4 浏览器生产读 | pass | **success**（45135094470） |
| #225 | T-74 mic 方案（docs） | 8/8 pass | cancelled（紧随 #226 合入覆盖） |
| #226 | T-65/T-66 验收记录（docs） | pass | **success**（45135424522） |
| #227 | WP4 汇合先交付（orchestrator.rs） | 8/8 pass | cancelled（由 #229 的 success 覆盖同一累计树） |
| #228 | M3-5 准出（docs） | 8/8 pass | cancelled（同上） |
| #229 | T-75 真机 mic | pass | **success**（45137412345） |
| #230 | M4-0 写门禁冻结（docs） | pass | **success**（45213969340） |
| #231 | E2E-CODEFLOW | 8/8 pass | cancelled（由 #233 success 覆盖） |
| #232 | T-76 远端 ASR | 8/8 pass | cancelled（同上） |
| #233 | 修复日报（docs） | pass | **success**（45214805655） |
| #234 | T-77 双设备 | pass | **success**（45221893700） |
| #235 | 首页 S1 委派切片 | pass | **success**（45226099841） |
| #236 | T-78 持久化方案（docs） | pass | **success**（45226186879） |

被取消的 master push 均为数秒内被下一次合入取代（GitHub concurrency），其累计树由紧随的 success 运行覆盖；PR 分支检查 8/8 pass 已单独核对。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| nightly（11-23） | 截至本卡提交未触发；最新仍为 11-22 失败（run 45052408864，Q-T65-01）。**G15「三晚连过」条件不满足，状态不改**；若 11-23/24/25 连过才开始重新计数。今晚定时将跑在 `ba9a7c7`（含 #237/#238），与本轮本地基线 `2c59a3a` 差异不影响工业链断言 |
| master push | 见 §2；本轮收尾时 `2c59a3a` 之前最后一次 success 为 #236（45226186879） |

## 4. 问题清单

### Q-T67-01 — M4-0 状态同步滞后（轻微，L0）

| 字段 | 内容 |
|---|---|
| 提交 | `2c59a3a` |
| 现象 | ① `docs/PLAN.md` M4-0 仍为 `- [ ]` 且注「本 PR」（#230 已合入、证据 `wa-m4-0.md` 在库）；② `mom/WA_M4_TASKS.md` §0 M4-0 状态仍为「进行中（本 PR）」 |
| 影响 | 文档链 T1 一致性；不阻塞业务 |
| 处置 | 本卡 PR 顺手做事实性同步（勾选 + 引 #230），不改任何断言与计划内容 |

### Q-T67-02 — des-experiment 套件内冷启动超时（间歇，测试基建）

| 字段 | 内容 |
|---|---|
| 提交 | `2c59a3a` |
| 现象 | `wa-m3-suite` 第 2 轮 M0 段：E2E-DESTRIAL/DESSTART/DESOPS/DESPAUSE 各 30.1s 超时于首页导航链接；随后 DESISO/DESLOOP 同导航 2.2s/1.0s 通过；独立跑 7/7 绿；第 3 轮全绿 |
| 根因候选 | harness 就绪门只等 `/api/health`，未等首页数据接口；冷启动/负载下首页渲染滞后 |
| 影响 | 套件可信度（间歇红），非产品缺陷 |
| 建议归属 | 本线（DES 实验入口测试基建）；建议夹具等待首页数据就绪或首用例前暖机，不改断言 |

### 跨组观察（非本线验收对象，转对应组）

| 观察 | 现象 | 归属 |
|---|---|---|
| E2E-ADD01-02 批量顺序依赖 | 批量跑 `fill` 后值被重置（1/4）；隔离 3/3 通过；二次批量因我方端口冲突未取得数据 | 第 1 线桌面组（双 context 用例间服务端语音会话残留/重渲染竞态） |
| E2E-MOBILE（Q-T65-01 延续） | `2c59a3a` 复跑仍 1 failed | 桌面/前端组；G15 状态不变 |

### 遗留

| ID | 状态 |
|---|---|
| Q-T60-02（远程真 DES/站点 live） | 仍 NOT_VERIFIED；真站点读属 M4-1（待授权 + 候选站就绪） |
| M4-1 候选站只读 | **待授权**，G1 未满足；本卡未开工 |
| #237/#238 复验 | 基线已前进（`ba9a7c7`），留待下一轮 |

## 5. 相对目标的差距（主轴对照）

主轴 = 桌面唯一入口上的「观察 → 提方案 → 人批准 → 执行 → 核验」闭环（`docs/PRODUCT.md` 第 1、3 节），工业侧按 World Adapter 契约作用到仿真工厂。

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 生产只读 M3 全链合同+浏览器双绿（M3-4 转绿收口） | 远程真站点 live 未验（M4-1 待授权；G1～G3 未满足） |
| DES 写闭环（授权、幂等、丢回执、恢复、终态保护、重启不误投）合同+浏览器全绿 | 生产写 M4 未开工（准入 G1～G7 未满足，门禁保持正确） |
| 运营处置链（频道批准、桌面内批准、跨运行隔离、噪声合并、离线）21/21 绿 | des-experiment 套件内间歇冷启动超时（Q-T67-02，测试基建） |
| WP4 汇合修复（高风险文件 #227）回归绿 | — |
| M4 写门禁（G8）在代码、台账、决策三层一致 | — |

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务、契约正文、CASES/features.json 断言、GUARDRAIL_REGISTRY
- 未开 M4-1；未接真站点凭据/egress；未宣称生产写
- 未深复验桌面入口链（第 1/2 线范围，只记跨组观察）
- 未定位 E2E-MOBILE / ADD01-02 根因（实现组）
- 验收记录 PR 提交前按流程自检；任务 ID「T-67」待用户确认
