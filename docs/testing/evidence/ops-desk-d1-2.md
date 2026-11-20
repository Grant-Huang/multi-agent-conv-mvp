# 运营桌面 D1-2（服务端桌面接口）：证据

日期：2026-11-19。

## Red
见 `ops-desk-d1-2-red.md`：空实现下 desk 返回空 rows，与引擎案例集合不一致。

## Green
```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test mom_ops -- --nocapture
test rt_opsdesk_10_desk_api_matches_engine_and_open_is_idempotent ... ok
test result: ok. 1 passed; 0 failed
```

实现：`OpsState{engine,publisher}`；三条桌面路由；锁顺序先引擎后发布器；不存在案例 404。
