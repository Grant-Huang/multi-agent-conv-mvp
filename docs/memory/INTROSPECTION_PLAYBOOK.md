# 业务内省：可执行说明书（S2–S8）

给后续（含较弱）agent 用。总方案见 [INTROSPECTION_PLAN.md](INTROSPECTION_PLAN.md)；冻结用例见 [ACCEPTANCE.md](ACCEPTANCE.md)；进度勾选在 `docs/PLAN.md`「业务内省（INT）」。

**状态**：S0、S1 已完成（2026-11-19）。从 **S2** 开始按本节做；每步一个 PR；用户确认后再开下一步。

---

## 0. 怎么用

1. 打开本文件对应步骤整节，按「落点 → 类型签名 → 夹具 → Red → 并入用例 → Green → 出口」顺序做。
2. 不要跳步；不要提前实现下一步的接口。
3. Red 证据必须贴在 PR 描述里（失败的测试名 + 业务断言原文）。
4. 完成本步后：勾选 `docs/PLAN.md` 对应项、把本文件该步状态改成 ✅、开 PR。

### 0.1 通用硬规则（每步）

| # | 规则 |
|---|---|
| 1 | 只动本步「落点」列出的文件；无关模块不改 |
| 2 | 默认关闭：未实现配置前行为必须与以前一致；实现配置后默认 `enabled = false` |
| 3 | **只读 MOM**：禁止调用 `Engine::ingest/approve/execute/...` 等写方法；测试夹具可以写，生产路径不能写 |
| 4 | 数字只由代码算；LLM 若出现（S7）只能解释，不能改数 |
| 5 | 高风险文件：`store.rs` 在 S1 已批准且已交付；S2+ **不要再改表结构**，除非用户重新批准。S5 改 `ChatCard` / `[introspection]` / HTTP 路径已在总方案第 13 节批准 |
| 6 | RT 用例并入 `CASES.md` 时：函数名必须是 `rt_int_NN_...`（小写、下划线），放在 `crates/runtime/tests/` 或模块内测试；同步 `docs/testing/features.json`；跑 `cd web && npm run test:map` |
| 7 | 浏览器用例 ID 只能是纯字母：`E2E-INTROVIEW` 等（见 ACCEPTANCE） |
| 8 | 不改已有测试断言；不 `#[allow]` 消警告；不直接推 `master` |
| 9 | 本机偶发失败（如 `rt_adm_14_admission_http_end_to_end`）若在干净 `origin/master` 上同样失败，记入 PR 说明，**不要为它改业务代码**；以 CI 为准 |

### 0.2 完成定义（每步出口都跑，贴输出）

```bash
cargo fmt --all -- --check
CARGO_TARGET_DIR=target cargo test
CARGO_TARGET_DIR=target cargo clippy --all-targets
cd web && npm run build && npm run test:map
```

沙箱内 git/子进程类测试可能假失败；完整 `cargo test` 在沙箱外跑。网络/代理：`git -c http.proxy= -c https.proxy=`。

### 0.3 分支与 PR

- 基线：`origin/master`（确认 S1 / #122 已合入；未合入则先合或基于 `int/s1-store`）。
- 分支名：`int/s2-analyze`、`int/s3-gate`、…（一步一个）。
- PR 标题：`feat(int): Sx …`；正文含 Red 证据、改动文件、完成定义输出摘要、超时/取消/清理说明（若有异步）。

---

## 1. S1 已交付（后续步骤必须复用，不要重写）

**状态**：✅（PR #122）

### 1.1 新表（已在 `SCHEMA`，勿再改列）

`introspection_jobs`、`introspection_findings`、`introspection_pushes`、`introspection_labels`、`app_state`。DDL 见总方案第 4 节；与代码一致。

### 1.2 `Store` API（`crates/runtime/src/store.rs`）

| 方法 | 行为要点 |
|---|---|
| `introspection_acquire_job(trigger, watermark_json, input_hash, lease_until) -> Option<Job>` | 事务内：有未过期 `running` 则 `None`，否则新建 |
| `introspection_finish_job(id, state, error) -> bool` | 置状态、写 `finished_at`、**清空租约** |
| `introspection_get_job` / `introspection_latest_watermark` | 水位**只看 `state=done`**，失败/取消不推进 |
| `introspection_fail_stale_jobs` | `running` 且 `lease_until <= now` → `failed`，error 含 `interrupted` |
| `introspection_insert_finding` | `ON CONFLICT DO NOTHING`；返回是否新写入 |
| `introspection_get_finding` / `list_findings(topic, limit)` | 列表按 `created_at DESC` |
| `introspection_set_gate` / `set_verdict` | 不存在返回 `false` |
| `introspection_add_push` | 同一 `finding_id` 只一次；已有则 `None` |
| `introspection_unread_count` / `mark_all_read` | `read_at IS NULL` 为未读 |
| `introspection_add_label` / `labels` | 按 `rowid` 顺序 |
| `app_state_get` / `set` | 通用 KV |
| `main_conversation_id` / `ensure_main_conversation(title)` | 键 `main_conversation_id`；唯一；事务内创建 |

类型：`crates/runtime/src/introspect/model.rs`（`IntrospectionJob` / `Finding` / `Label`）。

单元测试前缀：`store::tests::int_store_*`（11 个）。**不要删改这些测试。**

### 1.3 尚未交付（不要假设已有）

- `[introspection]` 配置段、调度循环、`SourceAdapter`、闸门、推送、`ChatCard::Introspection`、HTTP、前端角标。

---

## 2. S2 — 只读来源 + 方案采纳模式统计 + 校验器

**状态**：✅（PR 待开）  
**用例**：RT-INT-03、06、07、08（并入 `CASES.md`）  
**落点**（仅 `crates/runtime`）：

| 文件 | 动作 |
|---|---|
| `src/introspect/source.rs` | **新建** `SourceAdapter` + `MomSource` |
| `src/introspect/analyze.rs` | **新建** 采纳模式统计 |
| `src/introspect/finding.rs` | **新建** 稳定 ID、`FindingDraft`、校验器 |
| `src/introspect/mod.rs` | `pub mod source/analyze/finding` |
| `tests/introspection_analyze.rs` | **新建** RT 测试（或 `introspect` 模块内 `#[cfg(test)]`，但 RT 前缀函数必须能被 `check-map` 扫到：放在 `crates/*/tests/*.rs`） |

**不改**：`store.rs` 表结构、MOM 引擎、server、web。

### 2.1 类型与接口（照此实现，名称可多不可少）

```rust
/// 只读快照；内省不得持有 Engine 的 &mut。
pub trait SourceAdapter: Send + Sync {
    fn source_id(&self) -> &str; // "mom"
    /// 列出可用于统计的案例摘要（已从引擎拷贝出来的自有结构，不是 &Case）。
    fn list_case_summaries(&self) -> anyhow::Result<Vec<CaseSummary>>;
    /// 校验引用是否仍可解析（case_id 存在；可选事件序号）。
    fn resolve_ref(&self, case_id: &str, event_seq: Option<u64>) -> anyhow::Result<bool>;
}

pub struct CaseSummary {
    pub case_id: String,
    pub template: String,       // Case.template，分组键
    pub scenario: String,
    pub approvals: Vec<ApprovalSummary>,
    pub actions: Vec<ActionSummary>,
    pub event_times: Vec<(u64 /*seq*/, u64 /*at*/)>,
}

pub struct ApprovalSummary {
    pub id: String,
    pub scope: String,
    pub approver: String,
    pub expires_at: u64,
    pub plan_revision: u64,
}

pub struct ActionSummary {
    pub key: String,
    pub state: String,                 // 引擎里的 Action.state
    pub verification: Option<String>,  // 有值且表示通过时算「成功并验证」
    pub accepted_at: u64,
    pub complete_at: u64,
}

pub struct MomSource { /* 持 Arc 或只在调用时传入 &Engine；测试用 Engine::memory() */ }

/// 确定性统计结果（无解释层之前）。
pub struct AdoptionStats {
    pub template: String,
    pub approvals_total: u32,
    pub succeeded_verified: u32,
    pub action_failed: u32,
    pub approval_expired_unused: u32,
    pub latency_secs: Vec<u64>, // 从相关事件时间戳算出；可为空
    pub sample_size: u32,       // = approvals_total
    pub insufficient_sample: bool,
}

pub struct FindingDraft {
    pub topic: &'static str, // "adoption_pattern"
    pub scope: String,       // 如 template 名
    pub stats: AdoptionStats,
    pub evidence: Vec<EvidenceRef>, // case_id + 可选 seq
    pub verification: Verification, // Supported / Inconclusive / Contradicted
    pub hypothesis: String,         // 可为空；S2 可不写模型句
    pub next_step: String,          // 可为空
}

pub enum Verification { Supported, Inconclusive, Contradicted }

pub fn stable_finding_id(topic: &str, scope: &str, input_digest: &str) -> String;
// 建议：sha256 前 16 hex，前缀 "intf-"；同一输入必须稳定。

pub fn validate_draft(src: &dyn SourceAdapter, draft: &FindingDraft) -> ValidateOutcome;
pub enum ValidateOutcome {
    Accept(FindingDraft),
    Drop { reason: String },
    MarkInconclusive { draft: FindingDraft, reason: String },
}
```

### 2.2 统计规则（RT-INT-03，必须与断言一致）

对**每个** `template` 分组：

1. `approvals_total` = 该组所有 Case 的 `approvals` 条数之和。
2. `succeeded_verified` = 存在动作且 `state` 表示成功（建议认 `"succeeded"` / `"completed"` / `"done"`，以夹具实际写入为准，**夹具与实现用同一套字符串**）且 `verification` 为 `Some` 且不是失败语义。
3. `action_failed` = 动作最终失败（`state` 含 fail / failed / error）。
4. `approval_expired_unused` = 审批 `expires_at < engine.now()`（或夹具设定的「当前时间」）且该审批**从未**被任何成功动作使用。
5. 若 `approvals_total < min_samples`（S2 测试里写死 **10**，配置项到 S4/S5 再接；S2 函数参数传入即可）：`insufficient_sample = true`，`verification = Inconclusive`，假设/结论句只能是「样本不足」，**不得**写采纳率之类结论。
6. 耗时：用 `BoardEvent.at` 或动作 `accepted_at`/`complete_at`；公式在测试里写死一种（推荐：同一 case 上「审批事件时间 → 动作 complete_at」的差），实现必须与测试一致。

**RT-INT-03 夹具**：构造 **13** 个带审批的 Case（同一 `template`）：

| 数量 | 设定 |
|---|---|
| 9 | 审批在有效期内被用于执行，动作成功且 `verification = Some("ok")`（或同等） |
| 2 | 动作失败 |
| 2 | 审批已过期且未执行 |

期望统计：`approvals_total=13, succeeded_verified=9, action_failed=2, approval_expired_unused=2`。  
构造方式参考 `crates/runtime/tests/ops_desk.rs` 的 `pending_case` / `approve`（`Engine::memory()` + `EventInput::s1`）。若真实引擎路径难以精确造「过期未执行」，允许在**测试专用**的 `SourceAdapter` mock 上喂 `CaseSummary`（RT-INT-03 测分析与校验；另用一个小测试证明 `MomSource` 能从真实 `Engine` 只读拷出至少 1 个案例）。**生产代码路径禁止 mock。**

### 2.3 校验器（RT-INT-07、08）

1. 任一 `EvidenceRef` 经 `resolve_ref` 为 false → **整条 Drop**，记原因（RT-INT-08）。
2. 重新用同一输入跑统计，任一数字与 `draft.stats` 不一致 → Drop。
3. `insufficient_sample` 或证据不足 → `Inconclusive`，**不得**进入「可推送」集合（S2 用函数返回值区分即可；真正推送在 S5）。
4. RT-INT-07：人为造「证据不足」草稿 → `Inconclusive`。

### 2.4 只读边界（RT-INT-06）

- 测试：包一层 `RecordingSource` 或对 `Engine` 的只读包装；断言分析路径零写调用。
- 简单做法：`MomSource` 只调 `case_ids` / `case` / `events` / `now` / `sequence`；用类型系统不暴露 `&mut Engine`。测试里用 `static WRITE_COUNT` 在 mock 的写方法里 +1，跑完为 0。

### 2.5 Red → 并入 → Green

1. 写 `crates/runtime/tests/introspection_analyze.rs`：
   - `rt_int_03_adoption_pattern_matches_recomputed_stats`
   - `rt_int_06_analysis_makes_no_mom_writes`
   - `rt_int_07_insufficient_evidence_is_inconclusive`
   - `rt_int_08_unresolvable_evidence_is_dropped`
2. 空实现或 `todo!` / 返回中性值 → 跑出 **业务断言** Red，贴日志。
3. 并入 `docs/testing/CASES.md`（四行，Given/When/Then 从 ACCEPTANCE 抄，可略缩）、`features.json` 新增 feature `introspection`（或挂到现有 mom 相关 feature 的 `runtime_cases`——**推荐新建** `id: introspection`，`document` 可暂指 `docs/memory/INTROSPECTION_PLAN.md`，`coverage: runtime`，`cases: []`，`runtime_cases: ["RT-INT-03",...]`）。
4. 实现到 Green；`npm run test:map` 通过。

### 2.6 出口检查

- 四条 RT 绿；不改 S1 的 `int_store_*`。
- PR 描述写明：统计用的 `state`/`verification` 字符串约定。
- 更新本文件 S2 状态为 ✅、勾选 PLAN。

---

## 3. S3 — 发送闸门

**状态**：✅  
**用例**：RT-INT-16、17、18、19、20、22  
**落点**：`introspect/gate.rs`；测试 `crates/runtime/tests/introspection_gate.rs`  
**依赖**：S2 的 `FindingDraft` / `Verification`（可把 S2 输出映射为闸门输入结构）。

### 3.1 输入输出

```rust
pub struct GateInput {
    pub finding_id: String,
    pub verification: Verification,
    pub impact_cases: u32,          // 影响面
    pub has_next_step: bool,
    pub first_or_worsened: bool,    // 首次或明显恶化
    pub time_sensitive: bool,
    pub already_pushed_unread: bool,
    pub pushed_today: u32,          // 今日已推条数
    pub model_delta: i8,            // -1/0/1；无模型时为 0
    pub model_reason: String,       // model_delta != 0 时必填
}

pub struct GateConfig {
    pub push_threshold: f64,  // 4.0
    pub daily_cap: u32,       // 1
    pub impact_threshold: u32, // 初稿：影响案例数 >= 3 算「超门槛」（写进测试；可配置放到 S4）
}

pub struct GateDecision {
    pub score: f64,
    pub should_push: bool,
    pub reason: String, // 永远非空：推或不推都要写清
}
```

### 3.2 打分（与总方案第 7 节一致，测试写死）

| 条件 | 分 |
|---|---|
| `verification == Supported` | +2 |
| `impact_cases >= impact_threshold` | +1 |
| `has_next_step` | +1 |
| `first_or_worsened` | +1 |
| `time_sensitive` | +1 |
| `model_delta` ∈ {-1,0,1} | 原样加减；越界视为 0 并在 reason 记「非法 delta」 |

规则分 = 上表之和（含合法 model_delta）。  
`should_push` 仅当：

1. `score >= push_threshold`，且  
2. `verification == Supported`（Inconclusive/Contradicted **永不推**，即使模型 +1），且  
3. `!already_pushed_unread`，且  
4. `pushed_today < daily_cap`，且  
5. **模型不能单独把规则分 < threshold 的推出去**：先算 `rules_score`（不含 model），再加 delta；若 `rules_score < threshold` 且 `rules_score + delta >= threshold`，仍 **不推**，reason 写明「模型不能单独越阈」。  
6. 若 `rules_score >= threshold` 且模型想压到阈下：允许 `score` 减去 delta，但若结果 `< threshold` 导致不推，reason 必须带模型理由；**无理由的 delta 忽略**。

RT-INT-16：全不通过 → 全 `should_push=false`，每人有 reason。  
RT-INT-17：一条满足 Supported+影响面+下一步；一条 Inconclusive → 仅前者推。  
RT-INT-18：覆盖「模型不能单独越阈」与「无理由压分无效」。  
RT-INT-19：`already_pushed_unread=true` → 不推。  
RT-INT-20：`pushed_today >= daily_cap` → 不推，reason 含「日上限」与「另有 N 项」字样（N 用参数传入）。  
RT-INT-22：调用已有 `Store::introspection_add_label`；断言闸门配置对象**未被修改**（深比较或快照）。

S3 **只测纯函数 + label 落库**；不要起定时器、不要推消息。

### 3.3 并入与出口

测试函数：`rt_int_16_...` … `rt_int_22_...`。并入 CASES + features.json。出口同 §0.2。

---

## 4. S4 — 调度、租约、超时、取消、清理

**状态**：⏳  
**用例**：RT-INT-09、13、14、23（RT-INT-09 的**任务层**幂等；存储层幂等已由 S1 覆盖，本步测「跑两遍 job 不产生重复 finding」）  
**落点**：

| 文件 | 动作 |
|---|---|
| `introspect/job.rs` | **新建** 调度核心 |
| `introspect/mod.rs` | 装配 `run_once` |
| `config.rs` | 新增 `IntrospectionSection`（见下）——**已批准** |
| `lib.rs` `start_background` | 若 `enabled` 则 `tokio::spawn` 循环；**默认 false 时零行为** |

### 4.1 配置

```toml
[introspection]
enabled = false
schedule_hours = 24
timeout_secs = 120
min_samples = 10
push_threshold = 4
daily_cap = 1
operator_role = ""
```

```rust
pub struct IntrospectionSection {
    pub enabled: bool,
    pub schedule_hours: u64,
    pub timeout_secs: u64,
    pub min_samples: u32,
    pub push_threshold: f64,
    pub daily_cap: u32,
    pub operator_role: String,
}
impl Default for IntrospectionSection {
    fn default() -> Self { /* 与上表一致 */ }
}
```

挂到 `AppConfig`：`pub introspection: IntrospectionSection`，`#[serde(default)]`。

### 4.2 `run_once` 伪代码

```
fail_stale_jobs()
job = acquire_job(trigger, watermark, input_hash, now+lease_ttl)
if job is None: log job_skipped; return
select/timeout(timeout_secs):
  for theme in themes:  // 本期仅 adoption_pattern
    if cancel: finish cancelled; return
    drafts = analyze(...)
    for d in drafts:
      match validate(d):
        Accept(d) if !inconclusive -> insert_finding (稳定 ID); set_gate(decide(d))
        Drop -> log
  // S4 可不调用 push；推送在 S5。但 gate 结果要写入 store（set_gate）
finish done 或 failed(超时)
```

- 超时：`tokio::time::timeout`；状态 `failed`，error 含超时字样；**已 insert 的 finding 保留**；水位不推进（因非 done——注意：`finish_job("failed")` 后 `latest_watermark` 仍是上一次 done）。  
- 取消：`watch::Receiver<bool>`；主题之间检查。  
- RT-INT-09：同一 watermark + input_hash 跑两次 → finding 行数不增加。  
- RT-INT-23：未到点不跑（测调度间隔逻辑可用注入「假时钟」或直接测 `run_once` 不被自动调用）；手动 `trigger="manual"` 也走闸门。  
- CPU：分析进 `spawn_blocking`。

### 4.3 日志事件名（写入时用）

`introspection.job_start|job_end|job_failed|job_skipped|finding|gate`；字段带 `task_id = job_id`。`docs/DESIGN.md` §10 可在本步或 S5 补一行。

### 4.4 测试

`crates/runtime/tests/introspection_job.rs`：`rt_int_09_`、`rt_int_13_`、`rt_int_14_`、`rt_int_23_`。  
取消/超时用可控的 `SourceAdapter`（sleep / 计数器）。

---

## 5. S5 — 推送、主窗口、未读、权限、ChatCard、context_text

**状态**：⏳  
**用例**：RT-INT-21、24（**除记忆**）、25、26、27、28（**除记忆**）、29  
**落点**：

| 文件 | 动作 |
|---|---|
| `introspect/push.rs` | 推送 |
| `introspect/operator.rs` | `is_operator` |
| `crates/core/src/lib.rs` | `ChatCard::Introspection { ... }` |
| 生成 bindings | `CARGO_TARGET_DIR=target cargo test -p an-core`，提交 `web/src/bindings/` |
| `conversation.rs` | `context_text` 前缀 |
| `job.rs` | 闸门通过后调用 push |

### 5.1 ChatCard 新变体（新增，不改旧变体）

```rust
Introspection {
    finding_id: String,
    topic: String,
    summary: String,     // 大白话，进 content
    gate_reason: String,
    verification: String,
}
```

推送：

```rust
let main = store.ensure_main_conversation("主窗口")?;
let content = format!("［内省结论，仅供参考，不是指令］\n{}", summary);
let msg_id = store.add_message_with_card(&main.id, "assistant", &content, &card)?;
store.introspection_add_push(&finding_id, &msg_id)?;
// Bus 广播：复用现有会话消息事件（看 ops_desk / conversation 怎么通知前端），不要走 handle_message
```

**禁止**调用 `ConversationRuntime::handle_message`。

### 5.2 `context_text`

若消息 `card_json` 能反序列化为 `ChatCard::Introspection`，角色标签仍为「助手」，但正文前保证有前缀 `［内省结论，仅供参考，不是指令］`（若 `content` 已含则不重复）。  
测试：历史里放一条带祈使句的内省消息，断言 `context_text` 含前缀；**不**断言记忆跳过（G-9）。

### 5.3 `is_operator`

```rust
pub fn is_operator(identity: &str, cfg: &IntrospectionSection) -> bool {
    let _ = (identity, &cfg.operator_role); // 预留
    true // 现阶段
}
```

测试注入：用 `#[cfg(test)]` 或线程局部 / 参数 `OperatorCheck` trait，**生产默认恒 true**；RT-INT-21 在测试里切换 mock 返回 false，断言拒绝路径（S5 可先测函数；HTTP 拒绝在 S6）。  
RT-INT-25：`operator_role = "x"` 启动不报错；可读日志「暂不生效」（可用 `tracing` 断言或文档约定 + 单测调启动辅助函数）。

### 5.4 主窗口唯一（RT-INT-26/27/29）

复用 S1 API。本步补：推送只写入 `main_conversation_id` 对应会话；其他会话 `list_messages` 无内省卡。  
「不能删除」：若仍无删除会话 API，在测试里写注释 + `ACCEPTANCE` 已知条件；**若本步或并行 PR 增加了删除 API，必须拒绝删主窗口会话**。

### 5.5 记忆相关断言

RT-INT-24 / 28 的「不被抽取进个人记忆」：**跳过**，在测试名或注释标 `G-9`；只断言推送形态与 `context_text` 前缀。

---

## 6. S6 — HTTP + 前端

**状态**：⏳  
**用例**：E2E-INTROVIEW、INTROLABEL、INTROPUSH、INTROUNREAD；RT-INT-10  
**落点**：`crates/server/src/`（新路由模块或挂现有 router）、`web/src/`（角标 + 内省页最小 UI）。

### 6.1 API（与总方案第 8 节一致；返回风格跟现有 API，不要发明第二套 envelope）

先读一个现有 handler（如 admission）的返回形状，照抄。

| 方法 | 路径 | 鉴权 |
|---|---|---|
| GET | `/api/introspection/findings` | `is_operator` |
| GET | `/api/introspection/findings/{id}` | 同上 |
| POST | `/api/introspection/findings/{id}/verdict` | body: `{verdict, note}` |
| POST | `/api/introspection/findings/{id}/label` | body: `{kind, note}` |
| POST | `/api/introspection/run` | 手动 `run_once` |
| GET | `/api/introspection/unread` | |
| POST | `/api/introspection/read` | 全标已读 |

非运营（测试 mock）→ 403/401，**body 不含条数与标题**（RT-INT-21 的 HTTP 侧）。  
RT-INT-10：否决后 `verdict=rejected`；再跑分析，稳定 ID 相同的 finding **不因否决而删除**，但「同类不再重复生成」= insert 幂等；**规则/闸门配置不变**。

### 6.2 前端最小

- 会话列表：主窗口会话显示未读角标（读 `/unread`）。
- 打开主窗口：调 `/read`。
- 内省页：列表、原因、否决、label；无入口给「非运营」（前端可先不做角色，靠 API）。
- Playwright：`web/e2e/` 新文件，用例名与 E2E-INTRO* 对应；登记 `features.json` 的 `cases`。

G-6：无「个人」页签 → 角标挂会话列表即可；E2E-INTROUNREAD 的「个人页签」措辞在实现时映射为「会话列表主窗口入口」，若需改 ACCEPTANCE 浏览器步骤的**表面措辞**，先问用户（冻结的是业务断言，不是每一个 UI 词）。

---

## 7. S7 — DES 真值评测 + 可选模型解释

**状态**：⏳  
**用例**：RT-INT-15；另交一份评测说明（不必是 RT）  
**要点**：

- 模型解释默认 **关**；开时用后台槽位，取不到则跳过并 `introspection.job_skipped`（原因：无槽位），确定性结论仍在。
- 模型**禁止**改 `AdoptionStats` 数字；输出只追加 `hypothesis` 文本字段。
- DES：用 `simulation.ground_truth` / `des_truth` **只读**核对「植入问题是否被统计到」；脚本可放 `evals/` 或 `docs/memory/eval.md`。

---

## 8. S8 — 同因复发（先核实）

**状态**：⏳  
**用例**：RT-INT-02（核实后可能重写，**重写前必须用户确认**）

核实清单（写进 PR，先于代码）：

1. `Case.eight_d` / `proposal` / `des_reviews.claimed_causes` 能否稳定取出根因？
2. 有没有 `regressed`？没有则只做「同 `asset` / 同 `batch` 反复出现」描述性统计，并在结论标明「非根因」。
3. 更新 G-10 与 ACCEPTANCE 中 RT-INT-02 行，再实现。

---

## 9. 并入 CASES.md / features.json 配方

### 9.1 CASES.md 行格式

与现有表一致：

```markdown
| RT-INT-03 | introspection | （Given 缩写） | （When） | （Then） |
```

### 9.2 features.json 条目模板

```json
{
  "id": "introspection",
  "document": "docs/memory/INTROSPECTION_PLAN.md",
  "sources": [
    "crates/runtime/src/introspect/",
    "crates/runtime/src/store.rs"
  ],
  "coverage": "backend",
  "cases": [],
  "runtime_cases": ["RT-INT-03", "RT-INT-06", "RT-INT-07", "RT-INT-08"]
}
```

S6 起把 E2E 填进 `cases`，`coverage` 可改为 `browser` 或保持双列。

### 9.3 check-map

```bash
cd web && npm run test:map
```

失败常见原因：RT 行已写入但无 `rt_int_03_` 函数；E2E ID 含数字；features 与 CASES 不一致。

---

## 10. 已知缺口（实现时怎么处理）

| 编号 | 实现策略 |
|---|---|
| G-1/G-2 | 不做频率健康度；不要用驳回率冒充误报 |
| G-3 | 闸门数字用配置；靠 label 积累，不自动改规则 |
| G-4 | `is_operator` 恒 true；测试用注入 |
| G-5 | **已由 S1 `ensure_main_conversation` 解决指定方式**；「不可删除」等删除 API 出现时补守卫 |
| G-6 | 角标挂会话列表 |
| G-7/G-8 | 主题暂缓 |
| G-9 | 测试跳过记忆断言 |
| G-10 | S8 核实 |

---

## 11. 弱 agent 自检清单（提交前）

- [ ] 只实现了当前一步  
- [ ] Red 日志是业务断言，不是编译错误/环境错误  
- [ ] 未改冻结用例断言、未删 S1 `int_store_*`  
- [ ] 未写 MOM、未改 `orchestrator/registry/acp/util`  
- [ ] `enabled` 默认 false（若本步已加配置）  
- [ ] CASES + features + test:map 一致  
- [ ] fmt / test / clippy / web build 已跑并贴摘要  
- [ ] PLAN.md 本步已勾选；本 playbook 状态已更新  
- [ ] 一步一 PR，基线为最新 master  

---

## 12. 进度速查

| 步 | 状态 | PR |
|---|---|---|
| S0 | ✅ | #107 |
| S1 | ✅ | #122 |
| S2 | ✅ | （本 PR） |
| S3 | ✅ | （本 PR） |
| S4 | ⏳ | — |
| S5 | ⏳ | — |
| S6 | ⏳ | — |
| S7 | ⏳ | — |
| S8 | ⏳ | — |
