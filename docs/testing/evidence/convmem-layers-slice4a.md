# 分层记忆细化 片 4a：更正注记（RT-L2H-04，#346）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-05（2026-11-26 用户确认冻结；片 4/5 范围与做法 2026-11-26 会话经用户批准） |
| 用例 | RT-L2H-04（片 3 时已登记「实现前定义」，本片实现） |
| 运行模式 | 无头 local；进程内服务夹具；`cargo test -p an-server --test history_search` |

## 实现

- 新表 `message_corrections`（message_id PK / note / created_at，`CREATE TABLE IF NOT EXISTS` 随 open 建表，无补列迁移）；消息真删除时随删。
- `POST /api/desktop/v1/messages/{id}/correction {note}`：仅本人发送（role=user）消息；AI 回答 / 系统事件 403；空注记 400；未知消息 404；重复更正为替换（保留最新）。
- `search_history` 命中被纠正消息时附 `correction` 字段，并把被纠正命中排前（REQ-05「被纠正或撤回的带标记，优先给纠正后版本」的检索侧落地）；原文不改写（`messages` 表原文不变，更正单独成行）。

## Red 说明（如实）

更正注记端点与检索 `correction` 字段为本片新增：master 无该路由与字段，Red 以「端点缺失」形式存在（编译级）；用例已在片 3 登记为「实现前定义」（CASES RT-L2H-04 行），行为边界（403/400/404/替换/随删/优先排序/原文不改写）由测试断言直接验证。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
test result: ok. 4 passed; 0 failed; 0 ignored; ... finished in 0.07s
```

（RT-L2H-01～03 回归同绿。）

## 门禁

- fmt / clippy（-D warnings）/ check-map：pass（全量见 PR 评论）
