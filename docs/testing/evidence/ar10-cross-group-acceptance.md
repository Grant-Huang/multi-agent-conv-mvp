# AR-10 跨组场景独立验收（主线 #2：按严重度开工——#301/#325 修复复验 + 在账确认）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「按严重度顺序开工」指示（2026-11-26）；#312/#338 派工约定（开工前查 line:2 Issue、severity 优先、high 3 天/medium 7 天、巡检 4h） |
| 验收人 | 本 worker（未参与 #332/#334/#335/#336/#337/#338 实现） |
| 基线 | `origin/master` `6d6a67b`（#332 S5 第二批、#334 ACP 探测修复、#335 conv_lock 修复、#336 nightly 确定性方案 docs、#337 复探证据、#338 派工时限） |
| 检出方式 | 隔离 worktree `/tmp/ar10-wt` @ `6d6a67b`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-26（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-25 nightly 已采（AR-9：2 红=ADREAD+UX05，MOBILE/AGFIRST 首绿）；11-26 未触发（收稿时点 06:4xZ） |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 按严重度的开工队列（本线在账）

| 序 | 项 | 严重度 | 本卡处置 |
|---|---|---|---|
| 1 | #325 ACP 探测矛盾（第 5 线，high） | high | **修复复验 ✓**（#334/8579813） |
| 2 | #301 conv_lock 抖动（第 4 线，Q-AR2-04 样本之一） | high（master 红） | **修复复验 ✓**（#335/c798dd9，Issue 已 CLOSED） |
| 3 | S5 第二批（#332） | — | 复验 ✓ |
| 4 | ADREAD（#333，第 1 线，high） | high | **修复未进场，Issue 仍 OPEN**——AR-6 第二段继续等 |
| 5 | M1/M2（#298，P2） | P2 | 修复未进场，open 确认 |
| 6 | #336 nightly 触发确定性方案 | — | docs 待批（B 选项），归用户 |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| conv_lock 修复（#335，Issue #301） | **VERIFIED / T2** | 修复后单测 **10/10** + conv_lock 全模块 3 passed；测试判据改为「等待分支语义」（消除固定时序假设——正是 Q-AR2-04 建议的方向）；**Issue #301 已 CLOSED**——派工生命周期首次完整走通 |
| ACP 探测修复（#334，Issue #325，high） | **VERIFIED / T2** | an-agents 全 crate **82 passed / 0 failed**（含新增 acp_probe 42 行 + i325_diagnose 57 行测试）；「体检找到、检测 not_installed」矛盾修复（探测/执行前重新解析命令） |
| S5 第二批（#332） | **VERIFIED / T2** | `route_materials` 3 passed |
| mcp 套件回归 | **VERIFIED / T2** | 10 passed（含 expired_http_session——**未修复的两个抖动样本之一，本地绿维持**） |
| rt_restart_09（Q-AR2-04 样本之二） | 本地绿维持 | 未修复确认（CI 抖动观察续） |
| 在账确认 | — | **ADREAD Issue #333 OPEN（第 1 线未动）**；M1/M2 open（无修复 PR）；#336 方案待批 |
| master push @ `6d6a67b` | in_progress → 终态回填 §3 | run 45768847545 |

**总体**：severity 队列前两项（high）修复独立复验 **PASS**；派工机制全链路（dispatch→认领→修复→关闭→复验）走通；ADREAD/M1/M2 仍在实现线队列。

---

## 1. 本地证据（worktree @ `6d6a67b`）

```text
$ cargo build --locked --bin agentnexus --bin an-mock-acp → Finished
$ for i in $(seq 1 10); do cargo test -p an-runtime conv_lock::tests::entries_are_cleaned_up_after_use; done
→ 10/10 passed（0.03s/次）
$ cargo test -p an-runtime conv_lock → 3 passed（全模块）
$ cargo test -p an-agents → 82 passed / 0 failed（含 acp_probe + i325_diagnose 新测试）
$ cargo test -p an-agents --test mcp → 10 passed
$ cargo test -p an-server --test route_materials → 3 passed
$ cargo test -p an-server --test restart_semantics rt_restart_09 → 1 passed（未修复样本，本地绿维持）
```

### 核对清单

| 检查点 | 结果 | 依据 |
|---|---|---|
| conv_lock 判据改为等待分支语义（消除固定时序） | 通过 | c798dd9 diff + 10/10 复跑 |
| ACP 探测/执行命令一致（矛盾消除） | 通过 | acp_probe + i325_diagnose 新测试 |
| S5 第二批不回归 | 通过 | route_materials 3 |
| 未修复样本（mcp/rt_restart_09）本地绿维持 | 通过 | 观察续记 |
| 派工生命周期闭环 | 通过 | #301 dispatch→修复→CLOSED→本线复验 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#332 / #334 / #335 / #336 / #337 / #338：全 pass（gh pr checks 逐个核对）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `6d6a67b`（#338，run [45768847545](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45768847545)） | 记录时 **in_progress**；终态回填：（待回填） |
| nightly 11-26 | 未触发（收稿时点）；ADREAD 未修，预计仍 2 红形态（ADREAD+UX05 抖动候选） |

## 4. 问题清单

### 在账确认（severity 序）

| ID | 状态 |
|---|---|
| #333 ADREAD（high，第 1 线） | **OPEN 未动**——派工时限 high 3 天（#338）起算核点；超时由 4h 巡检按 #312 规则处理 |
| M1/M2（P2，第 5 线 S4/S2b） | open（修复未进场） |
| rt_restart_09 / mcp expired_http_session（Q-AR2-04 余二样本） | conv_lock 同类；**#335 的判据修复范式可复制**（等待分支语义）——建议第 4 线同法处置 |
| #336 nightly 触发确定性（watchdog + workflow_dispatch） | docs 待批——**归用户**（GR-4 简报 B 选项） |

## 5. 相对目标的差距（主轴对照——宪章 v1.4 口径）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **rust 抖动三样本之一修复并关闭**（#301），判据修复范式可复制到余二样本 | 余二样本（mcp/rt_restart_09）同法修复待办 |
| **派工生命周期全链路走通**（dispatch→认领→修复→关闭→独立复验） | ADREAD 超时未动（high 3 天时限核点临近） |
| ACP 探测矛盾修复（第 5 线 high 项清账） | M1/M2；ADREAD；GR-4 评审（11-28，今晚形态+本卡修复样本入证词） |
| — | 远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替第 1/3/5 线修复其在账项
- #336 方案批准归用户；nightly 分诊归值班线
