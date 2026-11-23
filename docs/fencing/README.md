# 七层围栏：审计、建设与持续治理

| 项目 | 内容 |
|---|---|
| 版本 | v1.0，2026-11-21（用户确认） |
| 地位 | **唯一的围栏入口**：七层定义、检查节奏、围栏登记与生命周期。研发流程（需求→版本、风险分级、授权、证据等级）见 [DEV_STANDARD.md](../../DEV_STANDARD.md) |
| 现状清单 | [GUARDRAIL_REGISTRY.yaml](GUARDRAIL_REGISTRY.yaml)：每条围栏的证据等级、负责人、到期日 |
| 缺口与路线图 | [GAP_ANALYSIS.md](GAP_ANALYSIS.md) |
| 来源 | 用户 2026-11-21 起草的《七层围栏：审计、建设与持续治理规范》与《实操指南》（原文存档见 [explore/guardrails-draft-2026-11-21/](../explore/guardrails-draft-2026-11-21/README.md)），按本仓库事实改写；背景材料 [AUTONOMOUS_DEV_ACTION_GUIDE.md](../AUTONOMOUS_DEV_ACTION_GUIDE.md) |
| 取代 | `7-fence-review.md` 第 1 节（七层落点）与第 3 节（差距清单）；该文件其余部分仍是定期检查操作手册 |

**总原则**：让 agent 在有效授权范围内连续完成任务；高后果错误在动作之前被技术措施挡住；普通 PR 只承担与本次改动有关的验证；每日处理增量质量；每周治理结构性风险；里程碑判断端到端质量与发布能力。**任何文字承诺都不能代替可信的 CI、GitHub 权限或运行环境控制。**

## 1. 四个可靠性断言

1. **有文档 ≠ 已执行**：声明了规则或存在脚本，不等于它被受保护的检查持续执行。
2. **有测试 ≠ 业务正确**：编号与文件存在（T1）、测试实际执行（T2）、断言符合业务规则（T3）要分级证明。
3. **另开会话 ≠ 独立执行权**：另开上下文可以减少自我合理化，但防止绕过要靠独立身份和受保护的工作流。
4. **定期检查 ≠ 事前保护**：权限、敏感数据、不可逆操作不能等到每日或每周才发现。

证据等级（DECLARED / IMPLEMENTED / VERIFIED / ENFORCED / NOT_VERIFIED）与 T1～T3 的定义见 [DEV_STANDARD.md](../../DEV_STANDARD.md) 第 8 节。

## 2. 七层定义与本仓库落点

层号用“第一层～第七层”，不用 L 前缀（L0～L3 是风险等级，D0～D3 是文档层级，见 DEV_STANDARD 第 2 节）。

| 层 | 核心责任 | 本仓库现有控制 | 负责 |
|---|---|---|---|
| **第一层 需求边界与验收** | 授权、非目标、可验证的完成标准 | 授权规则（DEV_STANDARD 第 5 节）；`docs/testing/CASES.md`、`features.json`；PR 模板“关联”“非目标” | 用户 |
| **第二层 产品知识与架构契约** | 防止绕过架构、接口与状态语义分裂 | `docs/PRODUCT.md`、`docs/DESIGN.md` 第 0 节、`docs/contracts/`（草案）、`docs/decisions/`、功能地图 | 用户；实现会话起草 |
| **第三层 开发执行与隔离** | 防止越界修改、泄密、误用高危工具 | `AGENTS.md`“修改范围”“先停下来问人”；云会话沙箱与分支限制；`change-policy` 规模检查 | 用户（平台设置） |
| **第四层 确定性自动验证** | 可重现、可机器判定的基础质量 | `.github/workflows/ci.yml`：`rust`、`web`、`python-websearch`、`python-dlp-egress`、`python-tools`、`change-policy`、`security`；`nightly-e2e.yml` | 实现会话；用户维护必需检查 |
| **第五层 独立 AI 审查** | 语义缺陷、缺失场景、回归 | 每晚评审 agent（`REVIEW.md`，事后）；`pr-shadow-review.yml`（风险分级与清单，非 AI 语义审查，不阻断） | 评审 agent；用户复核 |
| **第六层 风险分级与授权审批** | 正确分配人机决策权，防止自批 | `tools/ci/risk_classify.py` 与 `config/risk-policy.toml`（L0～L3）；`.github/CODEOWNERS`；分支保护 | 用户 |
| **第七层 发布、监控与恢复** | 版本可追溯、运行可观测、故障可恢复 | `docs/DESIGN.md` 第 10 节日志与诊断；版本说明模板（`docs/spec/release-template.md`）；tag `v1.0.0` | 用户 |

**横向机制**：身份与最小权限；证据与审计记录；缺陷、事故与评测回灌（[EVAL_LOOP.md](EVAL_LOOP.md)）；授权契约（[模板](../spec/autonomy-contract-template.md)）；围栏有效性与退役（第 5 节）。

各层的现状与证据等级以 [GUARDRAIL_REGISTRY.yaml](GUARDRAIL_REGISTRY.yaml) 为准，本表只说明分工。

## 3. 检查节奏

| 节奏 | 目标 | 本仓库现在由什么执行 | 范围 |
|---|---|---|---|
| **执行前** | 不可延期的保护 | 云会话沙箱与分支限制；`AGENTS.md`“先停下来问人”（DECLARED）；凭据不进仓库（`security` 作业 gitleaks） | 每个动作 |
| **每个 PR** | 不把新风险带入主干 | `ci.yml` 全部作业；`change-policy`（风险分级、变更规模、提示注入扫描、文档检查）；影子审查评论 | 本次改动及可信影响范围；影响不明时扩大 |
| **每日** | 及时发现累计风险 | 每晚评审 agent：从上次截止提交到 `origin/master` 的增量（`REVIEW.md` 第 2 节）；`nightly-e2e.yml` | 默认分支增量；漏跑要补查，不只看最近 24 小时 |
| **每周** | 系统治理 | 周五周检（`7-fence-review.md` 第 2、4 节）；本目录的缺口与路线图更新 | 架构与登记表漂移、重复根因、围栏误报与成本；提出 1～3 条有证据的改进，也提出可删除的检查 |
| **里程碑** | 确认可交付 | 版本说明第 7 节准出结论（PASS / CONDITIONAL / BLOCKED） | 端到端、关键不变量、迁移与恢复 |

| 层 | 每个 PR | 每日 | 每周 | 里程碑 |
|---|---|---|---|---|
| 第一层 | 必：关联授权或需求；行为变化时验收 | 无归属的新行为、未闭合需求 | 需求→实现→测试链抽检 | 范围、非目标、关键业务验收 |
| 第二层 | 触：受影响的接口、状态、契约 | 设计漂移线索 | 跨模块依赖、双重语义、过时功能地图 | 关键不变量、接口兼容 |
| 第三层 | 执行前必：沙箱、修改范围、敏感路径 | 身份、CI、权限配置漂移 | 最小权限与工具能力复核 | 部署身份、访问控制 |
| 第四层 | 必：相关测试、lint、安全门禁 | 合并后回归、不稳定测试 | 覆盖盲区、故障注入轮换 | 端到端、异常与恢复 |
| 第五层 | L0 可省；L1 轻量；L2/L3 深度 | 高风险合并补审、复核发现 | 业务语义与架构深审、误报漏报复盘 | 独立质量评估 |
| 第六层 | 必：风险等级、必需审批、禁止自批 | 例外与绕过记录 | 分级阈值与误判 | 剩余风险与发布授权签署 |
| 第七层 | 仅部署或迁移相关改动 | 运行环境健康（有环境时） | 事故复盘、可观测缺口 | 回滚演练、发布准备 |

**即时升级**：任何节奏发现已证实的高后果问题（越权、敏感数据泄露、不可恢复损坏、严重安全漏洞），立即建阻断或冻结事项，写明负责人、期限和恢复标准，不能等到周检。

## 4. 风险分级与必需检查

风险等级、合并前必需检查、不随等级降低的底线、例外管理，统一见 [DEV_STANDARD.md](../../DEV_STANDARD.md) 第 6 节。本节不重复。

## 5. 围栏登记与生命周期

**登记表** [GUARDRAIL_REGISTRY.yaml](GUARDRAIL_REGISTRY.yaml) 是现有围栏的唯一机器可读清单。每条至少有：`id`、`layer`、`objective`、`enforcement`、`schedule`、`owner`、`status`、`evidence_level`、`evidence`、`last_verified`、`positive_test`、`negative_test`、`exception`、`review_due`。

**状态**：`proposed → approved → pilot-warning → enforced → monitored → revised | retired`。`NOT_VERIFIED` 是证据等级，不是部署状态。

**谁能改状态**：

- proposed → pilot-warning：在批准范围内，agent 可以提 PR；
- 升为 `enforced`、降低检查强度、停用高风险规则：须用户批准，并有独立核验证据；**agent 不得给自己签署 `enforced`**；
- 登记表同一时间只由一个治理 PR 修改；多个 agent 并行审查时各出报告，再由一个 PR 汇总。

**新增或修改围栏前要回答的十个问题**：

1. 防止哪一种具体错误？
2. 证据来自哪个提交、事故、测试或业务不变量？
3. 严重性、发生概率、可检测性？
4. 现有围栏为什么没挡住（没执行、规则错、权限缺口，还是确实缺）？
5. 修现有检查还是新增？优先修现有的。
6. 放在执行前、PR、每日、每周、里程碑哪个节点？
7. 需要的是确定性测试、AI 审查、权限、告警还是恢复手段？
8. 怎样用正例和反例证明它可靠？
9. 对 PR 时长、误报、维护成本有什么影响？
10. 谁批准启用、谁维护、什么情况下退役？

**闭环**：证据 → 缺口 → 提案 → 成本与收益 → 授权 → 试点（只警告）→ 独立核验 → 强制 → 监控 → 修订或退役。每次最多推进 1～3 条。不要“每个 bug 都加一条永久检查”。

## 6. 可信边界（本仓库现实）

- 仓库内的文件（含 `CODEOWNERS`、工作流、检查脚本）都可能被有写权限的会话修改，必须由分支保护或 Rulesets、受限身份、必需检查来保护。
- **当前限制**：`CODEOWNERS` 只有 `@Grant-Huang`；agent 会话以同一账号提 PR，GitHub 不允许作者批准自己的 PR。在开发 agent 换成独立身份（[BOT_IDENTITY.md](BOT_IDENTITY.md) 阶段 2）之前，第六层“禁止自批”最多是 IMPLEMENTED，不是 ENFORCED。
- 分支保护与 Rulesets 的实际设置，云会话无法读取，状态为 NOT_VERIFIED；核对方法见 [BRANCH_PROTECTION.md](BRANCH_PROTECTION.md)。
- 定时检查不设为 PR 的必需检查：它们不在 PR 的提交上运行，强制等待会造成无意义的阻塞；PR 的必需检查只由针对该提交运行的工作流产生。
- 只读审计不应持有可改主分支的凭据；工作流用最小 `permissions`，第三方 action 固定到 commit SHA。

## 7. 职责（按真实的人和 agent）

| 决策或交付 | 用户（项目负责人） | 实现会话 | 评审 / 修复 agent |
|---|---|---|---|
| 需求意图、验收标准、授权 | 决定 | 起草、提问 | 核对 |
| 共享契约、设计边界 | 批准 | 起草方案 | 指出偏离 |
| 测试与证据 | 抽查 | 编写并如实报告 | 独立核验、复现 |
| CI、权限、分支保护 | 决定并在 GitHub 操作 | 出方案（L3） | 指出漂移 |
| 围栏升为强制、例外、退役 | 批准 | 提案 | 提供误报和漏报证据 |
| 版本发布与恢复 | 决定、打 tag | 准备版本说明 | 核对准出证据 |

开发者不能批准需要独立审批的改动；岗位增加后，在本表和 `CODEOWNERS` 里补上真实的人。

## 8. 本目录的其他文件

| 文件 | 内容 |
|---|---|
| [GUARDRAIL_REGISTRY.yaml](GUARDRAIL_REGISTRY.yaml) | 围栏登记表 |
| [GAP_ANALYSIS.md](GAP_ANALYSIS.md) | 缺口与路线图 |
| [PREFLIGHT.md](PREFLIGHT.md) | 提交 / 开 PR 前自检（命令与 PR 正文） |
| [BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) | 分支保护（阶段 0） |
| [BOT_IDENTITY.md](BOT_IDENTITY.md) | 开发 agent 独立身份（阶段 2） |
| [SHADOW_REVIEW.md](SHADOW_REVIEW.md) | 合并前影子审查 |
| [EVAL_LOOP.md](EVAL_LOOP.md) | 缺陷、事故 → 测试与评测闭环 |
| [PRODUCT_FENCES.md](PRODUCT_FENCES.md) | 工程围栏 → 产品能力映射 |
| [REVIEW_VERDICT_DESIGN.md](REVIEW_VERDICT_DESIGN.md) | 审查进“待人工”后的通过 / 退回设计 |
| [WP2_IMPLEMENTATION_PLAN.md](WP2_IMPLEMENTATION_PLAN.md) | WP2 / GS3：人工验收出口实施方按（已批准并实现 #215） |
| [WP4_IMPLEMENTATION_PLAN.md](WP4_IMPLEMENTATION_PLAN.md) | WP4 / GS4：任务拆分与模板执行实施方按（#216 已批；实现跟进） |

**指导不是安全边界**：`AGENTS.md` 里的禁止事项必须靠 CI、分支保护、权限和运行时强制。
