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

## Green

待用户确认用例后实现并补充。
