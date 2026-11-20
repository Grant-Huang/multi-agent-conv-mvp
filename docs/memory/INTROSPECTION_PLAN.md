# 业务内省：实现方案（已批准）

状态：**已批准（2026-11-18，第 13 节 7 项全部同意）。S0–S2 已完成（2026-11-19）；从 S3 起按 [INTROSPECTION_PLAYBOOK.md](INTROSPECTION_PLAYBOOK.md) 逐步做；每步一个 PR，用户确认后再进入下一步。**
依据：`docs/DECISIONS.md` 第 45–55 条；验收用例 `docs/memory/ACCEPTANCE.md`（RT-INT-*、E2E-INTRO*）；入口 [README.md](README.md)。
范围：只含**业务内省**（定时分析 → 发送闸门 → 推送到主窗口 → 未读角标 → 运行时标注）。个人风格、决策画像、组织 taste 属于另一份方案，不在这里。

**接手实现请读 Playbook，不要只读本节摘要。** 本节保留架构与批准记录；可执行粒度（类型签名、夹具、Red→Green、CASES 配方）在 Playbook。

按 `AGENTS.md`，下列高风险改动已在第 13 节获批：`store.rs` 新建表（S1 已做完）、`ChatCard::Introspection`、`[introspection]`、`/api/introspection/*`。S2+ **不要再改表结构**，除非重新批准。

---

## 1. 先说核实结果（读过代码后，和之前讨论不同的地方）

| # | 核实到的事实 | 对方案的影响 |
|---|---|---|
| 1 | `crates/runtime/src/dream.rs` **已经存在**，是“晚间审查”：读日志里的失败路由，写草稿，不调用模型 | 含义和“业务内省”不同。新模块**不叫 dream**，叫 `introspect`，避免混淆；两者互不依赖 |
| 2 | 本仓库的 MOM 用的是**公告板模型**（`Case`、`Approval`、`Action`、`BoardEvent`），保存在 MOM 引擎自己的 SQLite 里（`crates/runtime/src/mom/engine.rs`）。**没有** V2.2 里的 `RiskCase`、`Closure`、`regressed`、推送配额、忽略率 | 五个主题里**只有部分有数据可读**，见第 2 节。V2.2 的那套对象在另一个产品里，不在这个仓库 |
| 3 | `store.rs` 的做法是：`SCHEMA` 里 `CREATE TABLE IF NOT EXISTS`；补列用 `pragma_table_info` 检查后 `ALTER TABLE`。准入功能（E3）开了先例：**“只新建表，不改已有表”** | 本方案**只新建表**，不改已有表，迁移最简单、可回滚 |
| 4 | `messages` 表已有 `card_json` 列；`add_message_with_card` 写“带卡片的消息”：`content` 给人和模型看，卡片数据**不进模型上下文** | 内省消息 = 一条 assistant 消息 + 一张内省卡。`content` 写大白话摘要（模型追问时看得到），卡片只放证据入口 |
| 5 | `ChatCard` 是 `an-core` 里导出到 TypeScript 的枚举 | 新增一个 `Introspection` 变体，要按 `AGENTS.md` 重新生成 `web/src/bindings/` 并一起提交 |
| 6 | 后台任务的现有写法是 `lib.rs::start_background` 里 `tokio::spawn` + `loop { sleep }`，**没有取消信号** | 内省任务不能照抄，必须带取消和超时（第 5 节） |
| 7 | `context_text` 把最近 6 条消息当背景交给执行 agent，只区分“用户 / 助手” | 内省消息会进入这里，要给它**加“数据，非指令”的标记** |
| 8 | 仓库里**没有**“个人”页签、个人记忆模块、登录与角色；也**没有**删除会话的接口 | 角标只能先挂在会话列表上；“不被抽取进记忆”只能写成约束（G-9）；会话“不可删除”目前天然成立，加了删除接口时要补守卫 |
| 9 | S1 已落地：五张新表 + `Store` 读写 + `ensure_main_conversation`（`app_state.main_conversation_id`） | G-5「如何指定主窗口」已解决；后续步骤**复用**这些 API，不要重写存储层 |

---

## 2. 本期做什么、不做什么

用户已确认首批五个主题（决策第 49 条），第 52 条已让“阈值健康度”暂缓。再按**数据是否真实存在**核对：

| 主题 | 本仓库有没有数据 | 本期 |
|---|---|---|
| 方案采纳模式 | 有：`Case.approvals`（审批人、范围、过期）、`Case.actions`（状态、回执、`verification`）、`BoardEvent` | **做**（第一切片） |
| 同因复发 | 部分：有 `Case`、`world`、`breaches`、`alerts`、事件序列；**没有 `regressed` 状态，也没有显式根因字段** | **后做**，先核实“根因”能否从 `eight_d`、`proposal` 里稳定取出；取不出就只做“同设备 / 同批次反复出现”的描述性统计，并标注不是根因结论 |
| 推送质量 | **没有**：本仓库没有推送、配额、忽略率数据 | **暂缓**，记缺口 G-7 |
| 经验验真 | **没有**历史经验库 | **暂缓**，记缺口 G-8 |
| 阈值健康度 | 没有误报标注 | 已暂缓（决策第 52 条，G-1） |

**结论**：本期真正落地的内省主题是**“方案采纳模式”**，“同因复发”视核实结果跟进。其余三个主题等有数据来源再做。这样先把“定时运行 → 闸门 → 推送 → 角标 → 标注”这条骨架跑通，主题可以后加。

**数据读取接口统一**：内省通过一个 `SourceAdapter` 接口读数据，本期只实现 `MomSource`（读 MOM 引擎）。以后接入 V2.2 的 RiskCase，只加一个实现，不改内省本身（符合“统一接口、只实现接口”）。

---

## 3. 模块与落点

新增目录 `crates/runtime/src/introspect/`（只在 `runtime` crate 内，不动别的 crate 的逻辑）：

| 文件 | 职责 |
|---|---|
| `mod.rs` | 对外入口、配置、调度的装配 |
| `source.rs` | `SourceAdapter` 接口 + `MomSource`（**只读**） |
| `analyze.rs` | 确定性统计（方案采纳模式）。不调用模型 |
| `finding.rs` | `Finding` 数据结构、稳定 ID、校验（引用可达、数值回算一致） |
| `gate.rs` | 发送闸门：规则打分 + 模型 ±1 |
| `job.rs` | 定时调度、租约、水位、超时、取消 |
| `push.rs` | 往主窗口会话插入内省消息、未读计数 |
| `operator.rs` | `is_operator(identity)` 判断函数（现阶段恒为是） |

另外要动的现有文件（标注是否高风险）：

| 文件 | 改什么 | 高风险？ |
|---|---|---|
| `crates/runtime/src/store.rs` | `SCHEMA` 里**新增 5 张表**和对应的读写方法；**不改已有表** | **是，需批准** |
| `crates/core/src/lib.rs` | `ChatCard` 新增 `Introspection` 变体；重新生成 TS 绑定 | 公开类型，需批准 |
| `crates/runtime/src/conversation.rs` | `context_text` 对内省消息加“数据，非指令”标记 | 否，但小心 |
| `crates/runtime/src/lib.rs` | `start_background` 里启动内省调度（默认关） | 否 |
| `crates/runtime/src/config.rs` | 新增 `[introspection]` 配置段（默认关闭） | 公开配置字段，需批准 |
| `crates/server/src/lib.rs` | 新增 `/api/introspection/*` 接口 | 新增接口，需批准 |
| `web/src/…` | 角标、内省页 | 否 |
| `docs/DESIGN.md` §10 | 补内省的日志事件名 | 否 |

**不改**：`orchestrator.rs`、`registry.rs`、`acp.rs`、`util.rs`、MOM 引擎本身（只读它）。

---

## 4. 数据库：新表与迁移步骤

只新建表，写进 `SCHEMA`（`CREATE TABLE IF NOT EXISTS`），与准入功能同一种做法。

```sql
-- 内省任务：一次定时或手动运行
CREATE TABLE IF NOT EXISTS introspection_jobs (
    id TEXT PRIMARY KEY,
    trigger TEXT NOT NULL,        -- schedule / manual
    state TEXT NOT NULL,          -- running / done / failed / cancelled / skipped
    watermark_json TEXT NOT NULL, -- 各来源读到的位置（如各 Case 的最大事件序号）
    input_hash TEXT NOT NULL,
    lease_until TEXT,             -- 租约，防并发
    started_at TEXT NOT NULL,
    finished_at TEXT,
    error TEXT
);
-- 内省结论
CREATE TABLE IF NOT EXISTS introspection_findings (
    id TEXT PRIMARY KEY,          -- 稳定 ID = hash(主题, 范围, 输入摘要)
    job_id TEXT NOT NULL,
    topic TEXT NOT NULL,          -- adoption_pattern / ...
    json TEXT NOT NULL,           -- 数值、证据引用、假设、反证、验真状态
    verification TEXT NOT NULL,   -- supported / inconclusive / contradicted
    gate_score REAL,
    gate_reason TEXT NOT NULL,    -- 推送或不推送的原因（每条都记）
    pushed INTEGER NOT NULL DEFAULT 0,
    verdict TEXT,                 -- 运营负责人：confirmed / rejected
    verdict_note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_findings_topic ON introspection_findings(topic, created_at);
-- 推送记录与未读
CREATE TABLE IF NOT EXISTS introspection_pushes (
    id TEXT PRIMARY KEY,
    finding_id TEXT NOT NULL,
    message_id TEXT NOT NULL,     -- 写进主窗口会话的那条消息
    pushed_at TEXT NOT NULL,
    read_at TEXT                  -- NULL 即未读
);
-- 运营负责人的运行时标注（“该推 / 不该推”、以后的“是否误报”）
CREATE TABLE IF NOT EXISTS introspection_labels (
    id TEXT PRIMARY KEY,
    finding_id TEXT NOT NULL,
    kind TEXT NOT NULL,           -- should_push / should_not_push / ...
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
-- 应用状态（目前只存主窗口会话 ID）
CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

**迁移步骤（用户规则要求写明）**：
1. 部署新版本，启动时 `SCHEMA` 自动 `CREATE TABLE IF NOT EXISTS`，旧库无需手工操作。
2. 不修改任何已有表、列、索引，旧数据完全不动。
3. 默认 `[introspection] enabled = false`，没打开前新表保持为空，行为与以前一致。
4. **回滚**：把 `enabled` 改回 `false` 即可停用；回退到旧版本程序时，旧程序不认识这些表，会忽略它们，可以正常读写原来的库。新表可以留着，也可以手工 `DROP`，不影响旧功能。

---

## 5. 定时运行：超时、取消、清理（高风险点逐条说明）

| 点 | 做法 |
|---|---|
| 触发 | 配置里 `schedule_hours`（间隔小时数，初稿 24）；启动后按间隔运行；`POST /api/introspection/run` 手动触发，**同样走发送闸门** |
| 单实例 | 运行前在 `introspection_jobs` 抢租约（`lease_until`）；已有未过期租约则本轮跳过并记 `introspection.job_skipped`。单进程部署也保留，防手动与定时撞车 |
| 超时 | 整个任务用 `tokio::time::timeout` 包住，初稿 120 秒；超时标 `failed`，已提交的 Finding 保留，不丢原始输入 |
| 取消 | 持有一个 `tokio::sync::watch` 取消信号，进程退出或 `enabled` 热关闭时发出；每个主题之间检查一次；取消后不再写新 Finding，标 `cancelled` |
| CPU / 数据库 | 统计放进 `spawn_blocking`，不堵 tokio 线程；读 MOM 只用只读接口 |
| 清理 | 任何结束路径（成功、失败、取消、超时）都释放租约；临时数据只在内存，不落临时文件；启动时把上次崩溃遗留的 `running` 且租约已过期的任务标为 `failed`（`task.interrupted` 同类做法） |
| 幂等 | 同一水位、同一输入哈希重跑，Finding 的稳定 ID 相同，用 `INSERT … ON CONFLICT DO NOTHING`，不重复生成，也不重复推送 |
| 不抢前台 | 需要调用模型的步骤（本期放在最后一步，且默认关闭）用“取不到槽位就跳过本轮、不排队”的方式；前台对话永远优先 |
| 重试 | 不自动重试；下一个周期自然再跑，水位不回退 |

---

## 6. 分析与校验（以“方案采纳模式”为例）

**确定性统计（代码算，不用模型）**，按 `MomSource` 读到的数据：

- 每个场景模板下：审批发起数、通过数、过期数；
- 动作的最终状态分布（成功、失败）与 `verification` 结果；
- 从提案到审批的耗时分布。

**样本门槛**：每个分组样本数低于 `min_samples`（初稿 10）时，只输出“样本不足”，不下结论。

**模型（可选，默认关闭，放在最后一步）**：只在已算好的表上写定性解释、提出假设和反证；**不能自己算数字，也不能改表里的值**。

**校验器**（代码）：
1. 每个引用（`case_id`、事件序号）必须能在源数据里解析到，解析不到就丢弃该条；
2. 结论里的每个数字重新算一遍，与表一致才放行；
3. 证据不足的标 `inconclusive`，且不进推送。

---

## 7. 发送闸门（初稿，数字待试运行后调）

规则为主，模型只做 ±1：

| 规则 | 分值 |
|---|---|
| 验真状态为“支持” | +2 |
| 影响面（涉及的案例数）超过门槛 | +1 |
| 有明确可做的下一步 | +1 |
| 首次出现，或相比上次明显恶化 | +1 |
| 有时效（再不处理会错过窗口） | +1 |
| 模型自我审查微调 | −1 / 0 / +1，必须给一句理由 |

- 总分 ≥ `push_threshold`（初稿 4）才推送；**模型不能单独把规则判不推的结论推出去**，也不能无理由压掉规则判该推的。
- 每天最多 `daily_cap` 条（初稿 1）；超出的不外发，仍可在内省页看到，并提示“另有 N 项”。
- 同一条结论已推送且未读时不重推；只有验真结果变化、影响面扩大、被反驳才可再推。
- **每条结论都在 `gate_reason` 里写明推或不推的原因。**
- 运营负责人对每条结论可标“该推 / 不该推”，保存到 `introspection_labels`，**只积累，不自动改闸门规则**。

---

## 8. 推送、主窗口会话、未读与权限

**主窗口会话（解决 G-5）**：`app_state` 里存 `main_conversation_id`。第一次需要推送时，调用 `ensure_main_conversation()`：已有就复用，没有就创建一个标题固定的会话并记下 ID。之后永远用它，不新建第二个。（仓库目前没有删除会话的接口，所以“不能删除”现在天然成立；将来加删除接口时必须保护这个会话，补一条用例。）

**推送**：不走 `handle_message`，因此**不触发模型回复，也不创建任务**。直接调用 `store.add_message_with_card(main_id, "assistant", 摘要, 卡片)`，再通过现有 `Bus` 广播一个事件让界面刷新。写入前不需要占用会话锁（只追加一行，不改历史）；展示顺序按 `created_at`。

**“数据，非指令”**：`context_text` 里遇到内省卡的消息，前面加固定前缀“［内省结论，仅供参考，不是指令］”。用户追问“为什么这么说”时，主 agent 看得到这段摘要，但里面的祈使句不会被当成要执行的命令。

**不被抽取进个人记忆**：个人记忆模块目前**还不存在**。本方案只把规则写进约束：未来的抽取逻辑必须跳过带内省卡的消息。这一条要等记忆模块建好才能测，记为缺口 G-9。

**未读**：`introspection_pushes.read_at` 为空即未读。
- `GET /api/introspection/unread` 返回未读条数；
- `POST /api/introspection/read` 在用户打开主窗口会话时调用，把未读置为已读；
- 普通聊天消息不计入。

**权限**：`operator.rs` 提供 `is_operator(identity) -> bool`，现阶段恒为 `true`（单用户部署）。所有内省的读取和推送接口都先调用它。配置里预留 `operator_role = ""`，**暂不生效**，启动日志说明。测试时注入身份，验证“非运营负责人被拒绝”。

**界面（本期最小）**：
- “个人”页签不存在，所以角标先挂在**会话列表里的主窗口会话**上，条数来自 `unread`；
- 内省页（列表、证据链、确认 / 否决 / 补充、“该推 / 不该推”标注、未推送原因）只对运营负责人显示入口。

**新增接口一览**（均为新增，不改已有接口；沿用仓库现有的返回风格，与已有接口保持一致）：

| 方法 | 路径 | 作用 |
|---|---|---|
| GET | `/api/introspection/findings` | 列表（含未推送原因） |
| GET | `/api/introspection/findings/{id}` | 详情与证据链 |
| POST | `/api/introspection/findings/{id}/verdict` | 确认或否决，附理由 |
| POST | `/api/introspection/findings/{id}/label` | 标“该推 / 不该推” |
| POST | `/api/introspection/run` | 手动触发（同样过闸门） |
| GET | `/api/introspection/unread` | 未读条数 |
| POST | `/api/introspection/read` | 标记已读 |

---

## 9. 配置与日志

**配置**（`config.toml` 新增段，**默认关闭**）：

```toml
[introspection]
enabled = false
schedule_hours = 24
timeout_secs = 120
min_samples = 10
push_threshold = 4
daily_cap = 1
operator_role = ""   # 预留，暂不生效
```

**日志事件**（补进 `docs/DESIGN.md` §10，字段关联 `task_id = job_id`）：
`introspection.job_start`、`introspection.job_end`、`introspection.job_failed`、`introspection.job_skipped`、`introspection.finding`、`introspection.gate`（带推或不推的原因）、`introspection.push`、`introspection.read`、`introspection.denied`。

---

## 10. 分步实施（每步一个目标，先写测试再改实现）

每步的流程：**先写该步测试并取得 Red 证据（业务断言失败，环境失败不算）→ 把对应用例并入 `CASES.md` 并同步 `features.json` → 写实现到 Green**。出口是该步用例通过，且 `CARGO_TARGET_DIR=target cargo test`、`cargo clippy --all-targets`、`cd web && npm run build`、`npm run test:map` 通过，并贴出实际输出。

| 步 | 目标 | 主要落点 | 对应用例 | 回滚 |
|---|---|---|---|---|
| S0 ✅ | 冻结业务内省的验收用例，记录批准，建进度清单。**不一次并入 `CASES.md`**（`check-map` 要求每个 RT 用例都有 Rust 测试），改为每步先写测试再并入 | 仅 docs（#107） | 冻结范围见 ACCEPTANCE 七之三 | 删除文档改动 |
| S1 ✅ | 新表与 `Store` 读写方法 | `store.rs`（#122） | `int_store_*` 单元测试；RT-INT-09/26/27 存储部分；RT 行随 S4/S5 并入 | 新表可忽略 |
| S2 ✅ | `SourceAdapter` + `MomSource`（只读）+ 方案采纳模式的确定性统计 + 校验器 | `introspect/` | RT-INT-03、06、07、08 | 删除新目录 |
| S3 | 发送闸门 | `gate.rs` | RT-INT-16、17、18、19、20、22 | 同上 |
| S4 | 定时调度、租约、超时、取消、清理 | `job.rs`、`lib.rs`、`config.rs` | RT-INT-09、13、14、23 | 配置关闭 |
| S5 | 主窗口会话、推送、未读、`is_operator`、`ChatCard` 新变体、`context_text` 标记 | `push.rs`、`operator.rs`、`core`、`conversation.rs` | RT-INT-21、24（除记忆）、25、26、27、28（除记忆）、29 | 配置关闭 |
| S6 | HTTP 接口与前端：角标、内省页、标注 | `server`、`web` | E2E-INTROVIEW、INTROLABEL、INTROPUSH、INTROUNREAD；RT-INT-10 | 隐藏入口 |
| S7 | 用 DES 做“植入真值”评测；再决定要不要加模型解释层（默认关闭，后台槽位 2~5，取不到就跳过） | `des`、评测脚本 | RT-INT-15、评测报告 | 不启用模型层 |
| S8 | “同因复发”：先核实根因字段，能取出就做 | `introspect/` | RT-INT-02（核实后或重写，需用户确认） | 不启用该主题 |

每步尽量只动一个 crate；S5 同时动 `core` 和 `runtime`，因为 `ChatCard` 在 `core`，已在第 3 节标出。  
**逐步怎么写测试、类型签名、夹具、打分公式、CASES 配方 → [INTROSPECTION_PLAYBOOK.md](INTROSPECTION_PLAYBOOK.md)。**

---

## 11. 风险与对策

| 风险 | 对策 |
|---|---|
| 数据太少，大部分主题“不下结论” | 这是正确行为；内省页明确显示“样本不足”，不让人误以为坏了 |
| 内省消息混在工作对话里打扰 | 闸门克制 + 每天默认 1 条；以后觉得吵，可改成独立会话，改动集中在 `ensure_main_conversation` |
| 模型编数字 | 数字只来自代码；校验器回算不一致就丢弃 |
| 内省抢前台资源 | 取不到槽位就跳过；统计放 `spawn_blocking`；任务有超时和预算 |
| 误改 `store.rs` 影响现有功能 | 只新建表；`store.rs` 现有测试全部保持通过；新增方法单独测试 |
| `ChatCard` 变更牵动前端 | 新增变体，不改旧变体；重新生成并提交 TS 绑定，CI 会检查 |
| 闸门初始数字不准 | 全部可配置；每条记录原因，运行时标注用于回调（G-3） |

---

## 12. 新增的已知缺口

| 编号 | 缺口 | 重新启用条件 |
|---|---|---|
| G-7 | 本仓库没有推送、配额、忽略率数据，“推送质量”主题无法做 | 有真实推送来源（或接入 V2.2 的数据）后再做 |
| G-8 | 没有历史经验库，“经验验真”主题无法做 | 建立带适用条件的经验库后再做 |
| G-9 | 个人记忆模块不存在，“内省消息不被抽取进记忆”目前只是约束，无法测试 | 记忆模块建好后补用例 |
| G-10 | MOM 现有数据没有 `regressed` 和显式根因字段 | S8 核实后决定，或在 MOM 里补字段（另议） |

---

## 13. 请你批准或修改的事项

1. 本期只落地“方案采纳模式”这一个内省主题，“同因复发”后做，另外三个暂缓，是否同意？
2. 允许在 `store.rs` 的 `SCHEMA` 里**新增 5 张表**（不改已有表），迁移与回滚方式如第 4 节？
3. 允许在 `an-core` 的 `ChatCard` 里**新增 `Introspection` 变体**并重新生成 TS 绑定？
4. 允许新增 `[introspection]` 配置段和 `/api/introspection/*` 接口（均默认关闭 / 全新增）？
5. 模块命名用 `introspect`（不用 dream），是否可以？
6. 角标本期先挂在会话列表里的主窗口会话上，等“个人”页签做出来再迁移，是否可以？
7. 发送闸门的初稿数字（阈值 4、日上限 1、样本门槛 10、超时 120 秒）是否作为初始值？

第 13 节 7 项已于 2026-11-18 全部批准；S0/S1 已完成。每一步完成后更新 `docs/PLAN.md` 与 Playbook 进度表，并等用户确认再进入下一步。
