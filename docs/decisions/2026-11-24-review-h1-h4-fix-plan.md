# 审查 R-20261124-H1～H4（含 H5）修复方案：服务端批准登记、汇合只建一次、事件写入去重与夹具开关

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-24 |
| 来源 | 用户确认（2026-11-24 对话：三个待决项“都按推荐，交给对应的工作线”） |
| 状态 | 有效（方案已定；实现分三张任务卡，未开工） |
| 影响范围 | World Adapter（`crates/server/src/mom_world_des.rs`、RT-WADES、`web/e2e/wa-des-write.spec.ts`）；执行引擎 WP4（`crates/runtime/src/orchestrator.rs`、`store.rs`）；AI 桌面夹具与事件写入（`crates/server/src/desktop_api.rs`、`store.rs`、`config.toml`） |
| 相关设计 | [contracts/identity-and-approval.md](../contracts/identity-and-approval.md) A1～A5、B1～B4；[contracts/state-and-cancellation.md](../contracts/state-and-cancellation.md) S1、S3；[contracts/events-and-trace.md](../contracts/events-and-trace.md) E2；[review/2026-11-24.md](../review/2026-11-24.md) 高优先级一节与交接清单 |

**一句话结论**：H1/H2/H5 按方案 (a) 在服务端登记批准、`/execute` 只认登记过的批准、`propose` 不再改写修订；H3 用一条原子 SQL 保证每个父任务只建一个汇合任务；H4 改夹具事件编号、事件批量写入改为按编号去重，并新增 `desktop.test_fixtures` 开关把夹具接口挡在生产之外。三项分别交给 World Adapter 线、第 4 线（执行引擎）、桌面线实现。

## 背景

2026-11-24 每晚审查（PR #272，报告 `docs/review/2026-11-24.md`）报出 5 条 P1：H1～H5。几个术语：
- **DES**：模拟工厂；
- **World Adapter（WA）**：AgentNexus 读写外部“世界”（模拟工厂或生产系统）的服务层；
- **汇合任务（join）**：模板任务拆成多个分支后，把各分支结果合并的那一步；
- **夹具（fixture）**：测试用来造数据的接口。

## 问题

- H1：`/api/world/des/execute` 不核对批准，agent 可以伪造 `approval_ref` 自己批准自己。
- H2：`propose` 能把世界修订号改回旧值，让已作废的批准重新生效。
- H5：运营卡“批准并执行”不带角色头时默认当作 operator；案例已取消仍可批准；批准人写死为 `supervisor`。
- H3：模板任务偶发建出两个汇合任务，父任务交付两次。
- H4：Resume 夹具写死事件编号，第二次调用时主键冲突，同一批里真实会话的事件被整批回滚丢失；夹具接口在生产环境也能调用。

不修的后果：WA 写入绕过人工批准（违反契约 A1、A5、B1、B3），M4 生产写入的准入建立在一个漏洞上；用户收到重复交付；断线重连后补不回事件，任务时间线缺失。

## 决策依据

1. **H1、H2 已在 `fa9970d` 上复现**（2026-11-24，临时测试，复核后已删除）：
   - 伪造 `approval_ref="forged-by-agent"` 调 execute，返回 `200 status=accepted dispatched=true`；
   - 世界升到 rev-2 后旧批准返回 `409 APPROVAL_INVALID`，再调一次 `propose(rev-1)`，旧批准返回 `200 accepted`。
2. **H3 已复现**：三分支模板、立即返回的假 agent，60 次中 `dup_joins=1/60 dup_parent_deliveries=1/60`（审查报告为 1/40）。
3. **H4 已复现**（store 级）：第二次写入同编号的夹具事件后，同批真实会话事件入库 0 条（应为 2 条）。
4. **代码事实**：
   - `mom_world_des.rs` 的 `execute` 只把 `approval_ref` 存下来、用于幂等比较；
   - `propose` 在 `body.plan_revision` 与当前值不同时直接写入 `world_plan_revision`；
   - `ops_approve_execute` 缺角色头时 `unwrap_or("operator")`；
   - `orchestrator.rs` 的 `try_join` 先 `list_children` 判断，再 `insert_task`，中间没有互斥；
   - `store.rs` 的 `write_events` 整批一个事务，`commit` 失败只打 `tracing::error!`；
   - `desktop_api.rs` 的 `/resume/fixture/*` 三个路由无条件挂载。
5. **契约**：identity-and-approval B3 规定运营业务写批准入口为“运营频道 OpsCase 卡”，A1 要求批准绑定操作者、参数版本和有效期；events-and-trace E2 要求同一事件编号只处理一次；state-and-cancellation S1 要求状态变化由一次条件更新完成。
6. **调用方**：仓库内调用 `/api/world/des/execute` 的只有测试（RT-WADES、`web/e2e/wa-des-write.spec.ts`），没有产品代码。所以收紧 `/execute` 只影响测试，不影响用户功能。
7. **用户确认**（2026-11-24）：三个待决项都按推荐方案，由对应工作线实现。

## 定了什么

- **H1/H2/H5 采用方案 (a)：服务端登记批准**（不采用“先关掉 `/execute`”）。
- **H3 采用“原子插入汇合任务”**：不改表结构、不迁移，保留“先交付再标完成”的决策（[2026-11-23-wp4-join-deliver-before-complete](2026-11-23-wp4-join-deliver-before-complete.md)）。
- **H4 在 `config.toml` 新增 `[desktop]` 配置段及字段 `test_fixtures`**（布尔，默认 `false`；`AppConfig` 目前没有 `desktop` 段，`2026-11-24` 核对 `crates/runtime/src/config.rs`），用来挂载测试夹具接口；Playwright 测试环境打开。属于公开配置的新增，经用户确认；缺省时行为等同 `false`，现有配置文件不用改。
- 三项分三个 PR，由对应工作线实现，各自先写失败测试（Red）再改实现（Green）。任务卡编号以主线 #1 发号为准，下文暂用 FIX-A/B/C。

## 简单设计

### 任务卡 FIX-A（World Adapter 线）：写入口批准（H1、H2、H5）

| 项 | 内容 |
|---|---|
| 范围 | `crates/server/src/mom_world_des.rs`；`crates/server/tests/mom_world_des_write.rs`；`web/e2e/wa-des-write.spec.ts` 及相关 E2E；`docs/testing/` |
| 做法 | ① 内存 `Store` 新增批准登记表：`approval_ref → {proposal_id, action_digest, plan_revision, approver, role, consumed, test_identity}`。② `ops_approve_execute`：缺 `x-wa-role` 拒绝（403）；案例在 `cancelled_cases` 中返回 409；批准人记录真实角色；先登记批准，再调用与 `/execute` **共用**的内部执行函数。③ `/execute`：`approval_ref` 必须在登记表里存在、未用过，且与提案的摘要和修订号一致，否则返回 403/409，不产生执行记录和干预事件。幂等重放（同幂等键、同载荷）仍返回原结果（A3）。④ `propose`：`plan_revision` 与世界当前值不同时返回 409，不改写；修订只由 `test/bump-plan-revision` 或真实改版抬升。⑤ 新增 `POST /api/world/des/test/approve`（经 `require_test_controls`），登记一条标明“测试身份”的批准（B4），供 RT-WADES 与 E2E 使用 |
| 不做 | 不改 `DesConnector`、`engine.rs`；不改 `mom_world_mock.rs`（同类问题 R-20261121-M9 另卡）；不做真实身份（A6 属 M4） |
| 验收（先 Red） | 新增通用契约测试，覆盖 `/execute` 与运营卡两个入口：伪造批准、修订回退、案例已取消三种输入都被拒，且不产生执行记录。另含：`propose(rev-1)` 在世界为 rev-2 时返回 409、修订仍为 rev-2；不带角色头的运营卡批准返回 403、卡片阶段不变。现有 RT-WADES-09～14 与 E2E-WADYN/WAEXEC/WALOOP 改为先调 `test/approve` 取得批准；PR 写“测试变更说明”，业务断言不放松 |
| 风险 | L2；改动集中在一个文件；E2E 需同步改，按功能地图更新 |

### 任务卡 FIX-B（第 4 线 / 执行引擎）：汇合只建一次（H3）

| 项 | 内容 |
|---|---|
| 范围 | `crates/runtime/src/store.rs`（新增方法）；`crates/runtime/src/orchestrator.rs` 的 `try_join`；`crates/runtime/tests/template_orch.rs` |
| 做法 | `store.rs` 新增 `insert_join_if_absent(task, ctx) -> Result<bool>`：单条 `INSERT INTO tasks … SELECT … WHERE NOT EXISTS (SELECT 1 FROM tasks WHERE parent_id = ? AND fanout_kind = 'join')`，同一写连接内连带写入任务上下文，返回是否插入。`try_join` 改用它，未插入即返回，不转 QUEUED、不 spawn |
| 不做 | 不改表结构、不加迁移；不改“先交付再标完成” |
| 验收（先 Red） | 三分支模板、`env(Duration::ZERO)`，循环 ≥100 次：每个父任务恰好 1 个 join、1 条交付。修复前复现率约 1/60，Red 需在 PR 中贴出失败计数 |
| 风险 | L2，两个高风险文件都已按本决策获批；单条语句在 SQLite 写锁下原子 |

### 任务卡 FIX-C（桌面线）：夹具事件与事件写入（H4）

| 项 | 内容 |
|---|---|
| 范围 | `crates/server/src/desktop_api.rs`；`crates/runtime/src/store.rs` 的 `write_events` / `commit`；`crates/runtime/src/config.rs`（新增 `[desktop]` 段与 `test_fixtures` 字段，`#[serde(default)]`）；`web/e2e` 的测试环境配置 |
| 做法 | ① Resume 夹具事件编号改用 `an_core::new_id`，响应里返回实际编号，测试按返回值读取。② `write_events` 改为 `INSERT OR IGNORE`（契约 E2），统计被忽略的条数，大于 0 时打 `W_` 警告日志。③ `/resume/fixture/*` 只在 `desktop.test_fixtures = true` 时挂载；测试环境配置打开 |
| 不做 | 快照、记忆夹具的同类开关另卡（可在本卡顺带，但须在 PR 中写明） |
| 验收（先 Red） | 第二次 seed 后，真实会话的事件全部能回放；第二个夹具会话用自己的 `after_cursor` 订阅返回 200；开关关闭时 `/resume/fixture/seed` 返回 404 |
| 风险 | L2；`store.rs` 已按本决策获批；新增配置字段默认关闭，不影响现有部署 |

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| H1：先关掉 `/api/world/des/execute`（方案 b） | 改动更小，但 RT-WADES-09～14 与 E2E-WADYN/WAEXEC/WALOOP 要整体改写或停用；M4 生产写入仍需服务端批准登记，迟早要做 |
| H3：进程内互斥锁 | 只保护单进程，且不符合契约 S1“由一次条件更新完成” |
| H3：给 `tasks` 加部分唯一索引 | 更彻底，但要改表结构并附迁移，风险更高；原子插入已足够 |
| H4：`write_events` 失败时逐条重试 | 可行，但同编号重复本来就应忽略（E2），`INSERT OR IGNORE` 更简单 |
| H4：复用 `mom.world_mock` 开关 | 含义不同（MOM 演示世界 ≠ 桌面测试夹具），会让开关含义混乱 |

## 影响与后续

- 三张任务卡由主线 #1 发号后分派：FIX-A → World Adapter 线；FIX-B → 第 4 线；FIX-C → 桌面线。
- 各 PR 合并后，在 `docs/review/findings.jsonl` 追加对应 `fix_claimed` 事件，由下一次每晚审查核实为 `verified_fixed`。
- FIX-A 合并后，审查报告建议的“所有写入口的通用契约测试”即落地；`mom_world_mock.rs` 的同类问题（R-20261121-M9）建议复用同一套登记表另卡修复。

## 出问题先看

- 运营卡批准返回 403：检查请求是否带 `x-wa-role`（FIX-A 起不再默认当作 operator）。
- E2E 的 `/execute` 返回 403/409：检查测试是否先调了 `test/approve`，以及 `propose` 的修订号是否等于世界当前修订号。
- 夹具接口 404：检查配置 `desktop.test_fixtures` 是否打开。
- 日志出现事件被忽略的 `W_` 警告：说明有重复事件编号写入，按编号查来源。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-24 | 新建 | PR #272（审查报告）；用户 2026-11-24 确认 |
