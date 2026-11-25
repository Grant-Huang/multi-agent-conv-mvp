# AR-4 跨组场景独立验收（主线 #2：审查修复链 FIX-B/C/H3/H4 复验）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「继续你主线的下一卡」指示（2026-11-25） |
| 验收人 | 本 worker（未参与 #281/#285/#286 实现） |
| 基线 | `origin/master` `467d6e0`（含 #281/#284/#285/#286；#283 为本线 AR-3） |
| 检出方式 | 隔离 worktree `/tmp/ar4-wt` @ `467d6e0`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点）；预期形态：MOBILE 绿（新壳）+ 第 3 线 25 例红 + AGFIRST 红 |
| 证据等级 | 本地/PR CI/master push 为 T2 |

## 0. 本轮范围（`e28a5a5` → `467d6e0`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #286 | **T-95 FIX-C/H4：夹具门控与事件去重**（生产缺省 `desktop.test_fixtures=false` → 夹具端点 404；每次种子独立游标/事件不重复） | 新套件 `desktop_fixture_gate`（RT-FIXC-01/02）+ desktop.spec |
| #285 | **T-93 FIX-B/H3：模板汇合只建一次**（RT-TPL-17：400 父任务并发收尾） | `template_orch` rt_tpl_17 |
| #281 | 第 5 线 USTY 测试时钟确定性（微秒坍缩防护） | `personal_memory` |
| #284 | GR-4 nightly 评审简报（登记表例外 11-28 到期评审用） | docs（存目） |

**勘误（AR-4 过程内自纠）**：初读 diff 曾误判 RT-TPL-17「仅登记未实现」——实为 stat 输出被截断所致；核实 `template_orch.rs:501` 已实现并实测通过。如实记录以正流程。

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| 夹具门控（#286，审查 H4） | **VERIFIED / T2** | `desktop_fixture_gate` **2 passed**：连续种子独立会话/游标不重复、二次订阅 200；**生产缺省下三个夹具端点全部 404**——「生产界面去测试控制」合同化收口 |
| 模板汇合唯一性（#285，审查 H3） | **VERIFIED / T2** | **rt_tpl_17 passed（9.75s，400 父任务循环）** + template_orch 全套 8/8 |
| USTY 时钟确定性（#281） | **VERIFIED / T2** | `personal_memory` **10 passed**（含微秒坍缩防护用例） |
| 回归 | **VERIFIED / T2** | desktop.spec **32 passed**（+1 门控浏览器用例）；REV + MOBILE 2 passed（MOBILE 修复后第三次确认）；session_materials 5 / restart 11 / desktop_mem 9 / prod 9 / c2 7 全绿 |
| 合同合计（本轮所跑） | — | **61 passed / 0 failed** |
| PR CI #281/#285/#286 | **VERIFIED** | 全部 8/8 pass |
| **master push @ `467d6e0`** | **VERIFIED** | run 45519350458 **success**（15m10s）——**master rust CI 两连败后首次转绿** |

**总体**：审查修复链（FIX-B/C/H3/H4）独立复验 **PASS**；master CI 转绿；无新业务缺陷。

---

## 1. 本地证据（worktree @ `467d6e0`）

```text
$ cargo build --locked → Finished
$ cargo test --locked -p an-server --test desktop_fixture_gate --test personal_memory \
    --test session_materials --test restart_semantics --test desktop_mem \
    --test mom_world_prod
desktop_fixture_gate: 2（RT-FIXC-01/02）  personal_memory: 10（USTY 防护）
session_materials: 5  restart_semantics: 11  desktop_mem: 9  mom_world_prod: 9
$ cargo test --locked -p an-runtime --test template_orch → 8 passed（rt_tpl_17 9.75s）
$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts            → 32 passed (39.6s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 生产缺省夹具不可达（404，非静默） | 通过 | RT-FIXC-02（`test_fixtures=false`） |
| 夹具种子幂等边界：独立会话、游标/事件编号不重复 | 通过 | RT-FIXC-01（H4 主断言） |
| 汇合只建一次（400 并发收尾） | 通过 | rt_tpl_17（#285/H3） |
| 风格判定时钟确定性（微秒坍缩防护） | 通过 | personal_memory +7（#281） |
| 门控不破坏新壳与 MOBILE | 通过 | desktop 32 + MOBILE passed |
| Mock / fixture 可辨 | 通过 | MOCK_REGISTRY 同步更新（#286） |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#281 / #285 / #286：全部 **8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `467d6e0`（#286，run [45519350458](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45519350458)） | **success**（15m10s）——cf8d793/fa9970d 两连败后首次转绿（Q-AR2-04 观察续记：本轮 rust 稳定，是否为复跑/竞态自愈待后续样本） |
| nightly 11-25 | 未触发（收稿时点 00:36Z+）；11-24 nightly 已录 AR-2 |

## 4. 问题清单

### 延续（无新增业务缺陷）

| ID | 状态 |
|---|---|
| Q-AR3-01 / 第 3 线 25 例适配 | 仍未合入（收敛阻塞；AR-5 全量收敛前置） |
| Q-AR2-02 / AGFIRST | 本轮未重跑 agents 套件（第 1 线无新修复；AR-5 复验） |
| Q-AR2-04 / master rust 稳定性 | 本推转绿，观察续记 |
| GR-4 登记表例外到期（2026-11-28） | 评审简报已入库（#284 `GR4_NIGHTLY_REVIEW_2026-11-28.md`），归治理线/用户 |

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **生产界面测试控制清零**：夹具端点生产 404 合同化——宪章原则 8「生产不静默回退/可辨」的测试面收口 | — |
| **模板并发汇合正确性**：400 任务循环实证「恰好 1 汇合 1 交付」 | rt_tpl_17 单测较重（9.75s），可评估入夜间套件而非 PR 门（工程取舍，非本线决定） |
| master CI 转绿 | 第 3 线 25 例适配 + AGFIRST（AR-5 全量收敛前置）；rust 稳定性观察 |
| — | 远程真站点 live；真设备路径；M4 生产写；GR-4 到期评审（11-28） |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替第 3 线适配；未跑远程真站点
- GR-4 评审结论归治理线/用户，本卡只存目简报存在性
