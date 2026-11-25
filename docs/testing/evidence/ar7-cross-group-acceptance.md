# AR-7 跨组场景独立验收（主线 #2：审查修复 + T-96 + 审查发现跟踪）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「297 已合，你看下一步如何继续」指示（2026-11-25） |
| 验收人 | 本 worker（未参与 #289/#298/#299 实现） |
| 基线 | `origin/master` `03373f2`（#298 代码审查、#299 受理幂等载荷修复（R-20261121-M5）、#289 T-96 助理已读持久化） |
| 检出方式 | 隔离 worktree `/tmp/ar6d-wt` @ `03373f2`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 定时未触发（收稿时点 17:3xZ，预计 ~20:45Z）；预期形态：**仅 ADREAD 红**（AR-5 已证其余 92 绿） |
| 证据等级 | 本地/PR CI/master push 为 T2 |

## 0. 本轮范围（`81445e7` → `03373f2`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #299 | 修复：桌面受理幂等键校验完整载荷（R-20261121-M5，来自 #298 审查） | 新套件 `desktop_accept_payload` + desktop_accept 回归 |
| #289 | T-96 助理消息已读持久化（M3 夹具欠账收口；store +131） | `desktop_read`（扩至 6） |
| #298 | 代码审查 2026-11-25（cf8d793..4a810eb，27 PR；9 findings 入账） | 发现跟踪（§4） |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| 受理幂等载荷修复（#299） | **VERIFIED / T2** | `desktop_accept_payload` 1 passed（完整载荷校验）+ desktop_accept 4 passed 回归 |
| T-96 已读持久化（#289） | **VERIFIED / T2** | `desktop_read` **6 passed**（含持久化面；store 迁移面核查 MOCK_REGISTRY 已同步） |
| 浏览器回归 | **VERIFIED / T2** | desktop **32** + REV/**MOBILE 2** + **agents 11 全绿（AGFIRST 绿、零级联）** |
| 合同合计（本轮所跑） | — | **21 passed / 0 failed** |
| PR CI #299/#289/#298 | **VERIFIED** | 全 pass |
| **master push @ `03373f2`** | **VERIFIED** | run 45663787617 **success**（8m23s）——**rust 连续第三推绿**（467d6e0/6fbc83c/03373f3），Q-AR2-04 的复跑机制建议效果待样本继续累积 |

**总体**：修复与特性独立复验 **PASS**；审查发现（M1/M2/ADREAD/FIX-A）全部在账跟踪；无新增未归属缺陷。

---

## 1. 本地证据（worktree @ `03373f2`）

```text
$ cargo build --locked → Finished
$ cargo test --locked -p an-server --test desktop_accept_payload --test desktop_read \
    --test desktop_accept --test judge_materials --test session_materials
desktop_accept_payload: 1（#299 完整载荷校验）  desktop_accept: 4（回归）
desktop_read: 6（T-96 持久化）  judge_materials: 5  session_materials: 5
合计 21 passed / 0 failed

$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts  → 32 passed
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
$ npx playwright test e2e/agents.spec.ts   → 11 passed（AGFIRST 绿、零级联——第 1 线修复在最终基线生效）
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理幂等按完整载荷校验（非仅键） | 通过 | RT 系列（desktop_accept_payload，#299 决策文档入库） |
| 已读状态跨重启持久 | 通过 | desktop_read 6（T-96，store +131 无新表冲突） |
| S4 判断路径资料接线回归 | 通过 | judge_materials 5 |
| AGFIRST 修复在最终基线生效、无级联 | 通过 | agents 11/11 |
| MOCK_REGISTRY / features.json 同步 | 通过 | #289/#299 diff 核查 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#299 / #289 / #298：全 pass（gh pr checks 逐个核对）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `03373f2`（#289，run [45663787617](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45663787617)） | **success**（8m23s）——rust 连续第三推绿（Q-AR2-04 样本续记） |
| nightly 11-25 | 未触发（收稿时点）；预期仅 ADREAD 红 |

## 4. 审查发现跟踪（#298 在账项，本线记录）

| ID | 级别 | 状态 | 本线动作 |
|---|---|---|---|
| R-20261125-M1（S4 门关闭仍注入，已复现） | P2 | **open**（无修复 PR） | judge/session 套件绿但不含门关闭断言（正是审查论点）；修复落地后复验补门关闭用例 |
| R-20261125-M2（风格编辑绕过预览门） | P2 | **open** | 修复落地后复验 5.2b 预览门语义 |
| E2E-ADREAD（Q-AR5-01） | 中 | open（第 1 线桌面组，双壳复现） | 修复落地后 AR-6 第二段（ops 复验+归属定论） |
| FIX-A（WA 写入口批准登记，3×P1 open） | P1 | 未开工；评审问「M4 生产写前必须先合 FIX-A？」推荐是——**归用户决策** | 修复落地后 WA 写面复验 |
| R-20261124-H4（store 级回归测试补锁） | 建议 | 评审推荐补——归实现线 | 补测试后正常复验 |
| UX-03（#300）/ 派工机制（#311 DRAFT） | — | 在途 | 合入后进验收队列 |

## 5. 相对目标的差距（主轴对照——宪章 v1.4 口径）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 受理幂等加固（载荷级校验）+ 已读持久化收口（M3 欠账清） | **M1/M2 两个 P2**（审查已复现，修复未进场） |
| agents 串行文件全绿（AGFIRST 修复生效、零级联） | ADREAD（唯一浏览器剩红） |
| master rust 连续三推绿 | master rust 稳定性持续样本（Q-AR2-04） |
| 审查→派卡→修复→验收的闭环在本轮走通（#299 即审查产物） | FIX-A（M4 前置）；M4 生产写；远程真站点 live；真设备路径 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未修 M1/M2/ADREAD（归实现线）；未跑远程真站点
- #298 的两个「需要人决定」事项（FIX-A 时点、H4 补测试）归用户，本线只转记
