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

## Green（同日，实现后）

`CARGO_TARGET_DIR=target cargo test -p an-runtime --test ops_desk`：6 passed。

实现中只改了一处测试夹具：`pending_case` 起初对每个案例都用固定的事件时间 497 起算，同一引擎里第二个案例起走不到“待批准”（引擎时钟已经是 514）；改成以引擎当前时间为基准（与服务端场景生成一致），断言本身没有改。

完整门槛：
- `cargo test --no-fail-fast`：只有 `an-server --test admission` 的 `rt_adm_14_admission_http_end_to_end` 失败（“任务未结束”）；同一用例在干净的 `origin/master` 上同样失败，与本改动无关。
- `cargo clippy --all-targets`：无警告。
- `cd web && npm run build`：通过（`ChatCard.ts` 绑定已由 `cargo test -p an-core` 重新生成）。
