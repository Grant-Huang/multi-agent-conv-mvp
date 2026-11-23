# WP4 实施方按：任务拆分与模板执行（GS4 / M4.3）

| 项目 | 内容 |
|---|---|
| 任务 | T-73（方案卡）；实现另卡 |
| 日期 | 2026-11-22 |
| 状态 | **★方案已批准（#216）**；实现卡按本方按落地 |
| 缺口 | G02；黄金场景 GS4；PLAN M4.3 |
| 契约 | [state-and-cancellation](../contracts/state-and-cancellation.md) **S1/S2/S7**；[events-and-trace](../contracts/events-and-trace.md) **E1/R1/R2**（`task_id` + 父子关联；新写路径带 `trace_id`） |
| 权威设计 | [DESIGN.md](../DESIGN.md) **§9.6**；校验实现 `crates/runtime/src/template.rs`（RT-TPL-01～10 已绿） |
| 依赖 | **WP2 已合入**（#215）：子任务可走「待验收」出口，避免并行分支卡死 |
| 唯一实施负责人（获批后） | 本主线 worker（跨模块语义 + 编排）；实现卡不得并行改 `orchestrator.rs`/`store.rs` |
| 核对基线 | `origin/master` `bdcb082`（含 #215） |

**一句话**：把已校验的模板接入编排器——Router 封闭选择题 → 父任务挂并行分支 → 信封汇合 → Composer 只转述；吃不准不拆；首版三张手写模板，不做通用动态 DAG。

---

## 1. 问题与目标

| 现状 | 目标（GS4） |
|---|---|
| `template.rs` 仅校验 / `decide_split` / `assemble_join`；生产路径零调用（G02） | 注册表进 Runtime；命中模板则并行跑分支，汇合后一次交付 |
| 一条消息仍单任务；多问题混合委派整条一个动作（PLAN M4.3） | 仅当 `slots_ok=yes` 且模板与交付物匹配才拆；否则整单 |
| 无父子任务、无汇合编排、Composer 不知汇合结果 | 父/分支/汇合任务可追踪；缺一路记 `missing`、不填补；Composer 只转述 |
| 浏览器无模板路径（COVERAGE：templates 无界面入口） | 至少一条 E2E：受控模型触发 `quote`（或夹具强制 Split）→ 多分支进度可见 → 汇合交付 |

**非目标（本 WP4）**：

- 不做模型现场编图 / 通用动态 DAG / `depends_on` 边
- 不把 §9.5 研究报告流程改造成模板（报告已在 General 内并行，信封对齐即可）
- 不实施 WP5/WP7；不改 MOM/WA；不加多用户权限
- 不在本方案卡改业务代码（触 `orchestrator`/`store` 须批后再动）
- 不要求首版 F 组评测 ≥95%（属 M4.4；实现卡可另挂基线）

---

## 2. 推荐方案（★）

### 2.1 任务模型（父 / 分支 / 汇合）

```text
用户消息 → Turn/Router
  ├─ decide_split = Single → 现有单任务路径（不变）
  └─ Split(template_id)
        → 创建 parent（fanout=root，QUEUED/NEEDS_HUMAN 按 Policy）
        → 批准后（若需）为每个 branch 创建子任务（fanout=branch）并行 spawn
        → 全部子任务终态后 assemble_join
        → 创建 join 子任务（fanout=join）写入汇合结果 / 可选落盘
        → parent → COMPLETED；Composer 按 JoinInput + join.result 交付一次
```

| 角色 | `fanout_kind` | 说明 |
|---|---|---|
| 父任务 | `root` | 用户可见的「这次拆分」；持有 `template_id`；不直接跑 agent（或仅记账） |
| 分支 | `branch` | `branch_id` = 模板分支 id；`capability` → `Role`；goal/context 含 `serial_steps` |
| 汇合 | `join` | 输入 = `assemble_join` 结果；`writes_file` 时落盘；完成后驱动父终态与交付 |

**状态守卫（继承 S1/S2）**：

- 子任务仍用现有 `TaskStatus` 机；审查需人工走 **WP2**（`NEEDS_HUMAN` + result → `/review-verdict`）。
- 父任务：子任务未齐时保持非终态（建议 `RUNNING`）；任一支 `CANCELLED` 且策略为「取消整单」时父与其余 ACTIVE 子一并取消；全部终态且 join 成功 → 父 `COMPLETED`；join 失败或策略要求「任一支 FAILED 则整单失败」→ 父 `FAILED`（★默认：**过半分支 `ok` 才允许 join 落盘成功**，与报告「过半成功」对齐；否则 join 仍跑但 `complete=false`，父 `COMPLETED` 且交付写明缺失——与 §9.6「Composer 如实说缺哪路」一致）。
- ★默认汇合策略：**只要调度层能 `assemble_join`（无重复/未知 branch_id），就跑 join 并交付**；`complete=false` 不阻止交付，只在文案与事件中标明 `missing_branches`。任一支仍 `NEEDS_HUMAN` 时**不**启动 join（等人处理，满足 S7）。

### 2.2 持久化（★最小 DDL）

与 WP2「无列」不同：并行跨进程/重启要对齐父子，**建议两列**（`store.rs` 补列写法）：

```sql
ALTER TABLE tasks ADD COLUMN parent_id TEXT;     -- NULL = 顶层；子任务指向父
ALTER TABLE tasks ADD COLUMN fanout_kind TEXT;  -- NULL | 'root' | 'branch' | 'join'
ALTER TABLE tasks ADD COLUMN branch_id TEXT;    -- 分支 id；join 可用 '__join__'；root 为 NULL
ALTER TABLE tasks ADD COLUMN template_id TEXT;  -- root/join 冗余便于查询；branch 可同填
```

- 回滚：revert 代码后列可留（读取忽略）；无害。
- **备选（不推荐）**：零 DDL，把子 id 列表塞进父 `context` JSON——取消/列表/重启脆弱，仅当用户明确拒绝 DDL 时降级。

索引：实现卡可为 `parent_id` 加普通索引（可选）。

### 2.3 Router / 配置

| 项 | 内容 |
|---|---|
| Router 输出 | 在现有 `RouteDecision` **之外或之中**增加扁平三字段（§9.6）：`template_id`、`deliverable`、`slots_ok`；不适用填 `""` / `"no"`。★推荐：**RouteDecision 增可选字段**（serde default 空），一次结构化调用；避免二次 LLM |
| `decide_split` | 已实现；吃不准 → `Single` |
| 注册表 | 启动时 `TemplateRegistry::new`；模板来源 ★ `config/templates/*.json` + `[orchestrator] templates_dir`（缺省内置 `quote`/`stop_line`/`release` 三份与 §9.6 一致）；`max_branches` = 后台槽位（复用 RT-TPL-10 `background_capacity`） |
| Policy | 对 **父任务** 做一次审批/审查裁决；分支继承父的 `risk_level` 与工具约束（实现卡写清：分支 `required_tools` 默认空或按 capability） |

### 2.4 编排职责（高风险：`orchestrator.rs`）

| 步骤 | 行为 |
|---|---|
| 提交 | `Split` 时插入 parent（+ 可选先 `NEEDS_HUMAN`）；`approve(parent)` 后创建并 `spawn` 各 branch |
| 并行 | 每分支独立 `spawn`；执行组排队与现网一致；**不**在编排器内再实现一套槽位限制（已由 registry/pool 约束；模板加载时已卡分支数 ≤ 槽位） |
| 信封 | 分支完成时：从 `TaskResult.summary` 或约定 artifact（★推荐：要求分支 agent 最终 `answer` 为信封 JSON；解析失败 → 该路 `missing`）调用 `validate_envelope` |
| 汇合 | 全部 branch 终态且无 `NEEDS_HUMAN` → `assemble_join` → 创建 join 任务；join agent 收到 JoinInput JSON |
| 交付 | join 终态后 **只**对 parent 调 Composer 一次；分支过程可用进度事件，不各写一篇用户交付 |
| 取消 | `cancel(parent)` → 取消所有 `parent_id=parent` 且 ACTIVE 的子；`cancel(branch)` → 该支取消，汇合时记 `missing`（★默认不自动取消兄弟；若产品要「一票否决」在批准时写明） |

**禁止**：Reviewer / 分支 agent 调用「创建兄弟任务」；拆分只发生在编排层。

### 2.5 API / UI

| 面 | 首版 |
|---|---|
| HTTP | 复用现有 `GET /api/tasks`、`GET /api/tasks/{id}`（详情含 `parent_id`/`fanout_kind`/`branch_id`/`template_id`）；**可不新增**专用 API。列表可加 `?parent_id=` 过滤（实现卡可选） |
| 取消/批准/验收 | 仍 `POST .../cancel`、`/approve`、`/review-verdict`；批准对象为 **parent**（无 result）；验收对象为卡住的 **branch** |
| UI | 父任务卡展示模板名与分支状态摘要；子任务可在任务面板展开；对话区主状态跟父任务；文案避免把父的 `RUNNING` 说成单 agent「执行中」而无分支信息（labels 小改） |

### 2.6 事件与追踪

新事件（须进 DESIGN §10.3）：

| 事件 | 何时 |
|---|---|
| `task.template_split` | 决定拆分；字段：`template_id`、`parent_task_id`、分支数 |
| `task.template_branch_done` | 单路终态；`branch_id`、`status`（ok/missing/failed） |
| `task.template_join` | 汇合开始/结束；`complete`、`missing_branches` |

一律带 `task_id`（当前任务）；父/子通过 `parent_id` 关联（R2）。新写路径带 `trace_id`（R1）：父创建时生成，子继承同一 `trace_id`（若列/字段尚未存在，实现卡可先落在事件字段，WP6 再统一列）。

### 2.7 与 WP2 / 报告 / 桌面

| 关系 | 约定 |
|---|---|
| WP2 | 分支可进待验收；join **等待**人工出口；禁止把父标终态而丢子 |
| §9.5 报告 | 不改；内部并行保留 |
| 桌面 | 首版对话任务面板足够；桌面 Run 映射另议，非本卡 |

---

## 3. 没选的做法

| 做法 | 为什么没选 |
|---|---|
| 零 DDL 只改内存图 | 重启与取消无法对齐；与 G08/WP7 更糟 |
| 模型输出任意 DAG | §9.6 明确禁止；拆错成本高 |
| 每分支独立 Composer 交付 | 用户收到多条碎片；与「汇合回复」不符 |
| 拆分并进 `/approve` 语义 | 批准是执行前门，不是模板选择 |
| 本方案卡改 orchestrator/store | T-73 / AGENTS 高风险：先批后做 |
| 首版做 M4.4 评测门槛 | 可并行，但不阻塞编排接通 |

---

## 4. 迁移与回滚

### 4.1 ★默认（补列）

- 迁移：启动补列（可空）；旧行 `parent_id`/`fanout_kind` 为 NULL = 今日单任务。
- 回滚：revert 实现 PR；残留列忽略；进行中的模板父任务若代码回退，行为同「未知字段」，用户可取消。

### 4.2 配置

- 新增模板 JSON / `templates_dir`：回滚删配置即不再 Split（Router 空 `template_id` → Single）。

---

## 5. 实现卡顺序（批准后）

1. **Red**：编排集成用例 RT-TPL-11～（见 §6）在 `crates/server/tests` 或 `runtime/tests` 落地，断言失败。
2. **DDL + store**：父子查询、`list_children(parent_id)`、插入时写 `fanout_*`。
3. **Runtime 装载** `TemplateRegistry`；Router 三字段；conversation 在 delegate 路径调用 `decide_split`。
4. **orchestrator**：Split 提交 / 并行 spawn / 信封 / join / 单次 Composer；取消传播。
5. **UI**：父卡摘要 + 子状态；labels。
6. **E2E-TPL**（浏览器）：受控 harness 强制或触发 `quote` → 可见多路 → 汇合完成文案。
7. 更新 `features.json` / COVERAGE / DESIGN §0.3 状态 / §9.6「尚未改代码」句 / §10 事件；决策改有效。

**预估改动路径**：`store.rs`、`orchestrator.rs`、`conversation.rs`/`router.rs`、`lib.rs`（Runtime 装配）、`server`（若暴露过滤）、`web` 任务面板与 labels、`config/templates/*`、测试与 CASES。不改 `mom/engine.rs`、`DesConnector`、`report.rs` 主流程。

---

## 6. 验收矩阵（允许 + 拒绝）

| Case | 场景 | 期望 |
|---|---|---|
| RT-TPL-01～10 | 现有校验 | **保持绿**（基线，不改断言迎合） |
| RT-TPL-11（新） | Router 三字段 yes+quote | 创建 1 parent + 3 branch；事件含 `task.template_split` |
| RT-TPL-12（新） | 三路均 ok 信封 | join 跑完；parent COMPLETED；交付含三路字段；`complete=true` |
| RT-TPL-13（新） | 一路缺席/坏信封 | 该路 `missing`；不填补；交付写明缺失；`complete=false` |
| RT-TPL-14（新） | `slots_ok=no` / 未注册模板 | **不**拆分；单任务路径 |
| RT-TPL-15（新） | 取消 parent | 子 ACTIVE → CANCELLED；无新 join |
| RT-TPL-16（新） | 一支 `needs_human` | 不启动 join；WP2 accept 后可继续汇合 |
| （拒绝）分支重复 `branch_id` | 调度错误 | `assemble_join` Err；父 FAILED；有事件 |
| E2E-TPL | 浏览器 | 拆分进度可见；最终一条汇合向交付 |
| GS4 | 多问题/模板场景 | 无死胡同（S7） |

证据：假 LLM + 内存/临时 SQLite；harness 可对目标含「报价单模板」返回固定 RouteSlots。标注本地 / CI。

> 用例 ID 登记进 CASES 时：浏览器须 `E2E-[A-Z]+`（如 `E2E-TPL`）；运行时 `RT-TPL-NN`。

---

## 7. 风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| 误改编排破坏单任务/WP2 竞态 | L2 | 唯一负责人；条件 UPDATE；RT 回归 approve/review-verdict/cancel |
| 分支输出非信封 JSON | 中 | 解析失败 → missing；提示词与 RT 夹具固定格式 |
| 槽位不足仍拆分 | 中 | 加载时 `max_branches`；Router `slots_ok`；容量变化时拒绝注册 |
| 范围蔓延到动态 DAG / WP7 | 高 | 非目标写死；PR 检查 |
| DDL 与多 agent 并行改 store | 高 | 本卡独占 store；列可空兼容 |

---

## 8. 请批示

**请批准本 ★方案作为 WP4 实现卡的唯一依据**：

1. 父/分支/汇合任务模型 + **最小 DDL**（`parent_id` / `fanout_kind` / `branch_id` / `template_id`）。
2. Router 三字段 + 现有 `decide_split`；吃不准不拆。
3. 编排接入 `assemble_join`；Composer 只转述；缺路不填补。
4. 唯一实施负责人改 `orchestrator.rs` / `store.rs`；本 T-73 **只合文档**。
5. 若否决 DDL，请明示改用「context JSON 备选」，并接受重启/取消弱化。

批准后开实现卡：先 Red（RT-TPL-11～）再 Green。
