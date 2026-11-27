# 免 LLM 历史消息批量导入（R-W2 / #440）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory；L2 历史记录的测试基建（REQ-conversation-memory-05 用例族） |
| 用例 | RT-L2H-05（新增，含生产 404 与校验对照） |
| 来源 | 第 6 线 #425 W2（docs/testing/LOCAL_E2E.md §3.2；证据 local-dataset-2026-11-26.md） |
| 运行模式 | 无头 local；fixtures 开/关双配置 + 进程内服务夹具；`cargo test -p an-server --test history_search` |

## 缺陷

长程 / 可部署性测试需要「每人 30 天、每天 3～10 轮」历史会话，现有公开 API 全部走模型（`POST .../messages` 触发推理），无法批量造历史。第 6 线以「按日会话壳 + 有限 live 轮次」降级。

## 实现（采纳 issue 建议方案 A）

- `POST /api/desktop/v1/history/import`（**test_fixtures 门控**，生产配置不挂载 → 404）：按会话批量写入 `messages`（role / content / created_at），只写库、不触发任何推理；单次上限 500 条；角色限 user / assistant / system（400）；会话不存在 404；`conversation_id` 缺省时新建「导入」会话。
- store：`import_messages` 事务批量插入（id 服务端生成、created_at 保留原始值——检索引用带原始时间）。
- 口径：导入的消息即 L2 历史记录，检索 / 更正 / 删除语义与 RT-L2H-01～04 一致。

## Red（修复前，master `60f7b42` + 仅新增测试）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
rt_l2h_05_batch_import_without_llm ... FAILED
rt_l2h_05_import_gated_and_validated ... FAILED
test result: FAILED. 4 passed; 2 failed; ... finished in 0.08s
```

（端点缺失：POST 返回 axum 默认 404。）

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
test result: ok. 6 passed; 0 failed; ... finished in 0.08s
```

断言覆盖：导入 200（imported=3、按序入库）；检索按原始 created_at 命中；生产配置 404；非法角色 400；全程零模型请求。

## 门禁

fmt / clippy（-D warnings）/ check-map（115→118 browser cases 增长来自并行合并，非本 PR）/ doc_lint：pass（全量见 PR 评论）
