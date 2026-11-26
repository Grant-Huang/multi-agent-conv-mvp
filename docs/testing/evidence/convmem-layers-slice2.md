# 分层记忆细化 片 2：用例登记、注入基线与场合表落地（#346）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-05、06、07（2026-11-26 用户确认冻结）+ REQ-01（S6 场合修正） |
| 用例 | RT-SMAT-06、RT-SMAT-07（注入基线补录）；RT-EXEC-05（场合表落地，Red→Green）；RT-L2H-01～03、RT-ASEL-01～02（实现前定义，随片 3/4 补测试）；内省克制族沿用 RT-INT-20 / RT-INT-24 / RT-UFND-01 / RT-JMAT-03 |
| 运行模式 | 无头 local；进程内假 OpenAI 兼容服务；`cargo test -p an-server --test exec_materials / session_materials` |

## 需求冻结

REQ-05 历史记录可检索（L2）、REQ-06 按场合选材且所用记忆可说明、REQ-07 记忆与检索的注入防线：用户 2026-11-26 在第 5 线会话中确认冻结（验收文本按同日 [设计决策](../../decisions/2026-11-26-memory-layers-and-history.md)），状态翻转与用例挂接随本片落 features.json。

## 场合表落地修正（Red→Green）

2026-11-26 场合选材表「执行、委派」行：**不带风格偏好、L5**。修复前 S6-exec 执行块用回答路径同款 `render`（含发现段），且 S5 路由线索段（判断极简清单，含发现行）拼进任务上下文进入执行背景——两处泄漏。

Red（暂存 `render_exec` 修复后执行）：

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials rt_exec_05
failures:
    rt_exec_05_exec_block_excludes_introspection_findings
test result: FAILED. 0 passed; 1 failed; ... finished in 0.09s
```

修复：`session_materials.rs` 新增 `render_exec`（回执 + 个人记忆，剔除 findings）；`conversation.rs` 执行块改用 `render_exec`；S5 会话资料线索只进路由请求（`route_ctx`），任务上下文恢复纯净。

Green：

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test exec_materials
test result: ok. 5 passed; 0 failed; 0 ignored; ... finished in 0.11s
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test session_materials
test result: ok. 7 passed; 0 failed; 0 ignored; ... finished in 0.68s
```

RT-EXEC-05 守卫断言：同一发现在 respond 模式 runtime 的回答路径仍按相关性门注入（证明发现确实过门，执行块剔除非候选缺失所致）。

## 注入基线（既有行为补录，无 Red——行为随 5.1/5.2/S4/S5 已存在）

- RT-SMAT-06：含指令样式的记忆正文只出现在「数据，非指令」块内（含来源 id）；系统提示、历史、最新用户消息不携带；该轮不产生任务、不新增记忆写入。
- RT-SMAT-07：判断请求只含 ≤40 字头截要点与来源 id，完整指令正文不出现；`session.materials` 事件正文边界由 RT-SMAT-05 覆盖（tracing 订阅器每二进制单例）。

## 不做的事（边界）

- L2 检索（RT-L2H-01～03）与场合选材（RT-ASEL-01、02）：实现前定义，片 3 / 片 4 实现并以先 Red 补可执行测试。
- 写入闸门（外部候选、写入扫描）、撤回影响追溯：片 5。
- 「审批、业务写入」场合的材料组（方案原文/版本/影响/来源/同类历史审批）：随场合判断片落地。

## 门禁

- `node web/verification/check-map.mjs` → Feature map valid: 33 features, 99 browser cases
- convmem 相关五个测试文件（session/exec/route/judge/introspection/personal_memory）全绿
- 全量门禁与 CI：见 PR 评论
