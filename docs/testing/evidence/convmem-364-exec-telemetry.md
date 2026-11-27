# 委派执行资料埋点（R-20261126-L1 / #364）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory；S6-exec 观测性（REQ-01 用例族） |
| 用例 | RT-EXEC-09（新增） |
| 来源 | 夜间评审 2026-11-26（PR #356）交接清单第 10 条；台账 id=R-20261126-L1 |
| 运行模式 | 无头 local；exec 假模型 + 专属 tracing 订阅器；`cargo test -p an-server --test exec_materials` |

## 缺陷

委派成功路径无执行资料注入量埋点：注入规模（exec_chars）与原因（开关关闭 / 空块 / 角色跳过）不可观测。

## 实现

- `conversation.rs` delegate 成功处（`orchestrator.submit` 之后）补发一条 `session.materials.exec`：`task_id`、`conv_id`、`exec_injected`、`reason`（`injected` / `switch_off` / `empty` / `role_skipped`）、`exec_chars`（资料段字符数）。
- 只记量与原因，不记资料正文与用户正文（AGENTS.md 日志约定 + SMAT-05 口径）。
- DESIGN §10.3 `session.materials` 行登记该补发事件与字段。

## Red（修复前，master `30a4839` + 仅新增测试）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials rt_exec_09
test result: FAILED. 0 passed; 1 failed; ... finished in 0.09s
```

（断言「委派成功应补发 exec 埋点（含 task_id）」失败——事件不存在。）

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials
test result: ok. 9 passed; 0 failed; ... finished in 0.13s
```

断言覆盖：注入轮（`exec_injected=true` / `reason=injected` / 无资料正文泄漏）；关闭开关轮（`exec_injected=false` / `switch_off` / `exec_chars=0`）。测试断言对 tracing 字段引号渲染鲁棒（值本身断言，不做 `reason=` 前缀拼接匹配）。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
