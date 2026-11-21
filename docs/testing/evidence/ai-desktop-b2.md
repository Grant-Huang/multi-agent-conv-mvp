# 证据：AI Desktop B2 — Run SSE / 断线重放

- **日期**：2026-11-20（America/Vancouver）
- **决定**：DECISIONS 67（用户确认方案）
- **范围**：`GET /api/desktop/v1/events?conversation_id=&after=`；store `list_conversation_events_after`；不改 `orchestrator.rs`；不建 `runs` 表；保留 `E2E-ADRESUME` Mock

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-runtime --lib conversation_events_after --locked
→ 1 passed

CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_replay --locked
→ 5 passed（RT-ADREPLAY-01～05）
```

## 断言摘要

| Case | 结果 |
|---|---|
| RT-ADREPLAY-01 | 回放缺口后续接 live；信封含 `event_id`/`cursor` |
| RT-ADREPLAY-02 | 流内去重；任务权威 status/`updated_at` 不因迟到事件回退 |
| RT-ADREPLAY-03 | 无效 `after` → 410 `cursor_expired` |
| RT-ADREPLAY-04 | 同库重启后 `after` 仍可补齐落库事件 |
| RT-ADREPLAY-05 | 会话过滤不串写 |

## 非本卡

- 删除 `runResumeMock` / Playwright 接真实 SSE
- orchestrator 检查点续跑；独立 Run/Artifact 表
- B3 ConversationTaskLink
