# MOM 车间 DES 阶段④ Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（阶段③ 提交 75fac8a 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §8.4 阶段④修订说明；用例：[CASES.md](../CASES.md)「MOM 车间 DES 阶段④」。

范围：新增用例 RT-DES-12～15、19 与 HTTP-DES-04；接口骨架包括 `des/review.rs`（规则、审核接口、脚本替身、模型选择、引擎审核方法，均未实现）、`des/batch.rs`（批量实验，未实现）、配置字段 `[models].mom_reviewer`（含示例配置注释）、`AppState::with_reviewer`、HTTP 命令 `claim_cause` 与审核路由。设计中取消了"每案例每小时观测数上限"（用户同意）。没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
test rt_des_12_rule_checks_detect_wrong_handling ... FAILED
  cause claims are recorded: cause claims are not implemented yet
test rt_des_13_ai_review_cites_evidence_or_degrades ... FAILED
  AI review input: AI review input is not implemented yet
test rt_des_14_human_override_is_recorded_as_disagreement ... FAILED
  called `Result::unwrap()` on an `Err` value: AI review input is not implemented yet
test rt_des_15_batch_runs_are_reproducible_and_export_csv ... FAILED
  assertion `left == right` failed: one row per seed and policy  left: 0
test rt_des_19_reviewer_model_choice_and_independence ... FAILED
  assertion `left == right` failed  left: None
test result: FAILED. 16 passed; 5 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des
test http_des_04_review_flow_over_http ... FAILED
  command {"command":"claim_cause",...} failed: {"error":"cause claims are not implemented yet"}  left: 400
test result: FAILED. 3 passed; 1 failed
```

RT-DES-19 的配置解析已通过（`mom_reviewer` 被识别为已知字段，没有未知字段警告），失败点在模型选择尚未实现。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 425 passed，仅上述 6 个新用例失败，exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                                   → ✓ built
```

## Green

待实现后补充。实现时不修改上述断言；如发现用例本身有误，先说明并经用户确认。
