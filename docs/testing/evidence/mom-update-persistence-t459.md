# MOM 引擎 update 持久化分层实测证据（#459 方向 C）

决策：`docs/decisions/2026-11-27-mom-update-persistence-design.md`（用户 2026-11-27 拍板方向 C 并批准实施，见 Issue #459 评论「批准改方案，可继续」）。
分支：`line3-t459-persistence`（基于 `origin/master`）。

## 实施内容

`crates/runtime/src/mom/engine.rs`：

1. **变更比较短路**：`emissions` 为空且 `now` 变化 ⇒ 直接判定快照有变化（`now` 是快照字段，语义严格等价）；`now` 相同时保留全量比较，兜住「闭包直接改状态而未 emit」的罕见路径（该路径触发 `rewrite_all_cases` 兜底重写全部案例行）。
2. **持久化分片（schema v1→v2）**：`mom_case_state`（每案例一行，只重写事件覆盖到的案例）+ `mom_global`（非案例全局态单行：simulations / playback / DES 仿真态 / seen / now / next_token）+ `mom_meta`（CAS sequence）。`mom_events` 与事件格式零改动；`Snapshot` 内存结构零改动；v1 库打开时自动迁移（`mom_snapshot` 原行保留不删——单向迁移，回滚丢迁移后状态，沙盘可重放）。

## 实测数据

| 配置 | mom_des.rs（31 例） |
|---|---|
| 基线（本设计前，PR #460 关闭记录） | 42.40s |
| **本设计（无 opt-level，纯持久化分层）** | **26.83s（-37%）** |
| opt=2 路线对照（PR #460，已关闭） | 7.14s，但 CI 冷编译 +126s、作业总时长净 +5s |

**低于设计预期（10~18s / ≥50%）的原因**：批量仿真引擎是**单案例**——「事件 update」的单案例序列化 ≈ 全量，无法从分片获益（此类 update 占 21% 且单次成本最高）；可省的纯时钟推进部分（79%）已按设计省掉。设计预期区间按「事件 update 也可省」的乐观假设估出。CI 折算预计 150.7s → ~95s，且零编译代价——对比 opt=2 路线（CI 净零），本路线净收益为正。

## 验证

- `cargo test --locked -p an-runtime --test mom` → **55 passed / 0 failed**（含 2 个新测试：v1 库迁移与重启一致性 `v1_snapshot_database_is_migrated_and_reloadable`、纯时钟推进不重写案例行 `clock_only_updates_do_not_rewrite_case_rows`）。
- `cargo test --locked -p an-runtime -p an-server`：28 个测试二进制全绿（`rt_adm_14` 为已知本地环境既有问题，pristine master 同样失败，2026-11-26 基线核实）。
- `mom_des.rs`：31 passed / 0 failed（26.83s）。
- `cargo fmt --all -- --check` 干净；`cargo clippy --locked --all-targets -- -D warnings` 通过。

## 出问题先看

- 状态丢失：`mom_case_state`（案例行）与 `mom_global`（全局态）的写入是否随事件发生；迁移库看 `mom_snapshot` 原行与 `PRAGMA user_version`。
- CAS 冲突：`mom_meta` 的 `sequence` 行与 `mom_events` 最大 seq 是否一致。
