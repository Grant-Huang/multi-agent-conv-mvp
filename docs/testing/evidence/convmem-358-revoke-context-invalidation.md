# 撤回即失效：清扫未终态任务 context（R-20261126-H1 / #358）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-02（撤回语义）；宪章 CH-G2e |
| 用例 | RT-EXEC-06（撤回后批准）；store 单测 ×4（strip 行为矩阵） |
| 决策 | [2026-11-26-convmem-revoke-context-invalidation](../../decisions/2026-11-26-convmem-revoke-context-invalidation.md)（用户拍板：撤回即失效 + 撤回时清扫；修订 S6 决策第 4 条个人记忆部分） |
| 运行模式 | 无头 local；exec 路径假模型（route_decision=high → NEEDS_HUMAN）；`cargo test -p an-server --test exec_materials` / `-p an-runtime --lib` |

## Red（修复前，master `6e3b407` + 仅新增测试）

评审复现面：撤回记忆 → 批准待批任务 → General 首步 system 仍含记忆正文。

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials rt_exec_06
thread '...' panicked at crates/server/tests/exec_materials.rs:495:5:
撤回后批准，执行者不得再拿到记忆正文（CH-G2e）：你是 AgentNexus 的 General Agent...
test result: FAILED. 0 passed; 1 failed; ... finished in 0.09s
```

守卫断言先行通过（批准前 context 确含该记忆、任务进入 NEEDS_HUMAN），失败点精确落在修复断言上。

## 修复

- `store.rs`：`revoke_desktop_memory` 撤回时调用 `cleanup_revoked_memory_from_contexts`——扫描未终态（非 COMPLETED/FAILED/CANCELLED）任务 context，`strip_memory_from_context` 按 `来源：{id}（个人记忆` 条目粒度摘除（段空去段、块空去整块），兼容移除片 2 修正前的判断清单遗留行；不动 `orchestrator.rs`。清扫记 `materials.context.cleaned` 事件；失败不阻断撤回。
- 已终态任务 context 保留原件（L2 审计口径不变）。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials
test result: ok. 6 passed; 0 failed; ... finished in 0.12s   （含 RT-EXEC-06）
$ CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --lib strip
test strip_drops_whole_block_when_nothing_left / strip_none_when_memory_not_in_block /
      strip_removes_legacy_digest_line / strip_removes_memory_entry_keeps_receipts ... ok
test result: ok. 4 passed; 0 failed
```

## 边界（如实）

- 批准与撤回同时刻的竞态窗口（批准后瞬间撤回）：批准后的执行上下文已读出，清扫不追溯已发出的请求——本决策验收语义为「撤回后批准」（issue 验收标准原文）。
- 重试路径走全新委派（collector 本就排除已撤回记忆），重启续跑与新批准同源读取已清扫的库中 context。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
