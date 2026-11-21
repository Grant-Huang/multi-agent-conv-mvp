# 共享契约：状态与取消（草案）

| 项目 | 内容 |
|---|---|
| 状态 | 草案，待用户确认 |
| 核对基线 | 远端 `master` 的 `34fb1ab`（2026-11-21） |
| 适用特性 | 对话任务、AI 桌面的 Run、MOM 案例、World Adapter 执行、业务内省任务 |

## 1. 范围

所有有生命周期的对象：怎么进入终态、怎么取消、怎么重试、服务重启后怎么办、迟到的事件和回执怎么处理。

## 2. 现状

| 对象 | 状态 | 重启后 | 位置 |
|---|---|---|---|
| 对话任务 | CREATED → QUEUED → RUNNING → VERIFYING → COMPLETED / FAILED / CANCELLED，另有 NEEDS_HUMAN | 未结束的任务标为 FAILED，不续跑 | [DESIGN.md](../DESIGN.md) 第 6 节 |
| AI 桌面 Run | queued → running → completed / failed / cancelled，运行中可到 waiting_input / waiting_approval / paused | 有持久队列和检查点后才承诺续跑 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) 第 6 节 |
| MOM 案例 | 阶段用字符串表示（如 CANCELLED、BLOCKED、PARTIAL） | 持久事件队列，可恢复 | `crates/runtime/src/mom/`；台账 R-20261106-L6 |
| World Adapter 执行 | accepted → applied → confirmed，另有 rejected / failed / unknown | 远程 DES 仅在工厂实例存活时可重放 | [mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) 第 5 节 |
| 内省任务 | 租约、超时、取消 | 租约过期后回收（台账 R-20261120-H1 指出默认租约过长） | [memory/INTROSPECTION_PLAN.md](../memory/INTROSPECTION_PLAN.md) |

已知问题：NEEDS_HUMAN 同时表示“待批准”和“审查要人看”，后者没有出口（DESIGN 第 15 节 #4，SYSTEM_GAPS G04）；已取消的案例仍向工厂下发写操作，失败回执会把 CANCELLED 改成 PARTIAL（R-20261120-H2）。

## 3. 已确认的不变量

| # | 不变量 | 来源 |
|---|---|---|
| S1 | 终态（完成、失败、取消）进入后不再改变；每次状态变化都写明允许从哪些状态来，由一次带条件的更新完成 | DESIGN 第 6 节 |
| S2 | 取消与完成同时发生时，先写进数据库的算数；已取消的不再写结果、不再交付 | DESIGN 第 6 节 |
| S3 | 取消后，尚未发送的外部动作停止提交；已经发出的继续保存事实回执，只更新执行证据，**不改案例终态** | WORLD_ADAPTER 第 4.4 节 |
| S4 | 迟到事件和较旧的回执不得覆盖新状态（带版本的状态更新） | WORLD_ADAPTER 第 5 节；ai-desktop/DESIGN 第 6 节 |
| S5 | 普通重试创建新的 Run；有外部副作用时先确认旧尝试的结果，无法确认就等人核对 | ai-desktop/DESIGN 第 6 节 |
| S6 | 无法判定外部写入结果时进入 `unknown`，保留原关联键，不盲目重发 | WORLD_ADAPTER 第 5 节 |
| S7 | 没有死胡同状态：任何状态用户都有出口 | [INTEGRATION_PLAN.md](../INTEGRATION_PLAN.md) 第一节（完成的定义） |

## 4. 建议补充的不变量（助手建议，待确认）

| # | 不变量 | 理由 |
|---|---|---|
| T1 | 一个状态只表达一种含义：“待批准”和“待验收”分成两个状态 | G04；INTEGRATION_PLAN WP2 |
| T2 | 状态用类型（枚举）表示，不用字符串比较 | R-20261106-L6 |
| T3 | 每个对象的状态文档都写明“重启后怎么办”：续跑、明确失败，还是保持等待 | 对话任务与 MOM 现在语义不同（G08） |
| T4 | 占用资源的状态（预留、租约）都有到期时间 | R-20261106-M1、R-20261120-H1 |
| T5 | 没有经过审查的完成，在界面和交付文案上与“已验收”区分 | 2026-11-21 设计评审 D4 |

## 5. 待决定

1. 对话任务重启时：继续“标为失败”，还是做续跑？（涉及 `orchestrator.rs`、`store.rs`，须另出方案。）
2. 对话任务和桌面 Run 是否统一为一个模型（任务 = 多次 Run）？

## 6. 验收

确认后为每条不变量补用例编号；S3 可复用台账 R-20261120-H2 的复现用例。
