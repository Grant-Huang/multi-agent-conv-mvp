# 审查修复 2026-11-18（DES 部分）Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（master f05a339 之上）。
- 问题与验收：[docs/review/2026-11-18.md](../../review/2026-11-18.md) 的 R-20261118-H2、M1、L1、L2。
- 决策：[DECISIONS.md](../../DECISIONS.md) DES-25、DES-26。
- 用例：[CASES.md](../CASES.md)「审查修复 2026-11-18」。

范围：本提交只包含用例、决策记录与失败测试，行为尚未实现；没有修改已有测试断言。L2 只改文档，没有用例。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des rt_des_31
test rt_des_31_closing_an_incident_never_closes_a_safety_blocked_member ... FAILED
  closing the incident must not close a safety-blocked member  left: "CLOSED"

CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des rt_des_32
test rt_des_32_human_review_is_bound_to_an_ai_review_version ... FAILED
  agree needs a completed AI review

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des http_des_05
test http_des_05_review_decision_needs_ai_and_does_not_block_workers ... FAILED
  agree before any AI review must be rejected  left: 200
```

HTTP-DES-05 停在第一个断言（M1），走不到 L1 的部分。为了确认 L1 部分本身也是 Red，做了一次临时运行：只跳过 M1 断言（不提交，运行后已恢复原文件），结果如下。

```text
test http_des_05_review_decision_needs_ai_and_does_not_block_workers ... FAILED
  a 50 ms heartbeat took 2.001363247s: the AI review handler blocked the only async worker
```

**测试的一次修正（Red 阶段，用例尚未定稿）。** HTTP-DES-05 第一版先睡 100 ms 等请求到达，再开始测心跳。结果处理函数在这 100 ms 里就把工作线程占住了；等心跳开始计时，锁已经释放，阻塞被漏检，测试在 Red 阶段误通过。改为在整个请求期间连续打心跳、记录最大间隔后，能稳定检出阻塞（上面的 2.0 秒）。

失败原因都是业务行为不符合验收条件，不是环境问题。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 550 passed，3 failed（只有上述 3 个新用例），exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 27 features, 24 browser cases
cd web && npm run build                                                   → ✓ built
```

## 与 #96 合并（2026-11-18）

本 PR 推送后，另一条线的 #96（`fix/r-20261118-h2`）同期修复了 H2 并已合并到 master，也新增了一个 RT-DES-31。合并 master 时按以下方式处理：
- **实现：** 保留 #96 的实现。它跳过同样三类成员，在成员上记 `CaseCloseSkipped`，原因码为 `safety_trip` / `blocked` / `verification_incomplete`。在此之上加回决策 DES-26 的事项侧汇总：事项上为每个被跳过的成员记一条 `CaseCloseSkipped`（`member`、`reason`）。
- **用例：** 本 PR 原来的 H2 用例与 #96 重号，改编号为 RT-DES-33，断言不变；#96 的 RT-DES-31 原样保留。下文 Red 记录中的 `rt_des_31_closing_an_incident_never_closes_a_safety_blocked_member`，即现在的 `rt_des_33_incident_records_each_member_skipped_on_close`。

## Green

用户确认用例与修法（DES-26）后实现，没有修改测试断言。

- H2（`des/engine_link.rs` 的 `des_after_close`）：关闭事项时，跳过以下三类成员：安全联锁中（`safety_trip`）、BLOCKED、或有未核验的动作。每跳过一个，在事项上记一条 `CaseCloseSkipped`（成员、原因）；其他成员照常关闭。没有改 `engine.rs` 的 `close`。
- M1（`des/review.rs`）：
  - AI 审核记录新增递增的 `version`；
  - `HumanReview` 新增 `ai_version` 字段，带 serde 默认值，旧快照仍可读取；
  - 没有 AI 审核时，"同意"和"推翻"都被拒绝；AI 审核降级时，"同意"被拒绝；
  - 报告中的 `human` 只显示针对当前版本的结论，`human_history` 每条标出 `applies_to_current_ai`；分歧记录带 `ai_version`。
- L1：AI 审核拆成三步：
  - `des_ai_review_input`：准备输入，需要加锁；
  - `call_reviewer`：调用模型，锁外 await；
  - `des_record_ai_review`：写回结果，需要加锁。

  HTTP 入口把两段加锁的步骤放进阻塞线程（`with_engine`）。`run_ai_review` 的签名不变，测试与其他调用方不受影响。
- L2：在 `config.example.toml` 注释和设计 §8.2、§8.4 中注明真实审核模型尚未接入；§9 补上 `POST /api/mom/reviews/{case_id}/ai`，并改正"唯一写接口"的措辞。

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 553 passed，0 failed，exit=0
  rt_des_31_closing_an_incident_never_closes_a_safety_blocked_member ... ok
  rt_des_32_human_review_is_bound_to_an_ai_review_version ... ok
  http_des_05_review_decision_needs_ai_and_does_not_block_workers ... ok
  原有 RT-DES-12～14、19、25～30 与 HTTP-DES-01～04 全部 ok
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 27 features, 24 browser cases
cd web && npm run build                                                   → ✓ built
```

本机磁盘额度有限，以上命令在 `CARGO_PROFILE_DEV_DEBUG=0` 的环境中运行。这个变量只去掉调试信息，不影响测试行为。

**超时、取消与清理**
- AI 审核调用模型时有超时（HTTP 入口为 60 秒），模型报错时最多重试一次，调用期间不持有引擎锁。超时后 future 被丢弃，不留后台任务。
- 两段加锁计算在 `spawn_blocking` 中执行，计算量与原来相同。阻塞线程开始执行后不能被 HTTP 取消，但会在有界时间内完成并释放锁。
- 没有新增子进程。
