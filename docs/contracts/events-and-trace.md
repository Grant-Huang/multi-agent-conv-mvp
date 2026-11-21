# 共享契约：事件与追踪（草案）

| 项目 | 内容 |
|---|---|
| 状态 | 草案，待用户确认 |
| 核对基线 | 远端 `master` 的 `34fb1ab`（2026-11-21） |
| 适用特性 | 日志与诊断、AI 桌面事件流、MOM 事件、World Adapter 观测与执行、运营频道、业务内省 |

## 1. 范围

日志事件怎么命名、用哪些标识把一件事从头串到尾、实时事件流怎么续读和去重、错误码怎么写。

## 2. 现状

| 部分 | 标识与格式 | 位置 |
|---|---|---|
| 执行引擎日志 | 固定字段 `event`（“模块.动作”）、`code`（`E_` 错误、`W_` 警告）、`conv_id`、`task_id`、`agent_id`、`node_id`；主程序和节点用同一个 `task_id` | [DESIGN.md](../DESIGN.md) 第 10 节 |
| AI 桌面事件流 | `event_id`、`cursor`、`type`、`entity_id`、`entity_version`、`occurred_at`；游标过期回 410 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) 第 5 节；决定 67 |
| MOM | `case_id`、事件 `seq`；发件箱游标按消费者区分 | `crates/runtime/src/mom/engine.rs`；决定 58 |
| World Adapter | `request_id`、`proposal_id`、`execution_id`、外部关联号；游标绑定租户、世界、流、范围和后端实例；错误码用 `INVALID_ARGUMENT` 一类写法 | [mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) 第 4、5、8 节 |

问题：一件事从对话进入、变成案例、再变成执行时，三段各用各的标识，日志查不通（SYSTEM_GAPS G10）；错误码有 `E_` 前缀和 `INVALID_ARGUMENT` 两种写法；新增事件名没有登记到 DESIGN 第 10.3 节的情况多次出现（台账 R-20261105-2、R-20261120-L6）。

## 3. 已确认的不变量

| # | 不变量 | 来源 |
|---|---|---|
| E1 | 日志事件名用“模块.动作”格式，并在 DESIGN 第 10 节登记；新增埋点关联 `task_id` | DESIGN 第 10 节；`AGENTS.md`「日志」 |
| E2 | 同一个事件 ID 只处理一次；实体旧版本不能覆盖新版本 | ai-desktop/DESIGN 第 5 节 |
| E3 | 先提交权威状态和待发布记录，再投递事件 | ai-desktop/DESIGN 第 5 节 |
| E4 | 续读从游标之后开始、无遗漏无重复；游标过期时明确告知，由调用方重新取快照 | ai-desktop/DESIGN 第 5 节；WORLD_ADAPTER 第 4.2 节 |
| E5 | 日志只记凭据引用和必要业务信息，不记正文和凭证 | WORLD_ADAPTER 第 8 节；DESIGN 第 10.3 节 |

## 4. 建议补充的不变量（助手建议，待确认）

| # | 不变量 | 理由 |
|---|---|---|
| R1 | 引入一个贯穿的 `trace_id`：从桌面受理一条消息开始生成，经任务、案例、提案、执行一路携带，所有日志和事件都带上 | G10；World Adapter 正在实施，越早加越便宜 |
| R2 | 各特性保留自己的业务标识（`task_id`、`case_id`、`execution_id`），同时记录它们之间的对应关系 | 不改现有标识，只加关联 |
| R3 | 统一错误码风格：对外接口保留各自风格，但日志里的 `code` 一律用 `E_` / `W_` 前缀 | 诊断页按错误码统计需要一种写法 |
| R4 | 新增事件名或错误码时，由 CI 检查是否已在 DESIGN 第 10 节登记 | 台账里同类问题重复出现 |

## 5. 待决定

1. `trace_id` 从哪里生成：桌面受理请求时（推荐），还是每个子系统各自生成再关联？
2. World Adapter 对外错误码是否改成 `E_` 前缀写法，还是只在日志层映射？

## 6. 验收

确认后为每条不变量补用例编号；R1 可用 GS1 黄金场景验收：用一个 `trace_id` 查出从会话到执行回执的完整时间线。
