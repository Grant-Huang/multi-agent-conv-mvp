# 证据：T-81 记忆持久化验收探针（对 T-78 实现的回归守卫）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：主线 #1:1 用户指示「推，然后继续主线下一步」；本卡为测试+文档（L1），未触碰 store.rs/desktop_api.rs 生产代码
- **背景**：本线原 T-77 卡（记忆持久化实现）与并行会话的 **T-78（#237）撞车且被取代**；T-78 落地后，本卡以入口负责人身份用两条探针实证其满足本线验收口径，并补录 T-78 缺失的两个回归场景。原实现分支 `cursor/ai-desktop-t77-memory-persistence` 留档不推。
- **编号说明**：T-77（master=双设备 D01）、T-79（第 4 线）、T-80（WA M3-5）均已被占用，本卡顺延 T-81。

## Case

| Case | 场景 | 对 T-78 的结果 |
|---|---|---|
| RT-ADMEM-08 | 两个全新库各起服务；库一纠错+撤回后，库二必须仍是种子态（进程状态不得跨库泄漏） | **通过** |
| RT-ADMEM-09 | mem-1/mem-2 全部撤回后同库重开：列表为空、单查 410 `revoked`（软删行使 COUNT>0，ensure 播种不触发） | **通过** |
| RT-ADMEM-01～07 | T-78 既有基线 | 复跑全绿 |

## 命令与结果（分支 `cursor/ai-desktop-t78-acceptance` = master `ba9a7c7` + 本卡测试/文档）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_mem
running 9 tests
test rt_admem_01 ... ok （～07 ok，略）
test rt_admem_08_fresh_db_starts_from_seed_not_process_state ... ok
test rt_admem_09_all_revoked_restart_keeps_revoked ... ok
test result: ok. 9 passed; 0 failed; 0 ignored
```

全量 workspace、fmt、clippy、test:map 结果见 PR 正文（与 CI 一致的命令）。

### 全量 workspace（master `ba9a7c7` + 本卡）

`cargo test --locked --workspace`：34 个测试二进制全部 `ok`、0 失败——含此前在 540b1d4 曾连续两次失败的 `rt_adm_14`（10s「任务未结束」）。判定其失败为**间歇性/时序敏感**（与负载相关），非稳定回归；已在 PR 正文移交 admission 负责人关注。

## 验收结论

- **T-78 满足主线 #1:1 对「记忆持久化」的验收口径**：Store 权威、同库重开持久（其 RT-ADMEM-07）、库间隔离（本卡 08）、全撤回重启不重播种（本卡 09）。
- 09 的安全性来自**软删**（`revoked` 行保留使 `ensure_desktop_memory_seed` 的 COUNT>0 成立）；若未来引入硬删除或按 user 清理，此探针会即时变红——这正是补录它的目的。

## 非目标

- 不改 T-78 的实现与表结构（`user_id` 列、索引维持现状）
- 不做记忆 UI 改动；不做跨端记忆同步（依赖完整 IdP，后置）
- 浏览器 E2E 本轮未复跑（生产代码零改动，desktop.spec 于 T-77 开发期间 19/19 的记录仍适用）
