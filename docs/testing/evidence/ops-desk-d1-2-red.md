# 运营桌面 D1-2（服务端桌面接口）：Red 证据

日期：2026-11-19。

空实现：`GET /api/mom/desk` 恒返回空 `rows`；`GET …/cases/{id}` 恒 200 空对象；`POST …/open-in-channel` 恒返回空 `message_id`。

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test mom_ops -- --nocapture
thread 'rt_opsdesk_10_desk_api_matches_engine_and_open_is_idempotent' panicked:
assertion `left == right` failed: 桌面 rows 与 /mom/api/mom/cases 案例集合相同
  left: ["CASE-s1-1"]
 right: []
test rt_opsdesk_10_desk_api_matches_engine_and_open_is_idempotent ... FAILED
```

业务断言失败（不是编译错误或环境问题）。
