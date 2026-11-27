# MOM 引擎 update 持久化分层优化（#459 方向 C 设计，待批准）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 来源 | 助手建议（用户 2026-11-27 拍板方向 C「引擎序列化优化」，本设计为其具体化，**待批准后实施**） |
| 状态 | **待用户确认** |
| 影响范围 | `crates/runtime/src/mom/engine.rs`（仅此一个文件的持久化编解码层）；MOM 库 schema v1→v2 迁移 |
| 相关设计 | Issue #459；`docs/DESIGN.md` 第 1054 行（MOM 独立库表记载，随 PR 更新） |

**一句话结论**：把 MOM 引擎每次 `update` 的持久化从「整体快照全量 JSON 序列化 + 无条件 UPSERT」改为「① 时钟推进场景的变更比较短路（语义严格等价）+ ② 快照按案例分片存储（无事件的纯时钟推进只写几字节的 meta 行）」，预期 `mom_des.rs` 从 42s 量级降到 10~18s，不再依赖会拖慢 CI 冷编译的 opt-level=2。

## 背景

Issue #459 实测（插装 rt_des_15，32 万次 update，见 #459 评论）：`Engine::update`（`crates/runtime/src/mom/engine.rs`）三段成本——克隆 7.5%、变更比较（两次全量序列化）25.3%、SQLite 事务段 67.2%（其中 mom_snapshot 的全量 `serde_json::to_string(&next)` 是大头）。**79% 的 update 是无事件的纯时钟推进**（cmp_hit 253,687/320,000），却每次都付全量序列化。构建配置路线（opt=2）本机 5.9 倍但 CI 冷编译 +126s 净收益归零（PR #460 已如实关闭），故转向根因治理。

## 设计

### 第一刀：变更比较短路（严格语义等价）

现状：`emissions.is_empty() && serde_json::to_vec(&next)? != serde_json::to_vec(&self.state)?`——每次无事件 update 做两次全量序列化。

改为：`emissions.is_empty() && (next.now != self.state.now || serde_json::to_vec(&next)? != serde_json::to_vec(&self.state)?)`。

等价性：`now` 是 Snapshot 字段，now 不等 ⇒ 序列化必不等 ⇒ 短路分支与原判定同真；now 相等时走原全量比较（保留「闭包不 emit 而直接改状态」场景的判定）。纯时钟推进（79%）恰好 now 必变 → 两次全量序列化全免。

### 第二刀：快照分片存储（schema v1→v2）

- 新表：`mom_case_state(case_id TEXT PRIMARY KEY, seq INTEGER NOT NULL, body TEXT NOT NULL)`（每案例一行）；`mom_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)`（`now`、`sequence` 两行）。`Snapshot` 内存结构**不变**，只有持久化编解码分片。
- 写路径：`update` 事务里——① 事件覆盖的案例集合 D（emissions 的 case_id 去重，`__runtime` 除外）非空时逐案例序列化写 `mom_case_state`；② `mom_meta` 始终更新（now/sequence，几字节）；③ D 空且 now 未变且全量比较相等（真无变化）→ 不写任何行（原实现 UPSERT 一个相同 body，数据等价、分片版省掉）。
- 读路径：`from_db` 按 user_version 组装——v2 读 meta + 全部 `mom_case_state`；CAS 检查从 meta 读 `sequence`（SQLite IMMEDIATE 事务串行化写者，并发语义不变）。
- **迁移（v1→v2，附测试）**：打开 v1 库时一个事务内读 `mom_snapshot` JSON → 拆行写 `mom_case_state` + `mom_meta` → `PRAGMA user_version=2`（参考 `store.rs` 补列写法；`mom_snapshot` 原行保留不删）。新增迁移测试：v1 库 → 打开 → 案例/时钟/游标完整 → 再存再读一致。
- `mom_events` 表与事件格式**完全不动**；`human_view`/`agent_view`/发布器/DES 连接器零改动（都走内存 Snapshot）。

## 预期与验收门槛

- 预期：第一刀免 ~25% 比较成本；第二刀让 79% 的纯时钟推进 update 从「全量序列化+UPSERT」降为 meta 小写。合计 `mom_des.rs`（基线 42.40s）预期 **10~18s**。
- 验收门槛：基线相对提速 ≥50%；全量 an-runtime/an-server 测试绿（rt_adm_14 已知本地问题除外）；迁移测试覆盖 v1 库；`--locked` 不变。

## 风险与回滚

- 改动封闭在 `engine.rs` 一个文件（`mom_snapshot`/`mom_case_state` 表无外部引用，已 grep 确认）。
- **迁移单向**：回滚到 v1 代码后，v2 期间的仿真状态演进丢失（mom_snapshot 冻结在迁移时刻）。MOM 库是仿真沙盘数据、可由场景重放重建，影响可接受——**明示接受**。
- 提交前全量回归 + 迁移测试 + 计时数据（对照 #459 评论的插装基线）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 只做第一刀 | 收益约 1/4，大头（67% 的持久化段）不动；两刀共用一次评审，一起做 |
| Clone 优化（COW 等） | 实测仅占 7.5%，不值得复杂化 |
| 事件日志式持久化（不存快照，重放重建） | 改动语义大（读路径全变）、恢复时间变长，超出本单目标 |
| 保留 opt=2 路线 | CI 冷编译 +126s 净收益归零（PR #460 实测），除非 #432 引入编译缓存 |

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-27 | 新建（待批准） | Issue #459、PR #460 关闭记录 |
