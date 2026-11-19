# 运营桌面 / 运营频道 D0a（后端）：实现前 Red 证据

日期：2026-11-19。命令：`CARGO_TARGET_DIR=target cargo test -p an-runtime --test ops_desk`。

实现前先放了骨架（类型和签名、故意不完整的行为，`ChatCard::OpsCase/OpsSummary` 和 `[mom]` 配置结构，`ensure_conversation` 用随机 id），使测试能编译，失败点落在业务断言上：

| Case | 结果 | 失败的业务断言 |
|---|---|---|
| RT-OPSDESK-01 | 失败 | 分级函数对“SafetyBlocked”返回 P1，应为 P0 |
| RT-OPSDESK-02 | 失败 | 第一次 tick 没有发出卡片（0,0），应为 (1,0) |
| RT-OPSDESK-03 | 失败 | 运营频道会话不存在 |
| RT-OPSDESK-04 | 失败 | 打开开关后合并窗口默认值为 0，应为 60 |
| RT-OPSDESK-05 | 失败 | 第一次 tick 没有发出卡片（0），应为 1 |
| RT-OPSDESK-08 | 失败 | 会话 id 是随机值，应为固定的 `ops-channel` |

6 个全部失败，且都是业务断言失败，不是编译或环境错误。
