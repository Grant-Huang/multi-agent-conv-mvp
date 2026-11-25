# Agent 接入工作交给第 5 线，后续相关任务都派给第 5 线

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-25 |
| 来源 | 用户确认（2026-11-25 对话：“把 agent 接入工作任务安排给第 5 线，后面与此相关的任务就派给他”）；“Agent 接入”的边界划分为助手建议 |
| 状态 | 有效 |
| 影响范围 | [INTEGRATION_PLAN.md](../INTEGRATION_PLAN.md)「五条研发线与近期汇合」表第 5 行；[DESIGN.md](../DESIGN.md) 第 0.2 节分层表、第 0.3 节特性索引（“助手接入与管理（对话式）”“Agent 准入”）；派工规则（[2026-11-25-dispatch-to-workstreams](2026-11-25-dispatch-to-workstreams.md)） |
| 相关设计 | [DESIGN.md](../DESIGN.md) 第 7 节（agent 适配器，7.4 ACP 执行过程）、第 8 节（跨机器节点）、第 12 节（接入 ACP agent 的配置）、末尾 Agent 准入附节；[PLAN.md](../PLAN.md)「AG」一节 |

**一句话结论**：从 2026-11-25 起，“Agent 接入”归第 5 线：助手的发现、配置、诊断与对话式管理，Agent 准入，新 agent 类型（ACP、A2A、CLI 助手、跨机器节点）的接入与握手；执行组调度与执行协议仍归第 4 线。今后这类任务（缺陷、改进、测试缺口）一律派给第 5 线（GitHub Issue 标签 `line:5`）。

## 背景

[五线分工](2026-11-22-five-workstreams.md)中，第 5 线负责内省、分层记忆与会话上下文。`DESIGN.md` 第 0.3 节把“助手接入与管理（对话式）”和“Agent 准入”列为“横切”特性，没有归属的研发线；派工规则要求每条发现都按研发线表找归属，这类问题此前只能标 `line:2` 待定。

## 问题

Agent 接入的缺陷和改进没有固定的负责线，会无人认领或被顺手改。

## 决策依据

1. 用户 2026-11-25 的明确指示（见“来源”）。
2. 代码事实：助手接入与管理在 `crates/runtime/src/agent_catalog.rs`、`agent_config.rs`、`agent_diagnose.rs`、`agent_chat.rs`；准入在 `runtime/src/admission.rs`、`server/src/admission_api.rs`；适配器在 `crates/agents`（如 `acp.rs`），节点在 `crates/node`、`runtime/src/nodes.rs`（`DESIGN.md` 第 0.3 节表格）。
3. `DESIGN.md` 第 0.2 节把“agent 适配、节点”放在执行引擎层（第 4 线职责“执行协议与共享契约”），因此需要划清：**接入**（让一个 agent 能被发现、配置、通过握手与准入）归第 5 线，**执行**（编排、执行组、调度、质检返工）仍归第 4 线（推断：这是与原分层最少冲突的切法）。

## 定了什么

| 归第 5 线（Agent 接入） | 仍归第 4 线（执行） |
|---|---|
| 助手发现、配置、诊断、对话式管理（`agent_catalog / agent_config / agent_diagnose / agent_chat`） | 编排、执行组、调度与回退（`orchestrator.rs`、`registry.rs`） |
| Agent 准入（`admission.rs`、`admission_api.rs`） | 执行协议、共享契约中的任务与结果语义 |
| 新 agent 类型的接入与握手：ACP、A2A、CLI 助手、跨机器节点的连接与注册 | 质检、返工、人工验收、结果整合 |
| 设置“连接与能力”中助手部分的后端支持（界面由第 1 线做） | — |

- 跨两线的改动（例如改 `acp.rs` 的握手同时影响执行），主责第 5 线、配合第 4 线；`acp.rs`、`util.rs` 的子进程与 git 操作仍属 `AGENTS.md`“先停下来问人”。
- 业务系统、工具、模型服务的连接器（宪章 CH-G4）**不在本决定范围**，仍按原归属（World Adapter 属第 3 线）；连接器契约待建时另行确定。
- 已更新：`INTEGRATION_PLAN.md` 第 5 线一行；`DESIGN.md` 第 0.2、0.3 节相关行标“横切 · 第 5 线”。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 把 `crates/agents` 整体交给第 5 线 | 适配器的执行部分与执行组、回退强耦合，整体移交会让第 4 线失去执行协议的控制 |
| 新开第 6 线 | 用户指定第 5 线 |
| 连接器（CH-G4）一并交给第 5 线 | 用户只说了 agent 接入；连接器范围更大，涉及第 3 线 World Adapter，另行确定 |

## 影响与后续

- 已开 Issue 中暂无 Agent 接入类，无需改派。
- 今后审查发现的 Agent 接入问题标 `dispatch` + `line:5`。
- 第 5 线需补“助手接入与管理”的独立设计文档（`DESIGN.md` 第 0.3 节注明“无独立设计文档”），作为后续派工的验收依据。

## 出问题先看

- 某问题该归第 4 线还是第 5 线拿不准：按上表“接入 / 执行”判断；仍不清楚标 `line:2` 或问用户。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-25 | 新建 | 用户指示 |
