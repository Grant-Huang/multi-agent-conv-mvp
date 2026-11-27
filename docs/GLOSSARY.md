# 术语与编号表

| 项目 | 内容 |
|---|---|
| 地位 | 本仓库缩写、编号前缀与内部术语的**唯一解释处**（`DEV_STANDARD.md` 第 3.1 节）。PR、Issue、文档第一次出现时附一句说明或链接到本表 |
| 规则 | 不在本表里的缩写，不得出现在 PR / Issue 标题和“给审查人”一节；新造缩写先在本表登记（L0，随所在 PR 一起提交） |
| 来源 | 用户 2026-11-26 要求“缩写不是我定义的，得花很长时间对齐上下文”；见 [决定 2026-11-26-pr-readability](decisions/2026-11-26-pr-readability.md) |

## 1. 容易混淆的两组编号

| 写法 | 含义一 | 含义二 | 怎么写才不混 |
|---|---|---|---|
| L0～L3 / L0～L5 | **风险等级** L0～L3（改动风险，决定谁审批） | **记忆层** L0～L5（工作现场 … 内省发现） | 写“风险 L2”“记忆层 L2” |
| P0～P3 | **严重度** P0～P3（夜间评审发现的严重程度） | **实施阶段** P0～P3（集成流水线分期） | 写“严重度 P1”“阶段 P1” |

## 2. 编号前缀

| 前缀 / 写法 | 是什么 | 权威位置 |
|---|---|---|
| 第 N 线、`line:N` | 研发线编号（当前 6 条） | [INTEGRATION_PLAN.md](INTEGRATION_PLAN.md)「研发线与近期汇合」 |
| 风险 L0～L3 | 改动风险等级：L0 文档、L1 普通业务代码、L2 核心状态与公共接口、L3 治理与 CI | `config/risk-policy.toml`；[DEV_STANDARD.md](../DEV_STANDARD.md) 第 6 节 |
| `severity:high/medium/low` | 派工 Issue 的紧急程度 | [决定 2026-11-25-dispatch-to-workstreams](decisions/2026-11-25-dispatch-to-workstreams.md) |
| CH-N / CH-P / CH-U / CH-G + 数字 | 产品宪章条款：N 方向、P 原则、U 体验、G 成长与边界（如 CH-G2c = G2 第 c 款） | [PRODUCT.md](PRODUCT.md) |
| INTENT-YYYY-NNN | 产品意图审查的一条发现（界面或行为偏离宪章） | `docs/review/product-intent/` 各基线报告 |
| UX-NN、UX-NN-In | 体验地图的一条用户旅程 / 旅程中的第 n 条约束 | [experience-map](experience-map/README.md) |
| UXFIX-字母 | 2026-11-25 派给第 1 线的界面修复卡旧称，现以 Issue 编号为准 | [决定 2026-11-25-dispatch-to-workstreams](decisions/2026-11-25-dispatch-to-workstreams.md) |
| E2E-*、RT-* | 测试用例编号：E2E 浏览器端到端、RT 运行时（Rust）测试 | [testing/CASES.md](testing/CASES.md) |
| REQ-特性-NN | 特性档案里的一条需求 | `docs/features/<特性>/README.md` |
| R-YYYYMMDD-字母数字 | 夜间评审台账的一条发现（H/M/L 大致对应高/中/低） | `docs/review/findings.jsonl`；[REVIEW.md](../REVIEW.md) |
| 严重度 P0～P3 | 夜间评审发现的严重程度：P0 安全突破 … P3 文档偏差 | [REVIEW.md](../REVIEW.md) 第 4 节 |
| GR-层-名称 | 围栏登记表的一条围栏（如 GR-4-CI-BUDGET = 第四层“CI 耗时预算”） | [fencing/GUARDRAIL_REGISTRY.yaml](fencing/GUARDRAIL_REGISTRY.yaml) |
| 第 N 层（围栏） | 七层围栏：需求、知识、执行隔离、自动验证、AI 审查、分级审批、发布监控 | [fencing/README.md](fencing/README.md) |
| T0～T3 | CI 分层：T0 本地自检、T1 每个 PR 的快速检查、T2 合并前组合验证、T3 夜间全量 | [fencing/INTEGRATION_PIPELINE.md](fencing/INTEGRATION_PIPELINE.md) 第 5 节 |
| 阶段 P0～P3 | 集成流水线的实施分期 | 同上第 9 节 |
| W1～W6、D1～D9、F1～F7 | 第 6 线工作类别 / 可部署性检查项 / 故障注入项 | [testing/LOCAL_E2E.md](testing/LOCAL_E2E.md) |
| WP0～WP8 | 集成计划的工作包 | [INTEGRATION_PLAN.md](INTEGRATION_PLAN.md) |
| M0～M4 | World Adapter 里程碑（M3 只读接入、M4 写入） | [mom/WORLD_ADAPTER_PLAN.md](mom/WORLD_ADAPTER_PLAN.md) |
| 5.1、5.2a、5.2b、5.3 | 分层记忆特性的交付切片 | [features/conversation-memory/PLAN.md](features/conversation-memory/PLAN.md) |
| 记忆层 L0～L5 | 工作现场、会话摘要、历史记录、个人长期记忆、组织知识、内省发现 | [决定 2026-11-26-memory-layers-and-history](decisions/2026-11-26-memory-layers-and-history.md) |
| `baseline-YYYYMMDD` | 夜间全量全绿后打的基线 tag，长程测试与走查基于它 | [fencing/INTEGRATION_PIPELINE.md](fencing/INTEGRATION_PIPELINE.md) 第 7 节 |

## 3. 内部术语

| 术语 | 白话解释 | 权威位置 |
|---|---|---|
| AI 桌面 | 产品唯一主入口：提问、委派、取证、审批都在这里 | [ai-desktop/DESIGN.md](ai-desktop/DESIGN.md) |
| C-Agent / O-Agent / S-Agent | 会话前台（接话的那个）/ 编排执行（派活的）/ 结果整合（汇总的） | [ai-desktop/DESIGN.md](ai-desktop/DESIGN.md) 第 6 节 |
| Decision Harness | 判断用户意图、关联任务、选路由的规则层 | [DESIGN.md](DESIGN.md) 第 5、9 节 |
| 执行组 | 一个任务分给多个 agent 执行时的分组与回退 | [DESIGN.md](DESIGN.md) 第 7 节 |
| ACP / A2A / MCP | 接入外部 agent 的协议 / agent 之间委派的协议 / 给模型提供工具的协议 | [DESIGN.md](DESIGN.md) 第 8 节；[mcp/DESIGN.md](mcp/DESIGN.md) |
| DES | 仿真工厂（离散事件仿真） | [mom/DES_CLOSED_LOOP.md](mom/DES_CLOSED_LOOP.md) |
| MOM | 制造运营管理：案例、方案、审批与动作记录 | [mom/DESIGN.md](mom/DESIGN.md) |
| World Adapter | 统一访问“世界”（仿真工厂或真实工厂）的接口层 | [mom/WORLD_ADAPTER.md](mom/WORLD_ADAPTER.md) |
| NexusOps | 业务执行网关：经批准后把动作写到真实系统 | [ai-desktop/NEXUSOPS_REAL_WRITE_PLAN.md](ai-desktop/NEXUSOPS_REAL_WRITE_PLAN.md) |
| RelOS | 组织知识与关系库（设计中） | [DESIGN.md](DESIGN.md) 第 0.3 节 |
| DLP、外发脱敏 | 数据发往外部模型或工具前的敏感信息检查与脱敏 | [dlp/CONTRACT.md](dlp/CONTRACT.md) |
| 内省 | 系统从业务数据中自己找规律，产出“待核实”的发现 | [memory/INTROSPECTION_PLAN.md](memory/INTROSPECTION_PLAN.md) |
| convmem | “分层记忆与会话上下文”特性（conversation-memory）的简称 | [features/conversation-memory/README.md](features/conversation-memory/README.md) |
| 会话资料、资料块 | 每轮回答前由程序挑选、作为“数据”交给模型的背景材料 | 同上 |
| 风格卡 | 已确认的个人风格偏好，以一段固定格式附在回答提示里 | 同上 |
| 夹具（fixture） | 测试用的预置数据 | [testing/README.md](testing/README.md) |
| 影子审查 | CI 在 PR 上自动贴的风险等级与清单，只提醒不阻断 | [fencing/SHADOW_REVIEW.md](fencing/SHADOW_REVIEW.md) |
| 在制品 | 已开出、尚未合并的 PR | [fencing/INTEGRATION_PIPELINE.md](fencing/INTEGRATION_PIPELINE.md) 第 3 节 |
| 派工 Issue | 标签 `dispatch` + `line:N` 的任务卡；只挂主责线一个 `line:N` | [AGENTS.md](../AGENTS.md)“跨线分工” |
| 公告单 | 标题“[各线通知]”、标签 `announcement` 的长期 Issue，不关闭、按月换新 | [决定 2026-11-25-dispatch-to-workstreams](decisions/2026-11-25-dispatch-to-workstreams.md) 第 10 条 |
| 总 Issue / 子 Issue | 一项跨线工作拆成按线的子 Issue，挂在一个总 Issue 下 | 同上 |
