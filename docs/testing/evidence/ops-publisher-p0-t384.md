# OpsPublisher 分级回溯缺陷修复（Issue #384）Red→Green 证据

分支：`line3-t384-publisher-p0`（基于 `origin/master` `11fd6f1`）。

## 用例

`RT-OPSDESK-14`（`crates/runtime/tests/ops_desk.rs` `rt_opsdesk_14_case_advanced_before_first_tick_still_gets_card`），登记于 `docs/testing/CASES.md` 与 `docs/testing/features.json`（ops-desk）。复现路径与 Issue #384 的 API 三步序列同构：同一引擎两个待批准案例，发布器首个 tick 前把其中一个批准并执行到 `VERIFYING`，再 tick。

## Red（实现前）

```
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test ops_desk rt_opsdesk_14
```

```
test rt_opsdesk_14_case_advanced_before_first_tick_still_gets_card ... FAILED
节拍前已执行的新案例必须出卡（#384），实际 ["CASE-s1-1"]，
report TickReport { posted: 1, updated: 0, summarized: 0, skipped: 1 }
test result: FAILED. 0 passed; 1 failed
```

被执行案例的卡未发出（`skipped: 1`，即 `p1_no_card`），与 Issue #384 描述一致；业务断言失败即 Red，非环境失败。

## Green（实现后）

修复：`crates/runtime/src/mom/ops_desk.rs` 将 `DecisionRequested` 加入 `P0_KINDS`——决策请求按**事件种类**固定 P0，不再用消费时刻的案例阶段回溯定级。

```
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test ops_desk
→ test result: ok. 11 passed; 0 failed（rt_opsdesk_14 转绿，既有 10 个不变）
```

回归：

- `cargo test --locked -p an-runtime -p an-server`：除 `rt_adm_14_admission_http_end_to_end`（已知本地环境既有问题：pristine `origin/master` 冷 worktree 同样失败，10s「任务未结束」，与本改动无代码交集）外全绿，25 个测试二进制 `test result: ok`。
- `cargo fmt --all -- --check` 干净；`cargo clippy --locked --all-targets -- -D warnings` 退出码 0。
- `cd web`：`npm run typecheck` 干净、`npm test` 31 passed、`npm run build` 通过、`npm run test:map` `Feature map valid: 33 features, 99 browser cases`。
- E2E：`npx playwright test e2e/ops.spec.ts --workers=1` 全绿（回填于下）。

## 行为说明（不回退项）

- 待批准案例出卡路径不变（原来就经 `needs_human` 得 P0）； hourly_cap 与汇总卡行为不变（RT-OPSDESK-05 全绿）。
- 仅「案例在首个发布节拍前已被推进到后置阶段」这一缺陷类别变化：其创建期 `DecisionRequested` 事件现在按种类得 P0，出卡不再依赖消费时刻的阶段。
