# 审查修复 2026-11-19（R-20261119-M2、M13、M14）Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（master dea0e2a 之上）。问题来自 `docs/review/2026-11-19.md`（分支 `review/2026-11-19`）。决策见 [DECISIONS.md](../../DECISIONS.md) DES-29、DES-30。用例见 [CASES.md](../CASES.md)「审查修复 2026-11-19」。

范围：本提交只包含以下内容，行为尚未修复：
- 用例 RT-DESF-09、RT-DESF-10、RT-DESM-04、RT-DES-34、RT-DES-35；
- 决策 DES-30（助手建议）；
- 骨架 `Engine::des_human_review_for`：暂时忽略版本，等同原方法；
- 失败测试。

已有测试与断言没有改动。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-des --test factory -p an-des-mcp --test stdio \
  -p an-runtime --test mom_des -p an-server --test mom_des --no-fail-fast

an-des factory：8 passed; 2 failed
  rt_desf_09  panicked at crates/des/src/scenario.rs:585:30: attempt to multiply with overflow
              → simulation_create panicked on {... "start_minute":18446744073709551605 ...}
  rt_desf_10  one crew: the second machine waits for the first repair   left: 497（应为 737）
an-des-mcp stdio：3 passed; 1 failed
  rt_desm_04  McpError(-32603 "simulated factory worker failed")（应为工具错误）
an-runtime mom_des：30 passed; 1 failed
  rt_des_34   a decision on a superseded AI review is rejected: ()（旧版本仍被接受）
an-server mom_des：5 passed; 1 failed
  rt_des_35   ai_version is required: 200 OK
```

失败原因与审查报告描述的问题一一对应：
- 溢出导致 panic；
- 维修单不排队；
- 人工结论绑定当前版本。

都不是环境问题，同一测试目标中的其他用例都通过。

## Green

待用户确认用例与 DES-30 后实现。
