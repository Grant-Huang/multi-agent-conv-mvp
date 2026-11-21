# AI Desktop B1 证据（可靠受理）

日期：2026-11-20（America/Vancouver）

## 范围

- `POST /api/desktop/v1/conversations/{id}/requests`：202；同 `client_operation_id` 幂等回放；改正文 409
- `GET /api/desktop/v1/requests/{id}`：丢回执后回读
- store 表 `desktop_client_operations`；首次受理后后台 `continue_after_accepted`（不重复插 user 消息）
- **未改**：`orchestrator.rs`；既有 `POST .../messages`；`E2E-ADACCEPT` Mock 断言

## 命令

```bash
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_accept --locked
cd web && npm run test:map
```

## 结果

| 项 | 结果 |
|---|---|
| RT-ADACCEPT-01～04 | 4 passed |
| test:map | 32 features, 59 browser cases |
| clippy an-runtime/an-server | 无本卡新增警告 |

说明：全量 `cargo test` 中 `rt_adm_14_admission_http_end_to_end` 在本机超时「任务未结束」；用 master 的 `conversation.rs` 复现同样失败，与本卡改动无关。

## 后续

- 前端「真实」开关接 desktop v1（可选；本卡未改 Playwright）
- B2 SSE/重启；B3 ConversationTaskLink
