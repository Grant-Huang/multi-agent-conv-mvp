# T-82 跨组场景独立验收（主线 #2：会话资料注入 S1 + 记忆验收探针）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡（独立验收角色）+「继续」指示（2026-11-23） |
| 验收人 | 本 worker（未参与 #241/#243/#244 实现） |
| 基线 | `origin/master` `c69c9e5`（含 #241～#244；#242 为本线 T-80，已合并） |
| 检出方式 | 隔离 worktree `/tmp/t82-wt` @ `c69c9e5`，独立 CARGO_TARGET_DIR 全新构建 |
| 日期 | 2026-11-23（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 最新仍 11-22 失败；**11-23 定时收稿时点仍未触发**（第 3 线程序化监视至 ~21:50Z，结论回填其 PR #240）；登记表不改 |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`ba9a7c7` → `c69c9e5`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #241 | T-81 记忆持久化验收探针补录（第 1 线） | `desktop_mem` RT-ADMEM-08/09 |
| #243 | convem S1 会话资料注入（第 5 线；新模块 `session_materials.rs` + `conversation.rs` 回答路径 + `store.rs` 只读链接查询；**动 L2 文件 store.rs**） | `session_materials` RT-SMAT 合同 |
| #244 | M3 live 复审文档 | docs |

注：任务号 T-81 已被 #241 占用，本卡取 **T-82**。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 会话资料注入合同（RT-SMAT，#243） | **VERIFIED / T2** | `session_materials` **5 passed**（注入块=数据非指令、撤回链接排除、预算裁剪、超时不阻塞回答） |
| 记忆验收探针（RT-ADMEM-08/09，#241） | **VERIFIED / T2** | `desktop_mem` **9 passed**（+跨库隔离、软删重启存活） |
| 既有回归（Accept/C2/Replay/prod 门禁） | **VERIFIED / T2** | 4+7+5+6 全绿 |
| 浏览器抽查 | **VERIFIED / T2** | ADMEM/ADMEMW/ADRESUME 3 passed；E2E-REV 1 passed |
| E2E-MOBILE | **FAILED（第 5 轮确认）** | Q-T65-01 未修，G15 唯一阻塞不变 |
| PR CI #241/#243/#244 | **VERIFIED** | 全部 8/8 pass |
| master push @ `c69c9e5` | in_progress → 终态见 §3 | run 45248945078 |

**总体**：新行为链（会话资料注入、记忆探针）独立复验 **PASS**；无新缺陷。

---

## 1. 本地证据（worktree @ `c69c9e5`，全新 target）

```text
$ cargo test --locked -p an-server --test session_materials --test desktop_mem \
    --test desktop_c2 --test desktop_accept --test desktop_replay --test mom_world_prod
desktop_accept: 4   desktop_c2: 7   desktop_mem: 9（08/09 为 #241 探针）
desktop_replay: 5   mom_world_prod: 6   session_materials: 5（RT-SMAT-01/02 + 预算/超时/撤回）
合计 36 passed / 0 failed

$ cd web && npm run build → 通过
$ npx playwright test e2e/desktop.spec.ts -g 'ADMEM|ADMEMW|ADRESUME' → 3 passed
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → REV ✓；MOBILE ✗（30s 超时同签名）
```

### 观察与口径

- **门控默认值**：`session_materials` **默认开**（`d_true`，与内省等特性「默认关」惯例不同）——属 #243 冻结需求内（REQ-conversation-memory-01，2026-11-23 用户确认），如实记录，不算缺陷。
- `store.rs` 变更为**只读链接查询**（+15 行），未动状态机写路径；`conversation.rs` 注入点在回答路径系统提示后、历史前，符合「数据，非指令」口径。

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 资料来源可辨（task_id + 完成态 + 摘要原文） | 通过 | RT-SMAT-01 |
| 「数据，非指令」注入位置与最新消息完整保留 | 通过 | RT-SMAT-01 |
| 撤回链接后资料不注入、回答不中断 | 通过 | RT-SMAT-02 |
| 预算裁剪先裁资料、当前请求不挤掉 | 通过 | session_materials 套件（max_tokens 用例） |
| 资料源超时不阻塞回答 | 通过 | session_materials 套件（timeout 用例） |
| 记忆跨库不泄漏 / 软删不被重播种覆盖 | 通过 | RT-ADMEM-08/09 |
| Mock / fixture 可辨 | 通过 | harness world_mock；S1 资料源=进程内真实 Store 链接表 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#241 / #243 / #244：全部 **8/8 pass**（逐个 `gh pr checks`，2026-11-23）。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `c69c9e5`（#244，run [45248945078](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45248945078)） | **success**（记录时 in_progress，复跑确认终态于同会话） |
| push @ `c69c9e5` 终态回填 | 2026-11-23 同会话回填，全部作业通过 |
| nightly | 11-23 收稿时点未触发（第 3 线监视至 ~21:50Z）；G15 / GR-4-NIGHTLY-E2E 状态不变 |

## 4. 问题清单

### Q-T65-01（延续，第 5 轮确认）— E2E-MOBILE 未修复

`c69c9e5` 复跑同签名失败。G15 三晚连过被阻塞；#241～#244 均未触及。归属不变（Desktop/前端，与 G15 根因合并）。

### 遗留

Q-T60-02（远程真站点 live）NOT_VERIFIED；T-75 真机麦 / T-76 真实 ASR 真设备路径不宣称。

---

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 会话资料注入 S1 落地：同会话续问带来源、撤回即失效、预算与超时安全 | 分层记忆与注入 S2+（第 5 线排期） |
| 记忆持久化验收探针补齐（隔离/软删/重启） | 画像/taste 设计未定稿（宪章原则 10 待确认） |
| 既有全链回归绿 | **E2E-MOBILE 未修 → G15/G6 无法关闭**（唯一业务阻塞） |
| — | 远程真站点 live（试点窗口前提）、真设备路径、M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、契约正文、断言、登记表、PLAN；未修 E2E-MOBILE；未跑远程真站点/真设备
