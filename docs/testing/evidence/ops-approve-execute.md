# 运营案例批准即执行（INTENT-027，Issue #340）Red→Green 证据

决策：`docs/decisions/2026-11-26-ops-approve-auto-execute.md`；设计：`docs/mom/OPS_DESK.md` §4.1。
分支：`line3-t340-approve-execute`（基于 `origin/master` `6e784a4`）。

## 用例

- `E2E-OPSAUTOEXEC`、`E2E-OPSEXPIRE`（`web/e2e/ops.spec.ts`），登记于 `docs/testing/CASES.md` 与 `docs/testing/features.json`（ops-desk）。
- `RT-OPSDESK-12`、`RT-OPSDESK-13`（`crates/runtime/tests/ops_desk.rs`）。

## Red（实现前）

命令（仓库根）：

```
cd web && npx playwright test e2e/ops.spec.ts --grep "OPSAUTOEXEC|OPSEXPIRE" --reporter=line --workers=2
```

结果：`2 failed`（退出码 1）。

- `E2E-OPSAUTOEXEC`：卡上批准后卡片仍为「待批准」，断言「核验中」失败——批准后没有任何执行发生，也不存在执行入口。
- `E2E-OPSEXPIRE`：批准（资源不可用 → 执行无入口）后卡片摘要仍为「待批准，约束：…」（引擎已记录批准人 supervisor），断言「已批准」「执行受阻」失败；过期提示与重新批准均不存在。

业务断言失败即 Red；失败点与 Issue #340 描述的缺陷一致，不是环境失败。
证据：`web/test-results/cases/ops-E2E-OPSAUTOEXEC-*`、`ops-E2E-OPSEXPIRE-*`（截图、trace、error-context）。

## Green（实现后）

实现提交（本分支 `line3-t340-approve-execute`）后，同一命令：

```
cd web && npx playwright test e2e/ops.spec.ts --reporter=line --workers=1
```

结果：`9 passed (27.4s)`，退出码 0——含两个新用例与既有 7 个用例（E2E-OPSDESK / OPSDRAWER / OPSCHANNEL / OPSNOISE / OPSAPPROVE / ADREAD / OPSOFF）全部通过，无回归。

Rust 侧（`CARGO_TARGET_DIR=target`，仓库根）：

- `cargo test --locked -p an-runtime`：全绿（含既有引擎契约测试 `changed_resource_and_expired_approval_prevent_dispatch` 等，`approve`/`execute` 原语语义未变）；新增 `rt_opsdesk_12_approve_and_execute_dispatches_or_blocks_with_approval_kept`、`rt_opsdesk_13_approval_expiry_emits_once_and_card_prompts_reapproval` 通过。
- `cargo test --locked -p an-server --test mom_ops --test mom --test mom_des`：16 passed / 0 failed（含既有 `rt_opsdesk_11_approve_is_idempotent_and_rejects_stale`）。
- `cargo test --locked --workspace`：除 `rt_adm_14_admission_http_end_to_end` 外全绿。该测试为已知 flake（第 4 线记录「rt_adm_14 flake 勿当新缺陷」）；基线核实：pristine `origin/master`（6e784a4）独立 worktree 冷构建复跑同样失败（同一 10s「任务未结束」超时签名，`crates/server/tests/admission.rs:203`），属既有本地环境敏感问题，与本改动无代码交集（admission 测试不含 `[mom]` 配置）。
- `cargo clippy --locked --all-targets -- -D warnings`：退出码 0（仅 master 既有的 workspace manifest 警告）。
- `cargo fmt --all -- --check`：干净。

Web 侧（`cd web`）：

- `npm run typecheck`：干净；`npm test`：10 files / 31 tests passed；`npm run build`：通过；`npm run test:map`：`Feature map valid: 33 features, 101 browser cases`。

实现要点（对应决策「定了什么」）：

1. `Engine::approve_and_execute`（`crates/runtime/src/mom/engine.rs`）：一个事务内 `record_approval` + `dispatch_approved`；执行受阻时批准保留、发 `ExecutionBlocked{scope,reason}`、记 `execution_block` 提示字段；成功清除该字段并照旧走 `des_after_execute`。
2. `tick()` 过期检查（同文件）：批准未被动作消费且 `now > expires_at` 时发一次 `ApprovalExpired{scope}`（`Approval.expiry_notified` 防重复）。
3. `card_for(case, level, now)`（`crates/runtime/src/mom/ops_desk.rs`）：WAITING_HUMAN 且该范围无动作时，摘要按批准状态显示「已批准，待执行（执行受阻：原因）」/「批准已过期，需重新批准」；其余状态摘要不变。
4. `mom_ops.rs` approve 接口改走 `approve_and_execute`，响应增加 `executed?` / `blocked_reason?`（`already`/`approver` 字段不变）。
5. `OpsCards.tsx`：MOM 模拟卡（provenance=simulation）在 WAITING_HUMAN 且有范围/方案版本时按钮始终可见；标签以卡片携带的引擎状态（`card.approver`）为准——批准 / 重试执行 / 重新批准；World Adapter 卡（world_mock/des）行为不变。
