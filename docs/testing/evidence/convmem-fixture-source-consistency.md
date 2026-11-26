# 记忆夹具来源与确认状态一致性（INTENT-032 / #354）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory（配合 ai-desktop 记忆夹具）；需求 REQ-conversation-memory-02（L3 只存本人确认口径，见 #346 分层规则） |
| 用例 | RT-ADMEM-10、RT-ADMEM-11（新增）；RT-ADMEM-01/05/08/09、E2E-ADMEM、E2E-ADMEMW（随夹具口径同步） |
| 运行模式 | 无头 local；进程内服务夹具；`cargo test -p an-server --test desktop_mem` |

## 现象（修复前，master `6e784a4`）

`GET /api/desktop/v1/memories` 夹具重置后：

- `mem-2`「免打扰时段」`source: "设置 · 通知与主动性"`——设置项被当作个人记忆存了一份；在记忆里撤回它不会改变设置，同一件事出现两个来源。
- `mem-1`「质量报告偏好摘要格式」`source: "用户确认 · 2026-11-18"` 但 `confirmed: false`——来源文字与确认状态矛盾；按 CH-G1，未确认不应声称已确认。

## Red（先写断言看到失败）

新增 RT-ADMEM-10（列表不含设置来源条目）、RT-ADMEM-11（source 与 confirmed 一致），对修复前代码执行：

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_mem rt_admem_10
failures:
    rt_admem_10_fixture_has_no_settings_sourced_entries
test result: FAILED. 0 passed; 1 failed; ... finished in 0.04s

$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_mem rt_admem_11
failures:
    rt_admem_11_source_consistent_with_confirmation
test result: FAILED. 0 passed; 1 failed; ... finished in 0.03s
```

两条用例分别命中 issue 描述的两个缺陷（环境失败不算 Red：此为业务断言失败）。

## 修复

- `crates/runtime/src/store.rs` `desktop_memory_seeds`：移除 mem-2（设置项只存在于设置中心）；mem-1 来源改为系统生成的「用户保存 · 2026-11-18」（与 `POST /memories` 端点同一词汇；fact 不走确认闸门，不再声称「用户确认」）。
- 同步受夹具口径影响的既有用例：RT-ADMEM-01/05/08/09（改用 mem-1 或去掉 mem-2 断言）、E2E-ADMEM（来源断言改「用户保存」）、E2E-ADMEMW（撤回目标改 mem-1）。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_mem
test result: ok. 11 passed; 0 failed; 0 ignored; ... finished in 0.34s
$ node web/verification/check-map.mjs
Feature map valid: 33 features, 99 browser cases
```

## 未覆盖 / 边界

- E2E-ADMEM、E2E-ADMEMW 为浏览器用例，本地未跑 Playwright（本地门禁不含 e2e 执行），断言已随夹具口径更新，由 CI 执行。
- 「记忆里引用设置只显示『见设置』链接、不存副本」属第 1 线界面入口（#347），本 PR 不含界面改动。
- 记忆与设置混记的防线（外部候选不直接写入）在 #346 片 5（写入闸门）落地。
