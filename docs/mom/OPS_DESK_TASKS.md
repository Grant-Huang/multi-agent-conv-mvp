# 运营桌面 D1–D3 执行任务卡（给接手的 agent）

本文件是 [OPS_DESK.md](OPS_DESK.md)（设计）的**执行版**：每张任务卡写清要改哪些文件、接口长什么样、先写哪个测试、跑哪些命令、哪里容易踩坑。照卡片顺序做，不要跳步，不要自己扩范围。
设计取舍见 [DECISIONS.md](../DECISIONS.md) 第 56–58 条；通用规则见根目录 `AGENTS.md`。

## 0. 进度总表（每做完一张卡就改这里，并同步 `docs/PLAN.md` 的“运营桌面与运营频道进度”）

| 卡片 | 内容 | 状态 | PR |
|---|---|---|---|
| D0a | 事件分级、发布器、`[mom]` 配置、`ensure_conversation`、`ChatCard::OpsCase/OpsSummary` | 已完成 | #118 |
| D0b | 发布器接线、未读接口、频道卡片与角标 | 已完成 | #121 |
| **D1-1** | 运行时：桌面读模型 `desk_view` + `OpsPublisher::open_case`（RT-OPSDESK-06/07） | 已完成 | 本 PR |
| **D1-2** | 服务端：`GET /api/mom/desk`、`GET /api/mom/desk/cases/{id}`、`POST …/open-in-channel` | 已完成 | 本 PR |
| **D1-3** | 前端：“运营桌面”页签、看板、筛选、详情抽屉（E2E-OPSDESK/OPSDRAWER/OPSNOISE 转绿） | 已完成 | 本 PR |
| **D2** | 卡片上批准（决定 60） | 已完成 | 本 PR |
| D3 | 任务/资源入口、MCP 连接器回归（需先与用户确认范围） | 未开始 | — |

一张卡 = 一个 PR。做完先在 PR 里贴“完成的定义”命令的真实输出，等用户确认合并，再开下一张。

## 1. 开工前必读的现状（已核对代码，2026-11-19）

- **发布器**：`crates/runtime/src/mom/ops_desk.rs`，对外 API：`OPS_CHANNEL_ID`、`OpsLevel{P2,P1,P0}`、`classify(kind, stage)`、`OpsConfig{merge_secs,hourly_cap}`、`OpsPublisher::{memory, open, tick(engine, now_secs), unread, mark_read, skip_log}`。发布器状态在 `<db_path>.ops.db`，**不是业务状态**，丢了可由引擎事件重放。
- **服务端接线**：`crates/server/src/mom_ops.rs`（`spawn_publisher` 每 1 秒 tick；`router(publisher)` 提供 `/api/mom/desk/unread`、`/api/mom/desk/read`）；`crates/server/src/main.rs` 只在 `[mom] chat_bridge = true` 且 MOM 挂载成功（仅本机回环）时才创建发布器并 `merge` 路由。**桌面接口也必须走这个开关**：开关关闭时 `/api/mom/desk*` 一律 404（E2E-OPSOFF 已断言）。
- **引擎句柄**：`an_server::mom::AppState`（`crates/server/src/mom.rs`）里是 `Arc<Mutex<Engine>>`，用现成的辅助函数 `run(&s, |e| …)` 加锁调用；`list` 处理器给出了“遍历 `e.case_ids()` 再 `e.human_view(id)`”的写法，桌面读模型照此取数。D1-2 要把引擎句柄也传给 `mom_ops::router`（现在只传 `publisher`），需同步改 `main.rs` 的一行调用。
- **引擎时间**：`e.now()` 是**仿真分钟**，不是墙钟。截止时间 `deadlines`、`resource_observed_at` 都是仿真分钟；发布器的 `now_secs` 是墙钟秒，两者不要混用。
- **`human_view` 的字段**：不要凭记忆写，先 `curl -s localhost:18791/mom/api/mom/cases/<id>` 看一份真实输出（见 §6 起实例的方法）。已知包含 `projection_seq`、`plan_revision`、`stage`。
- **案例阶段字符串**：`ASSESS`、`WAITING_HUMAN`、`BLOCKED`、`VERIFYING`、`CLOSED`、`CANCELLED`、`BUDGET_EXHAUSTED`、`INVESTIGATING`。
- **SLA 字段**：案例上有 `alerts`（预警键，如 `initial:60`）和 `breaches`（已超时的种类，如 `initial`），由引擎在 `advance` 时写入。
- **证据过期规则**：引擎批准时检查 `now <= resource_observed_at + fixture_policy(scenario).snapshot_ttl_minutes`，否则报 `evidence snapshot stale; refresh plan`。`fixture_policy` 在 `crates/runtime/src/mom/model.rs`，公开。
- **前端布局**：`web/src/App.tsx` 是三栏（左：会话列表，中：`<main className="main">`，右：`<aside className="side">`）。运营频道、未读（`opsUnread`、`opsViewed`、2 秒轮询）已在 App.tsx 中。桌面页签放在 `main` 的顶部，不要动右侧栏。
- **绑定类型**：`web/src/bindings/` 由 `an-core` 生成，不能手改。D1 **不需要**改 `an-core`（桌面数据用 JSON，类型写在 `web/src/api.ts` 里）；如果你发现要改，先停下来问。

## 2. 通用流程（每张卡都照做）

1. `git -c http.proxy= -c https.proxy= fetch origin`（直连优先；直连失败再用 git 配置里的代理 `127.0.0.1:7897`），`git switch -c feat/<卡片名> origin/master`。
2. `git status`、`git diff` 确认没有别人的未提交改动；本仓库根目录下的 `AgentNexus_统一外发脱敏服务_技术方案.md`、`evals/reports/`、`test_data_set/` 是用户的未跟踪文件，**不要提交、不要删除**。
3. 先写测试（Red）→ 看到**业务断言**失败（编译错误、端口占用、浏览器没装都不算 Red）→ 把输出写进 `docs/testing/evidence/<卡片名>.md` → 再实现（Green）。
4. 冻结的断言（`web/e2e/ops.spec.ts`、`crates/runtime/tests/ops_desk.rs` 已有部分）**不能改**；确认断言有误时先向用户说明原因，等“允许修改测试逻辑”。
5. 同步更新：`docs/testing/CASES.md`（草稿 bullet 改成表格行并写测试名）、`docs/testing/features.json`（`ops-desk` 的 `runtime_cases` / 来源文件）、`docs/PLAN.md` 进度、本文件 §0。
6. 完成的定义（全部要在回复里贴输出）：
   ```bash
   cargo fmt --all -- --check
   CARGO_TARGET_DIR=target cargo test --no-fail-fast
   CARGO_TARGET_DIR=target cargo clippy --all-targets
   cd web && npm run build && npm test && npm run test:map
   cd web && PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts
   ```
   已知本机环境问题：`an-server --test admission::rt_adm_14_admission_http_end_to_end`（“任务未结束”）在干净 master 上本机也失败、CI 上通过，不要为它改代码；贴输出时注明。
7. 提交用普通 `git commit`（让钩子跑），**不要**用 `-c core.hooksPath=…` 或 `--no-verify` 绕过钩子。
8. 推送直连优先：`git -c http.proxy= -c https.proxy= push -u origin <分支>`。开 PR：`gh pr create`，PR 描述写清范围、测试证据、风险。
9. 新增决策只能追加到 `docs/DECISIONS.md` 末尾，**编号取文件里最大编号 +1，推送前再 `fetch` 一次确认没有被别的会话占用**（本项目发生过多次撞号，改号时要连带改引用它的文档、测试注释）。

## 3. D1-1：运行时读模型与“在频道中处理”

**目标**：把“桌面看到什么”做成引擎之上的**纯函数**，可单测；再提供“打开案例到频道”的幂等操作。

**改动文件**
- 新增 `crates/runtime/src/mom/ops_view.rs`，在 `crates/runtime/src/mom/mod.rs` 加 `pub mod ops_view;`（照 `ops_desk` 的声明方式）。
- 修改 `crates/runtime/src/mom/ops_desk.rs`：新增 `OpsPublisher::open_case`。
- 新增用例到 `crates/runtime/tests/ops_desk.rs`（文件名保持不变，函数名 `rt_opsdesk_06_…`、`rt_opsdesk_07_…`，check-map 靠这个命名识别）。
- 不改 `engine.rs`、`store.rs`（高风险，需用户批准）。

**接口（先照抄签名，再实现）**
```rust
// ops_view.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeskRow {
    pub case_id: String,
    pub title: String,          // 与运营卡同一个标题：把 ops_desk.rs 里 card_for 的 format!("{what}：{asset}") 抽成 pub(crate) fn case_title(&Case)，两处共用，不要复制
    pub stage: String,          // 引擎原样
    pub stage_label: String,    // 中文标签，见下表；未知阶段 = "未知状态（{stage}）"
    pub group: String,          // "pending" | "blocked" | "overdue" | "risk" | "active" | "completed" | "unknown"
    pub owner: String,          // 固定 "调度"（与 ops_desk::card_for 一致，直接复用其常量/函数，不要再写一份）
    pub deadline_minutes_left: Option<i64>, // 近似值：`case.deadlines` 里 kind 不在 `case.breaches` 中的最小截止分钟 − now；无则 None。引擎没有公开“是否已满足”的判断，所以这里只是近似，用例不要断言它的具体数值，只断言排序稳定
    pub stale: bool,            // 活跃案例且 now > resource_observed_at + snapshot_ttl_minutes
    pub provenance: &'static str, // 恒为 "simulation"
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeskView {
    pub mode: &'static str,     // "simulation"
    pub now: u64,               // 引擎仿真分钟
    pub state: &'static str,    // "no_data"（零案例）| "ok"
    pub strip: DeskStrip,
    pub rows: Vec<DeskRow>,     // 已排序，**包含**已完成案例（折叠由前端做）
}
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeskStrip { pub pending: usize, pub blocked: usize, pub overdue: usize, pub risk: usize, pub completed: usize }

pub fn desk_view(engine: &Engine) -> anyhow::Result<DeskView>;

// ops_desk.rs（OpsPublisher 上）
/// 把案例卡发到运营频道（用户在桌面点“在频道中处理”）。已有卡就返回已有消息 id，不重复发。
/// 绕过小时上限与合并窗口（是用户主动动作），但仍写入 ops_cases 记录，之后的 tick 照常就地更新这张卡。
/// 案例不存在返回 Err，错误信息以 "E_CASE_NOT_FOUND" 开头。
pub fn open_case(&self, engine: &Engine, case_id: &str, now_secs: u64) -> anyhow::Result<String>;
```

**分组规则（按顺序判断，先命中先用）**
| 条件 | group | stage_label |
|---|---|---|
| stage = `WAITING_HUMAN` | pending | 待批准 |
| stage ∈ {`BLOCKED`,`BUDGET_EXHAUSTED`} | blocked | 受阻 / 预算耗尽（`BUDGET_EXHAUSTED` 行文本里也要含“受阻”二字，可写成“受阻：预算耗尽”） |
| stage ∈ {`CLOSED`,`CANCELLED`} | completed | 已关闭 / 已取消 |
| `case.breaches` 非空 | overdue | 已超时 |
| `case.alerts` 非空 | risk | 有超时风险 |
| stage ∈ {`ASSESS`,`VERIFYING`,`INVESTIGATING`} | active | 评估中 / 核验中 / 调查中 |

已知阶段的标签直接调用 `ops_desk.rs` 里现有的 `stage_label(stage)`（现为私有 `fn`，改成 `pub(crate) fn`；它的映射：待批准/受阻/核验中/已关闭/已取消/预算耗尽/调查中，其余返回“评估中”）。运营卡与桌面必须用同一个函数，标签才不会不一致。`stage_label` 把未知阶段也当“评估中”，所以**未知阶段要先在 `ops_view.rs` 里单独判断**（不在上表前 7 类阶段、也不是 ASSESS 的，走 unknown 分支）。上表 `CLOSED` 的标签以 `stage_label` 的“已关闭”为准，不另写“已完成”。
| 其他 | unknown | 未知状态（{stage}） |

> 注意：`stage_label` 里“受阻”二字必须出现在受阻行，E2E-OPSDESK 用 `toContainText('受阻')` 断言第二行。

**排序**：`group` 优先级 pending < blocked < overdue < risk < active < unknown < completed；同组按 `deadline_minutes_left` 升序（`None` 排最后），再按 `case_id` 升序，保证结果确定。

**空数据与未知**：零案例时 `state="no_data"`，前端显示“暂无数据”，**不能**显示成“一切正常”；未知阶段走 `unknown`，不要 panic、不要归入 active。

**测试（先写 Red）**
- `rt_opsdesk_06_open_in_channel_is_idempotent`：造一个待批准案例，**不调用 tick**，调用 `open_case` 两次 → 两次返回同一个消息 id；频道里该案例只有一张卡；再 `tick` 后消息条数不变；对不存在的案例返回 `E_CASE_NOT_FOUND`。
- `rt_opsdesk_07_desk_view_matches_engine`：用 `pending_case` 夹具造 1 个待批准；受阻案例用 `engine.ingest(EventInput{ id:"B-trip-0", scenario:"s1", kind:"vibration", source:"EAM", occurred_at:900, batch:"B", customer:"A", asset:"CNC-XX", payload: json!({"line":"Line-B","safety_trip":true}) })` 造（载荷取自 `web/e2e/ops.spec.ts` 的 `trip()`，`tests/ops_desk.rs` 里目前没有现成受阻夹具，可顺手加一个 `blocked_case(e, batch)` 辅助函数）；已关闭案例照 E2E 的 `seedClosed` 顺序（approve → execute → advance 5 分钟 → close）；断言：`strip` 计数与引擎阶段一致；`rows[0]` 是待批准、`rows[1]` 是受阻、已完成最后；每行 `provenance == "simulation"`；零案例引擎 `state == "no_data"`；`rows` 里的每个 `case_id` 都能在 `engine.case_ids()` 里找到（桌面与引擎一致）。
- 夹具时间坑：`pending_case(e, batch)` 用 `e.now().max(497)` 作基准，事件时间 base / base+6 / base+14 后 `advance(base+17)`；**批准要在证据过期前做**；不要写死绝对时间，否则第二个案例进不了 WAITING_HUMAN。
- Red 的写法：先加空实现（`desk_view` 返回空 `rows`、`open_case` 返回 `Ok(String::new())`），确认断言失败再实现。

**验收**：`cargo test -p an-runtime --test ops_desk` 全绿（现有 7 个 + 新 2 个）；`npm run test:map` 通过。

## 4. D1-2：服务端接口

**改动文件**：`crates/server/src/mom_ops.rs`（加三条路由）、`crates/server/src/main.rs`（`mom_ops::router` 多传引擎句柄）、新增用例到 `crates/server/tests/`（参照该目录已有 `mom_*` 测试的建服务方式；如无现成辅助，先读 `crates/server/tests/` 里最近的一个文件再照写）。

| 方法与路径 | 请求 | 成功响应 | 失败 |
|---|---|---|---|
| `GET /api/mom/desk` | 无 | 200，直接返回 `DeskView` 的 JSON（不加 `status/data` 外层）。现有 `mom.rs` 与 `mom_ops.rs` 的接口都是裸 JSON（如 `{"unread":n}`），E2E 的 `unwrap = j => j.data ?? j` 两种都兼容；保持与 `mom_ops.rs` 一致，别自造格式 | 开关关闭时路由根本不存在 → 404 |
| `GET /api/mom/desk/cases/{id}` | 无 | 200，就是 `engine.human_view(id)`（含 `projection_seq`） | 案例不存在 → 404，`message` 含案例 id |
| `POST /api/mom/desk/cases/{id}/open-in-channel` | 空体 | 200，`{"message_id":"…","conversation_id":"ops-channel"}` | 案例不存在 → 404 |

- 墙钟秒用 `mom_ops.rs` 里现成的私有函数 `now_secs()`，同文件内直接调用。
- `mom_ops.rs` 现有的 `fail()` 一律回 500；“案例不存在”要回 404：新增一个 `not_found(id)` 辅助函数（返回 `StatusCode::NOT_FOUND` 和 `String`），**不要**改动现有 `fail`。路由状态现在是 `SharedPublisher`，改成一个结构体 `OpsState{engine, publisher}` 并 `#[derive(Clone)]`，两条旧路由的 `State` 提取同步改。
- `open-in-channel` 要持有发布器与引擎两把锁：**固定先锁引擎再锁发布器**（或照 `spawn_publisher` 里的顺序），避免死锁。
- 三条接口都**只读或只发卡**，不得调用 `approve/execute/close`（桌面不能批准）。
- 用例：`RT-OPSDESK-10`（新编号，写进 CASES.md 表格并补 features.json）：开关开 → `/api/mom/desk` 200 且 `rows` 与 `/mom/api/mom/cases` 的案例集合相同；`open-in-channel` 两次返回同一 `message_id`；不存在的案例 404。已有 E2E-OPSOFF 覆盖“开关关 → 404”，不用重复写。

## 5. D1-3：前端桌面

**必须满足的页面契约**（全部来自冻结的 `web/e2e/ops.spec.ts`，改前先再读一遍该文件的 `openDesk`、`rows`、`strip` 等辅助函数）：

| 元素 | 契约 |
|---|---|
| 页签 | `role="tablist"` 里有 `role="tab"`，名称含“运营桌面”；旁边还有“会话”页签。**仅当运营桌面开关开启时渲染**（用 `api.opsUnread()` 能返回数据判断开关，或新增 `/api/mom/desk` 探测；关闭时 404 → 不渲染，E2E-OPSOFF 要求页签数量为 0） |
| 状态条 | `data-testid="ops-strip"`，文本含 `待批准 N`、`受阻 N`（还可含 `超时 N`、`风险 N`） |
| 行 | 每案例一行 `data-testid="ops-row"`，`tabIndex={0}`；文本含案例 id、阶段标签、“模拟”徽标；**行内只能有一个 `button`**（待批准行是“在频道中处理”，其余行是“查看详情”）；按钮放在行最右侧，因为测试对整行 `click()` 点的是行中心，必须落在空白或文字上、不能落在按钮上；按钮的 `onClick` 要 `stopPropagation` |
| 筛选 | 一组带 `aria-pressed` 的按钮：“全部”“待我批准”“受阻/需协助”“风险提示”；选择写入 `localStorage`，刷新后保持（用例刷新后仍断言“待我批准”为按下） |
| 已完成 | 默认折叠；折叠按钮名称形如“已完成 (N)”，展开后才渲染这些行；折叠时 `ops-row` 里不能出现已完成案例 |
| 禁止 | 桌面与抽屉里**不得出现**名称为“批准”“批准执行”的按钮 |
| 抽屉 | `role="dialog"`、`aria-label="事项详情"`；六个 `h3`（标题逐字：发生了什么与影响、责任、方案与前提、缺什么、进展与下次反馈、凭什么算解决）；底部 `<details><summary>原始数据</summary><pre>{JSON.stringify(detail)}</pre></details>`，**折叠时 `projection_seq` 文字必须不可见**（`details` 原生折叠即可）；`Esc` 关闭并把焦点还给触发它的那一行（打开时记住 `document.activeElement` 或行的 ref，关闭时 `.focus()`） |
| 数据 | 行点击 → `GET /api/mom/desk/cases/{id}` → 抽屉；“在频道中处理” → `POST …/open-in-channel` → 切回“会话”页签并选中 `ops-channel` 会话 |

**抽屉六段内容的取值**（先看真实 `human_view` 输出，字段缺失一律显示“暂无”，不要报错，不要编造）：
1. 发生了什么与影响：阶段标签、场景/客户/批次/设备等案例描述字段、已知约束。
2. 责任：负责人（“调度”）、待谁批准、`decision_inbox`（若有）。
3. 方案与前提：当前方案摘要（`proposal` 里能读到的摘要字段）、方案版本 `plan_revision`、前提/限制。
4. 缺什么：按阶段给出人话——`WAITING_HUMAN`“等待批准人决定”；`BLOCKED`“安全联锁生效，需先解除”；有 `alerts/breaches` 则列出；都没有写“暂无”。
5. 进展与下次反馈：已完成/进行中的步骤数、最近一次截止点。
6. 凭什么算解决：核验要求（“核验证据齐全后才能关闭”）与已有证据数；全页顶部再标“模拟数据”。

**文件**
- 新增 `web/src/components/OpsDesk.tsx`（看板 + 抽屉两个组件，保持一个文件；超过约 300 行再拆 `OpsDrawer.tsx`）。
- 修改 `web/src/api.ts`：`opsDesk()`、`opsDeskCase(id)`、`opsOpenInChannel(id)`，返回类型在本文件里手写 interface（不要碰 bindings）。
- 修改 `web/src/App.tsx`：新增状态 `view: "chat" | "desk"`（默认 `chat`），顶部渲染 tablist；`view === "desk"` 时 `main` 里渲染 `<OpsDesk onOpenChannel={…}/>` 代替 `<ChatView/>`；桌面每 5 秒轮询 `opsDesk()`。
- 修改 `web/src/styles.css`：追加 `.ops-desk`、`.ops-strip`、`.ops-row`、`.ops-drawer` 样式；颜色沿用现有 `.badge`、`.dot` 变量，P0 行用 `level-p0` 的同一套红色。
- 不改 `ChatView.tsx`（除非 `open-in-channel` 要滚动到卡片：用 `document.getElementById` 之类最小办法，先不做）。

**测试**：E2E 已冻结，只能让它们转绿：`E2E-OPSDESK`、`E2E-OPSDRAWER`、`E2E-OPSNOISE`（桌面部分：13 行，每个案例 id 各出现一次）。另可补 vitest 单测（`web/src/**/*.test.tsx`）：分组与排序不在前端做（后端已排好），前端只测筛选和折叠逻辑。

**跑浏览器用例的坑**
- 必须加 `PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright`，否则找不到 Chromium（环境问题，不是 Red）。
- `ops.spec.ts` 自己构建 `agentnexus` 并在 **18791** 端口为每个用例起一个新实例（`[mom] chat_bridge=true`、`ops_merge_secs=1`、`ops_hourly_cap=10`，模型地址指向死端口）；默认 harness（18787）没有 `[mom]`，桌面功能是关的。端口被占用时先 `lsof -i :18791` 再处理，别改端口。
- 构建 web 用临时目录的 vite dist，`npm run build` 失败会让 E2E 起不来。

**验收**：5 个 E2E 全绿（OPSDESK、OPSDRAWER、OPSCHANNEL、OPSNOISE、OPSOFF）；`docs/testing/evidence/ops-desk-d1.md` 写 Red→Green；CASES.md 里 RT-OPSDESK-06/07/10 变表格行；`docs/PLAN.md` D1 打勾并写日期。

## 6. 手动查看真实数据（找字段时用）

```bash
# 起一个开了桥的实例（照 web/e2e/ops.spec.ts 里的 config 与端口 18791）
curl -s -X POST localhost:18791/mom/api/mom/scenarios/s1          # 造一个待批准案例，返回案例 id
curl -s localhost:18791/mom/api/mom/cases | python3 -m json.tool   # 列表
curl -s localhost:18791/mom/api/mom/cases/<id> | python3 -m json.tool
curl -s localhost:18791/api/mom/desk/unread                         # 运营频道未读
```

## 7. D2：卡片上批准（**已确认**，决定 60）

用户确认（2026-11-20）：按钮在运营频道 `OpsCase`；卡片携带 `scope`/`plan_revision`；重复点击 `200+already:true`；本期不做驳回。

### 7.1 公开类型（改 `an-core`，必须重生 bindings）

`ChatCard::OpsCase` 增加：
```rust
scope: Option<String>,         // s1 → Some("production")；s2 待首响 → Some("initial_response")；其它 None
plan_revision: Option<u64>,  // 发卡时的 case.plan_revision
```
改完后跑：`CARGO_TARGET_DIR=target cargo test -p an-core`（生成 `web/src/bindings/`），**一起提交**。

`card_for` / `open_case` 发的卡必须填这两个字段；已批准后 `approver` 有值，前端不再显示批准按钮。

### 7.2 服务端

`POST /api/mom/desk/cases/{id}/approve`，body：`{"scope":"production","plan_revision":1}`（空字段不要）。
- 成功：`200 {"already":false,"approver":"supervisor"}`，并**立刻**用 `update_message_card` 刷新频道里该案例卡（不要只靠 1 秒 tick）。
- 已批准（同 scope + 同 plan_revision）：`200 {"already":true,"approver":"…"}`，不调用会重复生效的逻辑（可跳过 `Engine::approve`）。
- 方案/证据过期（引擎报 `stale plan` / `evidence snapshot stale`）：**409**，正文含原因。
- 案例不存在：**404**。
- 操作人固定 `supervisor`，**不要**从请求体读 actor。
- 锁顺序：先引擎后发布器。不得改 `engine.rs`。

在 `OpsPublisher` 上加 `refresh_case(engine, case_id) -> Result<()>`：有 message_id 则 `update_message_card`，没有则 Ok(())。

### 7.3 前端

`OpsCards.tsx`：`WAITING_HUMAN` 且 `!approver` 且 `scope`/`plan_revision` 齐全时显示「批准」按钮（`exact` 名「批准」）；点击调 `api.opsApprove(caseId,{scope,plan_revision})`，进行中禁用；成功后用返回的 approver 更新本地展示或触发消息刷新；409 时提示「方案已更新，请刷新」。
`api.ts` 增加 `opsApprove`。桌面（`OpsDesk`）**不得**出现批准按钮（E2E 已有断言）。

### 7.4 用例（冻结后只改业务代码）

| 用例 | 断言 |
|---|---|
| RT-OPSDESK-11 | 服务端：待批准案例 approve → 200 already=false；再调一次 already=true；错误 plan_revision → 409；不存在 → 404；批准后频道卡 approver=supervisor |
| E2E-OPSAPPROVE | 桌面「在频道中处理」→ 卡上点「批准」→ 卡显示 supervisor；桌面无「批准」按钮；`GET /mom/api/mom/cases/{id}` 的 decision_inbox 显示已批 |

写进 `CASES.md` 表格 + `features.json`；证据 `ops-desk-d2-red.md` / `ops-desk-d2.md`。

### 7.5 本卡完成定义

与第 2 节相同，另加：`PLAYWRIGHT_BROWSERS_PATH=… npx playwright test ops.spec.ts` 含新 E2E-OPSAPPROVE 共 6 个全绿。

## 8. D3：任务/资源入口与连接器回归（先问用户，再动手）

依据 [UI_UX_PRINCIPLES.md](UI_UX_PRINCIPLES.md) 的“三入口（看板/任务/资源）”，D1 只做了“看板”。D3 需要用户先确认：任务入口展示什么（执行中的案例步骤？待我处理的人工任务？）、资源入口展示什么（DES 设备/物料状态，来源 `FactoryService`）。确认后拆卡。另一条线是 WP1-L4（经 `an-des-mcp` 替换进程内 DES 调用），见 [WP1_TASKS.md](../mom/WP1_TASKS.md)。

## 9. 已知局限（做的时候别“顺手修”，在 PR 里如实写）

- 运营卡的消息正文是固定文本“[模拟] 运营案例 {id}（详情见卡片）”，因为 store 没有“更新消息正文”的接口；内容全在卡片 JSON 里。
- 仓库里没有“个人记忆抽取”模块，“不进入记忆”目前只靠“内容在结构化卡片里”保证。
- 进程内没有纯遥测（P2）事件源，P2 路径由 `classify` 的单测覆盖。
- E2E-OPSNOISE 在 D1 完成前一直失败在“点击运营桌面页签”这一步，属预期；不拆分该用例（用户未要求）。
