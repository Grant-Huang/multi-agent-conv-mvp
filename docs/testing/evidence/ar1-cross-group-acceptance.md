# AR-1 跨组场景独立验收（主线 #2：WP7-C 重排 + trace_id 契约 + doc_lint 门 + 编号决策）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+「继续下一个任务」指示（2026-11-24）；**编号决策见 [2026-11-24-acceptance-round-numbering-ar-prefix](../../decisions/2026-11-24-acceptance-round-numbering-ar-prefix.md)（用户确认，本卡为 AR-N 序列第一轮）** |
| 验收人 | 本 worker（未参与 #260/#262/#263/#264/#265/#266 实现） |
| 基线 | `origin/master` `cf8d793`（含 #260～#266；#261 为本线 T-88，已合并） |
| 检出方式 | 隔离 worktree `/tmp/ar1-wt` @ `8f8d3d2` 后随基线抬升切至 `cf8d793`，独立 CARGO_TARGET_DIR |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED**（#262 Odoo 试点评审为 docs，live 归第 3 线） |
| G15 / nightly | 最新仍 11-23 失败（TS6133）；11-24 定时未触发（收稿时点）；修复后今晚应为首个运行期窗口 |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`33b2d05` → `cf8d793`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #260 | WA M4 门禁 / M3-5 进度 docs | docs |
| #262 | M3 Odoo 试点评审提案 | docs（live 结论归第 3 线） |
| #263 | **trace_id 最小版决策 + 契约 R1 拆分 + doc_lint 决策小节前缀匹配（CI 脚本 + 配套测试）+ 围栏登记更新** | `tools/ci` 测试 + 契约 diff |
| #264 | **WP7 方案 C：重启后重排未起跑单任务**（卡号沿用 T-85——与 #254 撞号，AR 决策的又一实证；`orchestrator.rs` L2 已由实现线合入，本卡复验语义） | `restart_semantics` RT-RESTART-07～11 |
| #265/#266 | RT-WAPROD-09 源响应无效→502 合同补录 | `mom_world_prod` |

**编号**：本卡起采用 **AR-N**（决策 2026-11-24-acceptance-round-numbering-ar-prefix）；T-88 为旧序列末轮。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| WP7-C 重排语义（#264） | **VERIFIED / T2** | `restart_semantics` **11 passed**（+5）：QUEUED/CREATED 跨重启**重新排队至终态**（07/08）、RUNNING/VERIFYING 仍 FAILED+原因且不被重驱误标（09）、回滚开关 `resume_queued_on_restart=false` 行为保持（10）、**模板家族不重驱**（11） |
| trace_id 契约 + doc_lint（#263） | **VERIFIED / T2** | doc_lint 测试 **22 passed**（pytest，CI 同命令）；契约 `events-and-trace.md` R1 拆分按决策 `2026-11-24-trace-id-minimal-scope` 落地；围栏登记更新核查 |
| RT-WAPROD-09（#266） | **VERIFIED / T2** | `mom_world_prod` **9 passed**（源响应无效→502，夹具路径） |
| 既有回归 | **VERIFIED / T2** | 合同 8 套件 **54/54**；浏览器 desktop 全量 **31 passed**；E2E-REV 绿 |
| E2E-MOBILE | **FAILED（第 9 轮本地确认）** | Q-T65-01 未修；G15/G6 关闭唯一运行期阻塞 |
| PR CI #260/#262/#263/#264/#266 | **VERIFIED** | 全部 8/8 pass（#265 收稿时 7 pass 余 pending） |
| master push @ `cf8d793` | **FAILED（Q-AR1-01）** | rust cargo test 红（rt_restart_09 CI 抖动）；web/python-* 绿 |

**总体**：WP7-C 重排、trace_id 最小契约、doc_lint 门变更独立复验 **PASS**；无新业务缺陷。

---

## 1. 本地证据（worktree @ `cf8d793`）

```text
$ cargo test --locked -p an-server（8 套件）
desktop_snapshot: 4  desktop_drafts: 4  personal_memory: 5  session_materials: 5
desktop_mem: 9       restart_semantics: 11（RT-RESTART-01～11，07～11 为 WP7-C）
mom_world_prod: 9（RT-WAPROD-01～09）  desktop_c2: 7
合计 54 passed / 0 failed

$ cd tools/ci && uv run --no-project --python 3.13 --with pytest python -m pytest tests/test_doc_lint.py -q
→ 22 passed in 0.42s（与 CI python-tools 同命令）

$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts             → 31 passed (23.1s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → REV ✓；MOBILE ✗（同签名）
```

**并发观察**：首轮浏览器因端口 18789 被另一会话 harness 占用而推迟（未复用对方服务器，`reuseExistingServer:false` 保持证据独立）；对方会话已在 #267 中把该坑补进坑表——与 T-67 的 18787 坑同类，归**测试基建**。

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| QUEUED/CREATED 跨重启重排至终态（开关默认开） | 通过 | RT-RESTART-07/08 |
| RUNNING/VERIFYING 与重排共存不误标 | 通过 | RT-RESTART-09 |
| 回滚开关关闭时行为回退（仍 FAILED+原因） | 通过 | RT-RESTART-10 |
| 模板家族不重驱（join 语义不被单任务重排破坏） | 通过 | RT-RESTART-11 |
| trace_id 最小契约与决策一致 | 通过 | 契约 diff + 决策文件存在性核查 |
| doc_lint 决策小节前缀匹配 | 通过 | 22 tests（正例/反例均覆盖） |
| 源响应无效显式 502（非静默） | 通过 | RT-WAPROD-09 |
| Mock / fixture 可辨 | 通过 | 同既有口径 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#260 / #262 / #263 / #264 / #266：**8/8 pass**；#265：收稿时 7 pass 余 pending（无 fail）。逐个核对，2026-11-24。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `cf8d793`（#266，run [45335684975](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45335684975)） | **FAILED**：`rust / cargo test` 红——`rt_restart_09_side_effect_states_fail_while_queued_states_survive` FAILED（restart_semantics.rs:584，exit 101），fmt/clippy 过；详见 Q-AR1-01 |
| nightly | 11-24 未触发（收稿时点）；今晚为 #259 修复后首个运行期窗口，结论归下轮引用 |

## 4. 问题清单

### Q-AR1-01 — master `cf8d793` rust 红：RT-RESTART-09 CI 抖动（高，master 阻塞）

| 字段 | 内容 |
|---|---|
| 提交 | `cf8d793`（run [45335684975](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45335684975)），失败用例来自 #264 新增代码 |
| 现象 | CI `cargo test`：`rt_restart_09_side_effect_states_fail_while_queued_states_survive` FAILED（restart_semantics.rs:584，panic），同套件其余 10 个通过；fmt/clippy 绿 |
| 本地 | 同提交 `rt_restart_09` 单独复跑 **10/10 通过**（0.01–0.02s）；全量套件 11/11——本地未复现 |
| 判定 | **CI 负载下的时序敏感抖动**：RT-RESTART-09 断言「RUNNING/VERIFYING 失败标记」与「QUEUED 重排」共存的窗口，CI 并行/慢机易踩重排先于断言完成的竞态；非确定性失败 |
| 期望 | 失败标记与重排的先后在测试内确定性编排（如显式等待终态事件而非固定时序），或实现保证标记先于重排 |
| 建议归属 | **第 4 线**（WP7-C 实现 + 新测试作者链）；master 当前红，阻塞后续合并可信度，优先修 |
| 备注 | 本线不接受「本地过=完成」：CI 失败记录即证据；修复后需 CI 连续多轮绿再关 |

### Q-T65-01（延续，第 9 轮本地确认）— E2E-MOBILE 未修复

`cf8d793` 同签名失败。今晚 nightly 若失败将首次可精确归因于此（编译关已除）。

### 编号竞态（延续记录）

T-85 双占实锤：#254（本线验收）与 #264（第 4 线实现卡 WP7-C）。AR-N 决策已生效，本线后续轮无此风险；实现线间竞争仍开放。

### 观察

- 端口 18789 并发 harness 冲突：本卡与第 3 线 #267 双向独立踩坑；`reuseExistingServer:false` 保持证据独立，建议各线跑套件前 `lsof -i :18789`（归测试基建，#267 落坑表）。

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **重启语义补全**：QUEUED 重排 + 回滚开关 + 模板豁免——服务生命周期三态（中断/重排/豁免）闭环 | — |
| **可追踪性底座**：trace_id 最小契约入库，doc_lint 门左移 | trace_id 全链贯通（观测面，后续卡） |
| 合同面继续加密（RT-WAPROD-09） | **Q-T65-01 MOBILE**：G15/G6 最后一公里 |
| — | 远程真站点 live（Odoo 试点待批，#262）；真设备路径；分层记忆 REQ-03/04；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、契约正文、断言、登记表、PLAN；未修 E2E-MOBILE；未跑远程真站点 live
- #262 Odoo 试点为提案 docs，批准与 live 验证归用户/第 3 线；治理线论文约束决策不复核
