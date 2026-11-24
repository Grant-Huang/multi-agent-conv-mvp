# 证据：T-95 夹具门控与事件去重（FIX-C/H4）

- **日期**：2026-11-24（America/Vancouver）
- **授权**：#1 发号 T-95 + 方案确认（`docs/decisions/2026-11-24-review-h1-h4-fix-plan.md` 任务卡 FIX-C；store.rs/config.rs 已按决策获批）
- **Gap**：审查 R-20261124-H4（MOCK_REGISTRY M2 事件写入去重注记）；`resume/fixture/*` 生产可达
- **范围**：`store.rs`（write_events INSERT OR IGNORE + W_ 日志）；`config.rs`（`[desktop] test_fixtures`，默认 false）；`desktop_api.rs`（夹具事件 new_id、响应返回实际编号、路由条件挂载）；`harness.mjs`（测试配置打开开关）；`desktop_fixture_gate.rs`（新增）
- **模式**：真实 HTTP + SQLite；生产缺省（开关关闭）夹具接口 404

## 验收对照（方案任务卡 FIX-C）

| 验收 | 结果 |
|---|---|
| 第二次 seed 后，真实会话的事件全部能回放 | ✅（INSERT OR IGNORE 使重复编号不再整批回滚；RT-FIXC-01 附带断言两夹具会话各自 2 条） |
| 第二个夹具会话用自己的 after_cursor 订阅返回 200 | ✅ RT-FIXC-01 附带 HTTP 断言 |
| 开关关闭时 `/resume/fixture/seed` 返回 404 | ✅ RT-FIXC-02（含 replay-a/continue-b） |

## 命令与结果

### Red（实现前）

```
$ cargo test --locked -p an-server --test desktop_fixture_gate
2 failed（01: seed 两次 after_cursor/事件编号写死相同；02: 夹具接口未门控）
0.05s 快速失败，非环境问题
```

### Green（实现后）

```
$ cargo test --locked -p an-server --test desktop_fixture_gate
test rt_fixc_01_seed_twice_both_conversations_replayable ... ok
test rt_fixc_02_gate_closed_fixture_routes_404 ... ok
test result: ok. 2 passed; 0 failed ... finished in 0.05s
```

全量 workspace：44 个二进制全绿；唯一失败 `rt_adm_14`（10.05s 间歇指纹，已知已移交）。`desktop.spec` **32 passed**（E2E-ADRESUME 在门控路由 + 新事件编号下正常，harness 配置已打开开关）。全量 10 套件 E2E 结果见 PR 评论。

## 非目标

- 快照、记忆夹具的同类开关（另卡，可在本卡顺带未做——按方案保持独立）
- WA §4.4 提交窗口收敛（WA 线）；`mom_world_mock.rs` 同类问题 R-20261121-M9（另卡复用登记表）

## 出问题先看

- 夹具接口 404：检查 `desktop.test_fixtures` 是否打开（harness/测试配置）
- 日志 `W_ 事件写入忽略重复编号`：按编号查来源（正常防御，非错误）
