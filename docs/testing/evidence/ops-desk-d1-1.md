# 运营桌面 D1-1（desk_view + open_case）：证据

日期：2026-11-19。

## Red
见 `ops-desk-d1-1-red.md`：空实现下 RT-OPSDESK-06（消息 id 非空）、RT-OPSDESK-07（`state=no_data`）业务断言失败。

## Green
实现后：
```text
$ CARGO_TARGET_DIR=target cargo test -p an-runtime --test ops_desk
running 9 tests
test rt_opsdesk_01_classify_is_deterministic ... ok
test rt_opsdesk_04_switch_defaults_off ... ok
test rt_opsdesk_08_ensure_conversation_uses_fixed_id_and_is_idempotent ... ok
test rt_opsdesk_03_channel_messages_are_data_not_conversation ... ok
test rt_opsdesk_06_open_in_channel_is_idempotent ... ok
test rt_opsdesk_02_restart_neither_duplicates_nor_loses ... ok
test rt_opsdesk_07_desk_view_matches_engine ... ok
test rt_opsdesk_09_unread_counts_new_messages_until_read ... ok
test rt_opsdesk_05_skips_are_explained_and_cap_summarizes ... ok
test result: ok. 9 passed; 0 failed
```

`cargo clippy -p an-runtime --all-targets -- -D warnings`：通过。
