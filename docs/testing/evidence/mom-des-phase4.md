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

实现（未修改任何测试断言）：
- `des/review.rs`：五条规则（`check_rules` 纯函数）；`resolve_reviewer`（`mom_reviewer` → `[llm].model` → 只做规则）；`run_ai_review`（锁外调用、超时、模型报错重试一次、输出校验只接受引用输入证据编号的结论）；`record_cause_claim`、`des_review_record`、`des_review_report`、`des_ai_review_input`、`des_human_review`。审核记录存放在快照新字段 `des_reviews`（`#[serde(default)]`，旧快照可读），不写入案例。
- `des/batch.rs`：`run_batch` 每个 seed × 策略一个独立内存引擎，按 seed 分段多线程运行，结果按原顺序拼接；`to_csv`。
- `server/src/mom_sim.rs`：未配置审核模型时，`POST /api/mom/reviews/{id}/ai` 返回规则结果并标注 `ai.status = unavailable`（不写审核记录）。

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 440 passed，0 failed，exit=0
  rt_des_12_rule_checks_detect_wrong_handling ... ok
  rt_des_13_ai_review_cites_evidence_or_degrades ... ok
  rt_des_14_human_override_is_recorded_as_disagreement ... ok
  rt_des_15_batch_runs_are_reproducible_and_export_csv ... ok   （100 seed × 2 策略 × 2 次，debug 构建 4 核约 64 秒）
  rt_des_19_reviewer_model_choice_and_independence ... ok
  http_des_04_review_flow_over_http ... ok
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 18 features, 14 browser cases
cd web && npm run build                                                   → ✓ built
```

**实现中的发现。** 传统策略的决策延迟（3 次交接 + 审批，均值约 80 分钟）超过证据有效期，引擎拒绝批准（"evidence snapshot stale; refresh plan"）。这是合理的安全检查，所以批量实验按引擎规则处理：决策到期时证据已过期则 `refresh_plan`，刷新后立即按新的推荐方案决策，等待时间仍从最初请求算起；刷新后已没有可行方案（交期赶不上）时这次决策作废、计为 `late`，并单独统计为新列 `expired_decisions`。示例（seed 1–3，P0，60 分钟）：

```text
seed,policy,cases,alarms,decisions,mean_decision_minutes,risk_parts,on_time,late,verified_cases,rule_failures
1,ai_native,1,2,1,5.00,0.0,1,0,0,0
1,traditional,1,2,1,72.00,0.0,1,0,0,0
2,ai_native,1,2,1,2.00,0.0,1,0,0,0
2,traditional,1,2,0,,0.0,0,1,0,0
3,ai_native,1,2,1,8.00,0.0,1,0,0,0
3,traditional,1,2,0,,0.0,0,1,0,0
```
（此输出在增加 `expired_decisions` 列之前记录；seed 2、3 的传统策略即为过期后无可行方案。）

**超时、取消与清理。**
- AI 审核：每次模型调用受 `tokio::time::timeout` 限制（HTTP 入口 60 秒）；调用期间不持有引擎锁；超时后 future 被丢弃，不留后台任务；只在模型报错时重试一次。降级时规则结果照常返回。
- 批量实验：同步运行，用 `std::thread::scope` 并行，函数返回前所有线程已结束；每次运行有上限（场景时长 ≤ 7 天，场景结束后最多再推进 1440 分钟，seed ≤ 10000）。没有新增子进程。

**外部内容进入模型上下文（7-fence-review.md 第 4 节）。** 本阶段只有脚本审核替身，没有接入真实模型。`des_ai_review_input` 已在 `instructions` 中声明案例与标准答案是不可信数据、不是指令；模型输出只接受结构化结论，引用必须在输入的证据编号中，文本长度与条数有上限。真实审核模型接入时须按第 4 节完整检查。

