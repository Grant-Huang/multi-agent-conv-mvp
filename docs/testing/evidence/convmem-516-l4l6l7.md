# 撤回清扫不阻断 + 导入限流 + 示例对齐（R-20261127-L4+L6+L7 / #516）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory / ai-desktop |
| 用例 | RT-L2H-06（导入限流）；L4 行为由既有撤回测试回归覆盖 |
| 来源 | 夜间评审 2026-11-27（PR #497）低优先级清单；台账 id=R-20261127-L4/L6/L7 |
| 运行模式 | `cargo test -p an-server --test history_search` |

## 三件事

### L4：清扫失败不阻断撤回
store.rs revoke 中 `cleanup_revoked_memory_from_contexts` 的 `?` 传播改为 `match` + `tracing::error`（含 memory_id），清扫失败撤回仍成功——对齐决策「清扫失败不阻断撤回」。

### L6：导入接口 unwrap + 限流
- `create_conversation("导入").unwrap()` → `match` + 500 兜底（不再 panic）
- 单条正文上限 **32KB**（400）、批量上限 500 条（既有验证保留）

### L7：示例文件对齐
`config.example.toml` 的 `connectors_path` 写在 `[introspection]` 段且代码无此字段——改为注释说明实际读取路径（环境变量 `CONNECTORS_TOML`，默认 `connectors.toml`）。

## 测试

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
test result: ok. 7 passed; 0 failed; ... finished in 0.05s
```

（RT-L2H-06 新增：单条 >32KB → 400；501 条 → 400。既有 RT-L2H-01～05 回归全绿。）

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
