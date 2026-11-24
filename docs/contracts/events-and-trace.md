# 共享契约：事件与追踪

| 项目 | 内容 |
|---|---|
| 状态 | **部分有效**（E1–E5、R1a、R2–R3 已确认；R1b 推迟到 World Adapter M4 写入前；R4 CI 强制登记仍建议） |
| 核对基线 | 远端 `master` 的 `43014db`（2026-11-22） |
| 适用特性 | 日志与诊断、AI 桌面事件流、MOM 事件、World Adapter 观测与执行、运营频道、业务内省 |
| 确认决策 | [2026-11-22-c2-m2-minimal-contracts](../decisions/2026-11-22-c2-m2-minimal-contracts.md)；[2026-11-24-trace-id-minimal-scope](../decisions/2026-11-24-trace-id-minimal-scope.md)（R1 拆分） |

## 1. 范围

日志事件怎么命名、用哪些标识把一件事从头串到尾、实时事件流怎么续读和去重、错误码怎么写。

## 2. 现状

| 部分 | 标识与格式 | 位置 |
|---|---|---|
| 执行引擎日志 | 固定字段 `event`（“模块.动作”）、`code`（`E_` 错误、`W_` 警告）、`conv_id`、`task_id`、`agent_id`、`node_id`；主程序和节点用同一个 `task_id` | [DESIGN.md](../DESIGN.md) 第 10 节 |
| AI 桌面事件流 | `event_id`、`cursor`、`type`、`entity_id`、`entity_version`、`occurred_at`；游标过期回 410 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) 第 5 节；决定 67 |
| MOM | `case_id`、事件 `seq`；发件箱游标按消费者区分 | `crates/runtime/src/mom/engine.rs`；决定 58 |
| World Adapter | `request_id`、`proposal_id`、`execution_id`、外部关联号；游标绑定租户、世界、流、范围和后端实例；错误码用 `INVALID_ARGUMENT` / `APPROVAL_INVALID` / `IDEMPOTENCY_CONFLICT` 一类写法 | [mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) 第 4、5、8 节；#199/#200 |

问题：一件事从对话进入、变成案例、再变成执行时，三段各用各的标识，日志查不通（SYSTEM_GAPS G10）；错误码有 `E_` 前缀和对外字符串两种写法；新增事件名没有登记到 DESIGN 第 10.3 节的情况多次出现（台账 R-20261105-2、R-20261120-L6）。

## 3. 已确认的不变量

| # | 不变量 | 来源 |
|---|---|---|
| E1 | 日志事件名用“模块.动作”格式，并在 DESIGN 第 10 节登记；新增埋点关联 `task_id`（有则必带） | DESIGN 第 10 节；`AGENTS.md`「日志」 |
| E2 | 同一个事件 ID 只处理一次；实体旧版本不能覆盖新版本 | ai-desktop/DESIGN 第 5 节 |
| E3 | 先提交权威状态和待发布记录，再投递事件 | ai-desktop/DESIGN 第 5 节 |
| E4 | 续读从游标之后开始、无遗漏无重复；游标过期时明确告知，由调用方重新取快照 | ai-desktop/DESIGN 第 5 节；WORLD_ADAPTER 第 4.2 节 |
| E5 | 日志只记凭据引用和必要业务信息，不记正文和凭证 | WORLD_ADAPTER 第 8 节；DESIGN 第 10.3 节 |
| R1a | 引入贯穿的 `trace_id`：**在 HTTP 受理请求时确定**（请求头 `X-Trace-Id` 合法则沿用，否则服务端生成），放进请求的 tracing 上下文并回写响应头。**C2/M2 及之后的写路径结构化日志必须带 `trace_id`**，同时带已有业务标识；旧路径触及时补齐，不设截止期。实现卡 TRACE-1，未落地前评审指向该卡，不判违约 | 决定 2026-11-22-c2-m2-minimal-contracts（D5）；[2026-11-24-trace-id-minimal-scope](../decisions/2026-11-24-trace-id-minimal-scope.md) |
| R1b | `trace_id` 写入事件表与执行记录、随事件流下发：**推迟到 World Adapter M4 生产写入开工前**，须另出改表方案（涉及 `store.rs`） | [2026-11-24-trace-id-minimal-scope](../decisions/2026-11-24-trace-id-minimal-scope.md) |
| R2 | 各特性保留自己的业务标识（`task_id`、`case_id`、`execution_id`），并持久记录已知对应关系。最小关联：`case_id` ↔ `execution_id`；若分流为平台任务再带 `task_id`。不强迫三套 ID 合一 | 同上；G10 |
| R3 | 对外接口错误码保留各自风格（如 `APPROVAL_INVALID`、`IDEMPOTENCY_CONFLICT`）；**日志里的 `code` 一律用 `E_` / `W_` 前缀**（可在边界映射） | 同上（D6） |

**标识权威**：

| 标识 | 权威拥有者 | 含义 |
|---|---|---|
| `task_id` | 执行引擎 / 桌面 Task | 对话侧工作单元 |
| `case_id` | MOM | 业务案例 |
| `execution_id` | World Adapter | 一次已批准写入的执行记录 |
| `proposal_id` / `approval_ref` | WA / 审批网关 | 提案与批准引用 |
| `run_id` | 桌面交互投影 | 一次运行尝试；业务事实仍归 MOM/WA |
| `trace_id` | 受理层 | 跨模块时间线关联键 |

## 4. 建议补充的不变量（助手建议，待确认）

| # | 不变量 | 理由 |
|---|---|---|
| R4 | 新增事件名或错误码时，由 CI 检查是否已在 DESIGN 第 10 节登记 | 台账里同类问题重复出现 |

## 5. 待决定

1. ~~旧路径补齐 `trace_id` 的截止里程碑~~：已定，触及时补齐、不设截止期（[2026-11-24-trace-id-minimal-scope](../decisions/2026-11-24-trace-id-minimal-scope.md)）。
2. R4 是否在文档检查过渡期结束后改为阻断。

## 6. 验收

| 不变量 | Case ID | 场景 |
|---|---|---|
| R2 | RT-WADES-09～11、RT-WADES-15 | 执行记录可对照 `case_id` / `execution_id` / 关联键 |
| R1a | RT-TRACE-01～03（待补，TRACE-1） | 同一次写路径日志含 `trace_id` 且能串到 `proposal_id`/`execution_id`（有则带 `case_id`、`task_id`）；不带请求头时服务端生成并回写；非法请求头被丢弃 |
| R1b | 待 M4 前方案 | 事件表与事件流带 `trace_id` |
| E2 / E4 | RT-ADREPLAY、RT-WADES-03 | 去重与游标过期 |
| R3 | 诊断抽查 | 对外码不变；日志 `code` 为 `E_`/`W_` |
