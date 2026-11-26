# 分层记忆细化 片 4b：「带了哪些记忆、为什么」事件记录（RT-ASEL-02，#346）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-06（2026-11-26 用户确认冻结；片 4 范围与做法会话经用户批准） |
| 用例 | RT-ASEL-02（片 3 时登记「实现前定义」，本片实现）；RT-ASEL-01 保持实现前定义（六场合完整判定随对应入口线） |
| 运行模式 | 无头 local；进程内假模型 + 专属 tracing 订阅器（独立测试二进制）；`cargo test -p an-server --test materials_event` |

## 实现

- `session.materials` 事件补两个字段（DESIGN §10 事件表同步）：
  - `occasion`：程序按状态确定性判断（history 含当前消息，仅一条 = 「新会话」，否则 = 「会话续问」）——CH-P1 场合不交给模型猜；
  - `why`：各段入选原因码（`receipts:in_window` / `memories:effective_fact` / `style:effective_style` / `findings:related_gate`，分号分隔），与既有 `skipped`（跳过原因）互补，每轮可还原「带了哪些记忆、为什么」（CH-G1、G3）。
- 事件隐私口径不变：只记来源 id 与原因码，不记资料正文与用户正文。

## Red 说明（如实）

事件字段为本片新增：master 的 `session.materials` 无 `occasion`/`why`，断言「事件含 occasion 与原因码」在实现前必然失败——Red 以字段缺失形式存在（事件缺失级）；字段落地后的行为由测试断言直接验证（首轮=新会话、后续=会话续问、三段原因码齐全、未注入段不出现、无正文泄漏、来源 id 可追溯）。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test materials_event
test result: ok. 1 passed; 0 failed; 0 ignored; ... finished in 0.03s
```

## 边界

- 「回到老任务」「审批写入」「运营值班」「跨设备续接」场合的完整判定（RT-ASEL-01）随对应入口线（第 4 线会话边界 / 第 3 线值班 / 第 1 线跨设备入口）落地；本线已交付的场合规则：执行不带 L5（RT-EXEC-05）、随口问答不带 L5 经相关性门天然成立（RT-UFND-01）。
- 事件展示入口归第 1 线（#347）。

## 门禁

- fmt / clippy（-D warnings）/ check-map：pass（全量见 PR 评论）
