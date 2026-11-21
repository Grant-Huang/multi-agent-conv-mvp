# 证据：AI Desktop B3 — 关联持久 + 基础配置

- **日期**：2026-11-20（America/Vancouver）
- **决定**：DECISIONS 68（用户确认方案）
- **范围**：ConversationTaskLink + preferences；不改 `orchestrator.rs`；保留 E2E-ADSCOPE/ADPROF Mock

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_b3 --locked
→ 7 passed（RT-ADLINK-01～04、RT-ADPREF-01～03）
```

## 断言摘要

| Case | 结果 |
|---|---|
| RT-ADLINK-01 | 201 建链；同操作 ID 重放同一 link_id |
| RT-ADLINK-02 | 改 conversation → 409 operation_conflict |
| RT-ADLINK-03 | 撤回幂等；默认列表隐藏已撤回 |
| RT-ADLINK-04 | 归属会话投影；重启后仍可见 |
| RT-ADPREF-01 | GET 默认 version=1 |
| RT-ADPREF-02 | PATCH 升 version 并持久化 |
| RT-ADPREF-03 | 过期 version → 409；不覆盖 |

## 非本卡

- 前端接真实 links/preferences；删除 profileStore/draftStore Mock
- 记忆建议 / Attention（C2）；草稿云同步
