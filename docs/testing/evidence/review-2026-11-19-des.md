# 审查修复 2026-11-19（R-20261119-M2、M13、M14）Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（含冲突解决与 Green 实现）。问题来自 `docs/review/2026-11-19.md`（分支 `review/2026-11-19`）。决策见 [DECISIONS.md](../../DECISIONS.md) DES-29、DES-30。用例见 [CASES.md](../CASES.md)「审查修复 2026-11-19」。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-des --test factory

an-des factory：8 passed; 2 failed
  rt_desf_09  panicked at crates/des/src/scenario.rs:585:30: attempt to multiply with overflow
              → simulation_create panicked on {... "start_minute":18446744073709551605 ...}
  rt_desf_10  one crew: the second machine waits for the first repair   left: 497（应为 737）
```

CI job 135934074231 同样在这两条失败（Rust exit 101）。另有：
- `rt_desm_04`：溢出经 MCP 变成 `-32603`（应为工具错误）
- `rt_des_34`：过期 `ai_version` 仍被接受
- `rt_des_35`：缺少 `ai_version` 仍返回 200

失败原因与审查报告一致，不是环境问题。

## Green

实现要点：
- M2：`scenario` / 资源耗时上限；MCP `catch_unwind` + 锁中毒恢复
- M13：`an_des::schedule` 共享排班；模拟工厂与进程内累计已接受维修；同设备重叠拒绝
- M14：`des_human_review_for` 校验版本（`E_REVIEW_STALE` → HTTP 409）；请求体 `ai_version` 必填；HTTP-DES-04 请求体补版本号（断言未改）

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-des --test factory
  ok. 10 passed; 0 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-des-mcp --test stdio
  ok. 4 passed; 0 failed（含 rt_desm_04）

CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des
  ok. 31 passed; 0 failed（含 rt_des_34）

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des -- rt_des_35 http_des_04 http_des_05
  ok. 3 passed; 0 failed

CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_connector --test mom_des_http_mcp
  ok. 6 + 6 passed; 0 failed
```
