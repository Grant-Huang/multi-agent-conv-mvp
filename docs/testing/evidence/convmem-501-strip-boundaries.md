# 多段正文记忆撤回整体摘除（R-20261127-H1 / #501）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory；REQ-conversation-memory-02（撤回语义）；契约 CH-G2e |
| 用例 | RT-EXEC-10（集成）、strip 单测 ×2（多段整体摘除 / 兄弟条目不误扩）；memory_expired 无关此卡 |
| 来源 | 夜间评审 2026-11-27（PR #497）台账 id=R-20261127-H1（两审查者独立复现）；同宪章条款 24h 内第二次 P1 |
| 运行模式 | `cargo test -p an-server --test exec_materials` / `-p an-runtime --lib strip` |

## 缺陷（已复现，两审查者独立）

`strip_memory_from_context` 遇空行或「来源：」行就终结条目：正文多段（含空行）的记忆撤回后，清扫结果仍残留「第二段 SECRET」，且日志报「已清扫」误导核查。由 #436 引入。

## 修复（按 issue 修复方向「渲染时折叠摘要换行」+ 条目边界重构）

1. **渲染不变量**（`session_materials.rs`）：六处摘要插值统一过 `fold_summary`——换行折叠为空格，条目在资料块内**恒为「来源：」+「摘要：」两行**，多段正文不再制造伪条目边界。
2. **清扫边界**（`store.rs strip_memory_from_context`）：条目终结只认**行首「来源：」**（摘要行中段的「来源：」字样不误扩，新单测覆盖）；空行随条目整体保留/摘除。
3. **行首不变量 + 折叠**共同保证：条目 = 「来源：<id>（个人记忆…」行 + 单行摘要，摘除粒度精确到条目。

## Red（修复前，master `f213ab3` + 仅新增测试）

```
rt_exec_10_multi_paragraph_revoked_memory_fully_removed ... FAILED
  （执行请求/exec_material 残留 SECRET-501——issue 复现本体）
strip_multi_paragraph_summary_removes_all_paragraphs ... ok（此单测在旧解析下恰巧过——空行终结行为）
strip_keeps_sibling_entry_with_userwritten_source_text ... FAILED（兄弟条目被误扩删除）
```

集成测试 rt_exec_10 与兄弟条目误扩用例在修复前失败，构成 Red。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials
test result: ok. 10 passed; 0 failed; ... finished in 0.14s
$ CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --lib strip
test result: ok. 6 passed; 0 failed; ... （多段整体摘除 / 兄弟条目不误扩 / 既有 4 例回归）
```

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
