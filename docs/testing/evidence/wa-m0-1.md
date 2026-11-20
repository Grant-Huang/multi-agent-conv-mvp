# World Adapter M0-1 — 协议 / 实验 Mock HTTP

## 范围

- 新增 `crates/server/src/mom_world_mock.rs`：`/api/world/mock/*`
- 合同测试 `crates/server/tests/mom_world_mock.rs`：RT-WA-01～08
- 配置 `[mom] world_mock`（默认 false）；loopback 且打开时由 `main` 挂载（测试控件关闭）
- 不改 `engine.rs`

## Red → Green

实现前：路由不存在 / 断言失败（业务缺合同）。  
实现后：

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_mock
running 8 tests
… ok
test result: ok. 8 passed; 0 failed
```

## 合同覆盖

| Case | 要点 |
|---|---|
| RT-WA-01 | 非法时长；试跑 `delivery_count=0` |
| RT-WA-02 | 正式启动幂等；试跑≠正式 |
| RT-WA-03 | Observe/Subscribe；游标过期 `CURSOR_EXPIRED` |
| RT-WA-04 | Propose/Execute 幂等 / Confirm |
| RT-WA-05 | 运营员禁实验控制；跨运行 Execute 拒绝 |
| RT-WA-06 | 丢回执 `unknown` → 部分失败 `failed` |
| RT-WA-07 | 暂停等待≠失败；step 后 Confirm `applied` |
| RT-WA-08 | 标准答案仅审核可见 |

## 后续

M0-2/M0-3 接前端与真实 OpsPublisher 投影；M1 替换适配器时保留同合同套件。
