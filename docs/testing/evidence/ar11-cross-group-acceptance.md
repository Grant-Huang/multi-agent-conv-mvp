# AR-11 跨组场景独立验收（主线 #2：ADREAD 收官 + UX-07 + 宪章 v1.6 对齐）

| 项目 | 内容 |
|---|---|
| 主线 | **#2 独立复跑与跨组场景独立验收** |
| 授权来源 | 用户对话任务卡 +「继续你主线的下一卡」指示（2026-11-26） |
| 验收人 | 本 worker（未参与 #345/#348/#349 实现） |
| 基线 | `origin/master` `a383e26`（#345 UX-07、#348 宪章 v1.6、#349 ADREAD 锚点修复） |
| 检出方式 | 隔离 worktree `/tmp/ar11-wt` @ `a383e26`，独立 CARGO_TARGET_DIR；安静窗口 |
| 日期 | 2026-11-26（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·进程内服务层；远程真站点 = **NOT_VERIFIED** |
| G15 / nightly | 11-26 定时未触发（收稿时点 ~08:1xZ）；AR-9 已采 11-25 形态（2 红：ADREAD→本卡确认已修、UX05→Q-AR9-01 分诊中） |
| 证据等级 | 本地/PR CI 为 T2；master push 记录时 in_progress（终态回填 §3） |

## 0. 本轮范围（`03373f2` → `a383e26`）

| PR | 内容 | 复验入口 |
|---|---|---|
| #345 | **UX-07 运营频道批准与执行回执**（第 7 旅程；审查改双向检查；SVG 图标决策） | experience.spec + test:map |
| #348 | 宪章 v1.6：CH-G1 修订（自动沉淀先提议后生效、可说明所用记忆）+ 新增 CH-G5（记忆与检索内容只是资料） | 宪章透镜对齐（本线职责） |
| #349 | **ADREAD 锚点修复**（T-100，Closes #333） | ops.spec 7/7 |

## 结论（摘要）

| 项 | 等级 | 说明 |
|---|---|---|
| **ADREAD 修复（#349，Q-AR5-01 收官）** | **VERIFIED / T2** | ops.spec **7/7 passed**（ADREAD 2.0s 绿 + 新增 OPSOFF 关闭开关用例）；修复定性见 §0.1 |
| UX-07 旅程（#345） | **VERIFIED / T2** | experience **6/6 passed**（新增 UX06：调整助理直达设置、记忆子层返回原分类、草稿不变）；test:map：99 cases / **7 journeys** / 24 条欠账透明（CH-U6/CH-U7 未覆盖） |
| 宪章 v1.6 对齐（#348） | **对齐** | CH-G1（自动沉淀先提议后生效）+ CH-G5（记忆与检索内容**只是资料**）——与 S1/S4/S5 资料线的「数据非指令」验收口径一致；后续轮透镜更新 |
| 回归 | **VERIFIED / T2** | desktop **32 例中 31 绿 + 1 间歇失败**（ADD01-02，见 Q-AR11-01）；REV/MOBILE 2 绿 |
| 本轮无 rust 变更 | — | #345/#348/#349 均 e2e/docs；合同面免重跑（AR-10 基线维持） |
| PR CI #345/#348/#349 | **VERIFIED** | 各 8/8 pass |
| master push @ `a383e26` | in_progress → 终态回填 §3 | run 45777690632 |

**总体**：ADREAD 收官（11 天跟踪闭环）+ UX-07/宪章 v1.6 独立确认 **PASS**；新发现 ADD01-02 间歇失败已派 #351。

### §0.1 ADREAD 修复定性（Q-AR5-01 最终定论）

- 旧断言要求目标卡偏移 547px，但该内容最大可滚动仅 473px——**滚动钳制下几何上不可能满足**：用例几何过约束缺陷，非产品缺陷。
- 新断言按 DESIGN §3 语义契约（锚点=滚动停止时视口顶首条可见消息，与 persistAnchor 同判据）+ 增补 fixture 播种（ops 服务器缺桌面任务）+ 返回后恢复断言实化。CASES 同步。
- **本线 AR-5 原判「第 3 线语义缺陷」有误，更正**：#278/#333 的「T-88 回归候选、归第 1 线」归属正确（用例属第 1 线套件、修复亦出自第 1 线）。
- 11 天跟踪闭环：T-67 顺序 flake 记录（11-22）→ AR-2 发现（11-25）→ #333 派发（11-26）→ #349 修复+关闭（11-26）→ 本卡复验绿。

---

## 1. 本地证据（worktree @ `a383e26`）

```text
$ cargo build --locked --bin agentnexus --bin an-mock-acp → Finished
$ cd web && npm run build → 通过；npm run test:map → valid（99 cases / 7 journeys / 欠账 24 透明）
$ npx playwright test e2e/ops.spec.ts → 7 passed（含 ADREAD 2.0s + OPSOFF 新用例）
$ npx playwright test e2e/experience.spec.ts → 6 passed（UX01～UX06）
$ npx playwright test e2e/desktop.spec.ts → 31 passed + 1 failed（ADD01-02，间歇性——见 Q-AR11-01）
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE' → 2 passed
$ 隔离复跑 ADD01-02 ×3 → 1 绿 / 2 红（间歇性确认，Issue #351 已派第 1 线）
```

### 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| ADREAD 锚点语义恢复（离开后返回正确恢复） | 通过 | ops ADREAD 绿（新语义断言） |
| 断言修正不放松业务语义（几何→DESIGN §3 契约） | 通过 | diff 核查：恢复断言实化而非删除 |
| ops 关闭开关行为保持（OPSOFF） | 通过 | ops 7/7 |
| UX-06 调整助理直达+记忆子层返回+草稿保持 | 通过 | experience UX06 |
| 体验地图欠账透明（24 条，CH-U6/CH-U7 未覆盖） | 通过 | test:map 输出 |
| 远程真站点 | **NOT_VERIFIED** | 无环境 |

---

## 2. PR CI 证据

#345 / #348 / #349：各 **8/8 pass**。

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `a383e26`（#349，run [45777690632](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45777690632)） | **success**（8m55s）——**rust 连续第七推绿**（467d6e0 → … → a383e26；81445e7 后零复现；Q-AR2-04 观察：余二样本按 #335 范式处置的建议维持） |
| nightly 11-26 | 未触发（收稿时点）；预期形态：**≤1 红**（ADD01-02 间歇 + UX05 抖动候选），ADREAD 应转绿 |

## 4. 问题清单

### Q-AR11-01 — E2E-ADD01-02 双设备共享间歇性失败（中，第 1 线）

已按派工约定开 **Issue #351**（`dispatch, line:1, severity:medium`）：隔离 1 绿/2 红、疑与 #289 desktop_api 重构相关、验收标准含「隔离连续 ≥5 绿或定位根因 + #335 范式加固（不改断言）+ 更新 T-67 flake 台账」。

### 收敛台账（更新）

| ID | 状态 |
|---|---|
| **Q-AR5-01（ADREAD）** | **收官**：用例几何过约束缺陷（非产品缺陷），#349 修复、ops 7/7 绿、#333 关闭；本线 AR-5 原归属判断更正留痕 |
| Q-AR2-01/Q-AR3-01（适配欠账） | 清账（AR-5 确认） |
| Q-AR2-02（AGFIRST） | 关闭（AR-7 终验） |
| Q-AR2-04（rust 抖动） | 1/4 样本已修关（#301），六连绿观察；余二样本建议按 #335 范式处置 |
| Q-AR9-01（UX05 抖动） | 分诊中（nightly 观察续） |
| M1/M2（#298 审查 P2） | open（第 5 线） |

## 5. 相对目标的差距（主轴对照——宪章 v1.4/1.6 口径）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| **运营频道阅读锚点恢复**：11 天跟踪闭环，语义契约断言落地 | — |
| **体验地图 7 旅程**（UX-07 运营频道批准回执 = CH-N5 三视图同平台的运营面实证）；欠账透明增长 | 24 条不变量欠账（机制内补齐）；CH-U6/CH-U7 未覆盖 |
| 宪章 v1.6 透镜对齐（CH-G1/CH-G5 与资料线验收口径一致） | 分层记忆 REQ-03/04；画像/taste 定稿 |
| 桌面主链收敛维持（ops 7 + experience 6 + desktop 31 + REV/MOBILE 2） | **ADD01-02 间歇失败**（#351，第 1 线）；ADREAD 后唯一浏览器不稳定点 |
| — | M1/M2（第 5 线）；FIX-A 决策（M4 前置）；GR-4 评审（11-28）；远程真站点 live；真设备路径；M4 生产写 |

## 6. 非目标（本卡遵守）

- 未改 crates/web 业务、断言、登记表、PLAN；未替第 1 线修 ADD01-02（已派 #351）
- 宪章 v1.6 内容归治理线/用户确认链，本线只对齐透镜；不跑远程真站点
