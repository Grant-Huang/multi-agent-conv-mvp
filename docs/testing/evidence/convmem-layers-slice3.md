# 分层记忆细化 片 3：L2 历史记录检索与本人消息删除（#346）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-05（2026-11-26 用户确认冻结；实现方案 2026-11-26 会话中经用户批准） |
| 用例 | RT-L2H-01～03（实现即测试）；RT-L2H-04（纠正注记与撤回标记，实现前定义，片 4/5） |
| 运行模式 | 无头 local；进程内服务夹具（文件库 + 独立 rusqlite 连接回填旧时间戳）；`cargo test -p an-server --test history_search` |

## 方案（用户批准）

- `GET /api/desktop/v1/history/search?q=&days=&limit=`：检索面 = 本库会话消息 + 已完成任务回执（单用户桌面库；多用户隔离随服务端身份，REQ-03/第 4 线）；默认窗口 180 天，`days` 显式加大才返回更早；AI 历史回答 `kind = 当时的回答`（CH-G5：只作资料不作事实源）；每条命中带 `msg_id` / `conv_id` / `at` 出处。
- `DELETE /api/desktop/v1/messages/{id}`：仅本人发送（role=user）消息，真删除（行删除）；AI 回答 / 系统事件不可删（403）；重复删除 404；任务、审批记录不在删除面（保留原件）。
- store.rs 新增 `search_history`（只读，instr 关键词匹配 + RFC3339 截点）与 `delete_message`（单条行删除）；无 schema 变更、无迁移。

## Red 说明（如实）

本片为**新增端点**：master 无 `/api/desktop/v1/history/search` 与 `DELETE /api/desktop/v1/messages/{id}` 路由，测试无法在实现前编译/路由缺失返回默认 404——Red 以「端点缺失」形式存在（编译级），无业务断言级 Red。删除/403/404 等行为边界由测试内断言直接验证（重复删除 404、AI 回答 403、空关键词 400）。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
test rt_l2h_03_default_window_180_days_with_explicit_override ... ok
test rt_l2h_01_past_answers_marked_and_sourced ... ok
test rt_l2h_02_deleted_message_gone_from_search_and_list ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; ... finished in 0.05s
```

RT-L2H-03 用独立 rusqlite 连接把探针消息 `created_at` 回填 200 天前，验证默认窗口不返回、`days=400` 显式指定返回且引用带原始时间。

## 边界与待办

- 纠正注记（更正单独记录、原文不改写、检索优先纠正后版本）→ RT-L2H-04，片 4/5。
- 「回到老任务按事件调入 L2 片段进上下文」→ 片 4 场合判断。
- 频道 / 共享会话接入 → 随 REQ-03 / 第 4 线服务端身份。

## 门禁

- `node web/verification/check-map.mjs` → Feature map valid（本片新增 RT-L2H-01～03 进 runtime_cases，测试函数已存在）
- 全量门禁与 CI：见 PR 评论
