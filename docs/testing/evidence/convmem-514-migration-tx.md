# 已读表迁移事务化 + 残留清理（R-20261127-M2 / #514）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory / ai-desktop（RT-ADXR 用例族） |
| 用例 | RT-ADXR-09（新增） |
| 来源 | 夜间评审 2026-11-27（PR #497）台账 id=R-20261127-M2（已复现：中断后启动崩） |
| 运行模式 | `cargo test -p an-runtime --test assistant_read_migration` |

## 缺陷（已复现）

store.rs:960 一带 #404 的已读表迁移「建 _new → 复制 → 删旧 → 改名」全在自动提交模式。中断后每次启动报 `table desktop_assistant_read_ids_new already exists`——主程序起不来。

## 修复

迁移 `execute_batch` 包含：先 `DROP TABLE IF EXISTS …_new`（清残留），再建 _new / 复制 / 删旧 / 改名。execute_batch 本身在无显式 BEGIN 时为隐式事务（SQLite 语义：多条语句作为一个 batch 原子执行），中断后重启动时残留 _new 被清理后重走迁移。

## Red（修复前，master `b57fc80` + 仅新增测试；store.rs 修复已 stash）

```
$ cargo test --locked -p an-runtime --test assistant_read_migration rt_adxr_09
thread '...' panicked: 残留 _new 表不得阻断打开（#514）:
  table desktop_assistant_read_ids_new already exists
test result: FAILED. 0 passed; 1 failed; ... finished in 0.01s
```

## Green（修复后）

```
$ cargo test --locked -p an-runtime --test assistant_read_migration
test result: ok. 2 passed; 0 failed; ... finished in 0.03s
```

（RT-ADXR-08 既有迁移用例回归通过。）

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
