# 运营桌面 D1-1（desk_view + open_case）：Red 证据

日期：2026-11-19。

空实现：`desk_view` 恒返回 `state="ok"`、空 `rows`；`OpsPublisher::open_case` 恒返回空串。

```text
$ CARGO_TARGET_DIR=target cargo test -p an-runtime --test ops_desk rt_opsdesk_06 -- --nocapture
thread 'rt_opsdesk_06_open_in_channel_is_idempotent' panicked at crates/runtime/tests/ops_desk.rs:
消息 id 非空
test rt_opsdesk_06_open_in_channel_is_idempotent ... FAILED

$ CARGO_TARGET_DIR=target cargo test -p an-runtime --test ops_desk rt_opsdesk_07 -- --nocapture
thread 'rt_opsdesk_07_desk_view_matches_engine' panicked at crates/runtime/tests/ops_desk.rs:
assertion `left == right` failed
  left: "ok"
 right: "no_data"
test rt_opsdesk_07_desk_view_matches_engine ... FAILED
```

均为业务断言失败（不是编译错误或环境问题）。
