# 七层围栏草案原文（2026-11-21，已并入，存档）

| 项目 | 内容 |
|---|---|
| 状态 | **已归档**：内容已按仓库事实改写并入正式文档，本目录只作来源存档，不作为依据 |
| 提供者 | 用户，2026-11-21（在仓库外起草） |
| 并入位置 | [DEV_STANDARD.md](../../../DEV_STANDARD.md)（研发流程部分）、[docs/fencing/README.md](../../fencing/README.md)（围栏部分）、[GUARDRAIL_REGISTRY.yaml](../../fencing/GUARDRAIL_REGISTRY.yaml)、[GAP_ANALYSIS.md](../../fencing/GAP_ANALYSIS.md) |
| 决策 | [decisions/2026-11-21-dev-standard-and-guardrails.md](../../decisions/2026-11-21-dev-standard-and-guardrails.md) |

| 文件 | 内容 |
|---|---|
| [AGENT_GUARDRAILS_MASTER.md](AGENT_GUARDRAILS_MASTER.md) | 《Agent 自主开发七层围栏：审计、建设与持续治理规范》v1.0 提案 |
| [IMPLEMENTATION_PLAYBOOK.md](IMPLEMENTATION_PLAYBOOK.md) | 《七层围栏：Claude Code / Codex 实操指南》v1.0 |

## 并入时的主要改动

| 草案写法 | 正式文档的处理 | 原因 |
|---|---|---|
| 以 `DEV_STANDARD.md` v0.1 为输入 | 仓库原无此文件；把 `docs/spec/README.md`（研发对齐规范草案）移到根目录作为 `DEV_STANDARD.md` v0.2 | 只留一个规范主文件 |
| 围栏层写作 L1～L7，风险写作 R0～R3，文档层写作 L0～L3 | 围栏层用“第一层～第七层”；风险沿用现有 L0～L3（脚本与 CI 已在用），附与 R0～R3 的对照；文档层改为 D0～D3 | 消除三套 L 编号重名 |
| `trace_check.py`、`drift_check.py`、`guardrail_audit.py`、`scripts/preflight.sh` | 改为引用现有的 `web/verification/check-map.mjs`、`tools/ci/doc_lint.py`、`docs/fencing/PREFLIGHT.md`；缺的列入缺口 | 仓库中不存在这些文件 |
| 新建 Daily / Weekly / Milestone 工作流 | 先对应到现有的每晚评审 agent、`nightly-e2e.yml`、周五周检、版本说明准出 | 避免重复建设 |
| `docs/fencing/` 下约 10 个新文件 | 第一批只建 `GUARDRAIL_REGISTRY.yaml` 与 `GAP_ANALYSIS.md`（含路线图）；风险策略、审查策略、变更记录并入已有文件 | 避免再次分散 |
| RACI 用产品、QA、平台、SRE 等岗位 | 按真实的人（用户）和 agent 写 | 避免 owner 字段虚构 |
| 示例工作流用 `actions/checkout@v4` | 正式文档要求固定 commit SHA | 与仓库现行做法一致 |
| 六环（含“计划”） | 五环加独立的进度位置 | 防止计划文档重新承载设计 |

草案中引用的外部链接（GitHub Rulesets、Actions schedule、Claude Code、Codex）未在并入时逐一核实；正式文档未直接依赖它们。
