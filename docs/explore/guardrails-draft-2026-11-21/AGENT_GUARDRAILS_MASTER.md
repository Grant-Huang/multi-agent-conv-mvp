# AgentNexus Agent 自主开发七层围栏：审计、建设与持续治理规范

**版本**：v1.0（提案）  
**日期**：2026-11-21  
**适用**：Claude Code、Codex、其他 Coding Agent、开发/QA/架构/平台团队  
**状态**：待项目负责人批准；**不是**仓库已部署状态报告  
**输入依据**：`DEV_STANDARD.md` v0.1、2026-10-30《自主开发中Agent围栏建设建议以及团队思维模式转型》及同内容的《AI内服提升中Agent围栏建设建议以及团队思维模式转型》、前次 v0.2 修订提案。  
**外部实践参考**：GitHub Rulesets/Required Status Checks；Claude Code 项目指令、Hooks 与 Subagents；Codex `AGENTS.md` 与 Skills。具体功能和权限以实际安装版本/仓库配置核验为准。

> **总原则**：让 Agent 在有效授权范围内连续完成任务；高后果错误在动作之前被技术措施阻断；普通 PR 仅承担与本次变更有关的验证；每日处理增量质量；每周治理结构性风险；里程碑判断端到端产品质量与发布能力。任何 AI 文本承诺均不能代替可信 CI、GitHub 权限或生产环境控制。

## 1. 目标、边界与治理对象

### 1.1 六环与七层的关系

现有 `DEV_STANDARD.md` 的六环（**需求 → 设计 → 计划 → 实现 → 质量 → 部署**）是**工作对象及追踪链**，不是七层围栏；两者不能强行一一对应。

- **六环**保存需求意图、设计选择、实施与测试证据、版本归属；必要时引用 `docs/PRODUCT.md`、`docs/DESIGN.md`、`docs/contracts/`、`docs/features/<id>/README.md`、`docs/testing/features.json`、`docs/decisions/`。
- **七层围栏**规定谁/什么可以做、何时检查、如何阻断、如何证明、失败如何恢复。
- **四种检查节奏**规定检查何时进行：PR、Daily、Weekly、Milestone；此外有不可延期的**执行前/发布前保护**。
- **跨层治理**包括 Agent 身份与最小权限、可追踪证据、Eval/事故回灌、自主开发授权契约。

### 1.2 四个可靠性断言

1. **有文档 ≠ 已执行**：声明了规则或存在脚本，不等于它被受保护的检查持续执行。
2. **有测试 ≠ 业务正确**：编号/文件存在性、测试执行、业务语义正确性必须分级证明。
3. **独立 Agent ≠ 独立执行权**：另开上下文可减少自我合理化，但防止绕过要靠独立权限和受保护工作流。
4. **周期检查 ≠ 事前保护**：权限、敏感数据、不可逆操作等不能放到每日/每周才发现。

## 2. 七层围栏的正式定义与分工

| 层 | 核心责任 | 主要控制与证据 | 负责角色 | 失败处置 |
|---|---|---|---|---|
| **L1 需求边界与验收** | 授权、非目标、业务规则、可验证完成标准 | 特性/REQ ID；验收场景；授权范围；例外记录 | 产品负责人；需求分析；开发 Agent | 未授权行为不得实施；缺少低风险细节可记录假设并按授权契约处理 |
| **L2 产品知识与架构契约** | 避免架构绕行、接口/状态机语义分裂 | 产品宪章、Feature Map、架构图、API/状态契约、ADR、契约测试 | 架构负责人 + 模块负责人 | 契约破坏/跨模块高风险先批准；普通漂移纳入周期治理 |
| **L3 开发执行与隔离** | 防止 Agent 越界修改、泄密、误用高危工具 | 工作树隔离、路径约束、身份权限、凭据隔离、命令与预算限制 | 工程平台/安全负责人 | 越权即时停止；记录审计；不可仅靠提示词限制 |
| **L4 确定性自动验证** | 可重现、可机器判定的基础质量 | 受影响测试、lint、typecheck、build、契约/集成测试、扫描、退出码 | 开发 + QA + CI 维护人 | 关键门禁失败不可合并；非关键趋势进入定期任务 |
| **L5 独立 AI Review** | 分析语义缺陷、缺失场景、回归与审查误报 | 隔离上下文审查、发现项定位、复现、Finding Verifier、评测样本 | 独立 Reviewer Agent；QA 监督 | 高置信严重问题阻断并验证；不确定问题记录待复核 |
| **L6 风险分级与授权审批** | 正确分配人机决策权，防止自我批准 | R0–R3 风险等级、CODEOWNERS、Rulesets、required reviews/status checks | 模块 Owner、技术/安全负责人、发布负责人 | 不符合审批条件不能合并/部署；例外必须有时效与责任人 |
| **L7 发布、监控与恢复** | 确保真实运行可观测、版本可追溯、故障可恢复 | release manifest、健康检查、灰度、回滚/前滚、事故记录、演练 | SRE/运维 + 发布负责人 | 发布前失败阻断；发布后按预案停止/恢复并复盘 |

**七层之外的横向机制**：①身份和最小权限；②证据与审计日志；③缺陷/事故/Eval 回灌；④自主开发授权契约（允许任务、禁止动作、成本/时间/次数预算、升级条件）；⑤围栏有效性与退役治理。

## 3. 检查职责矩阵（核心执行表）

**符号**：**必**=该节奏必须运行本层的相应范围；**触**=由变更内容/风险触发；**抽**=周期抽检或覆盖评估；**—**=非常规；**前置**=不能推迟至定期。

| 围栏 | 每个 PR（Diff 范围） | 每日（默认分支增量） | 每周（项目级治理） | 关键里程碑（全局准出） |
|---|---|---|---|---|
| **L1 需求** | **必**：关联授权/REQ 或说明不适用；行为变化验收 **触** | 新增无归属行为与未闭合 REQ **触** | 需求→实现→测试链抽检 **必** | 里程碑范围、非目标、关键业务验收 **必** |
| **L2 架构知识** | 受影响接口/状态/契约检查 **触** | 新增设计漂移线索 **抽** | 跨模块依赖、双重语义、过时 Feature Map **必** | 关键不变量、重要接口兼容、迁移设计 **必** |
| **L3 执行隔离** | **前置必**：沙箱、凭据/修改范围、敏感路径控制 | 身份/CI/权限配置漂移 **必** | 最小权限复核、工具能力与预算审计 **必** | 生产角色、部署身份、访问控制验证 **必** |
| **L4 确定性质量** | **必**：相关 lint、测试和安全门禁；受影响集成测试 **触** | 合并集合回归、flaky 与构建健康 **必** | 性能趋势、覆盖盲区、故障注入轮换 **必** | 端到端业务、异常/恢复、性能关键条件 **必** |
| **L5 独立 Review** | R0 可省；R1 轻量；R2/R3 深度 **触** | 高风险合并审查补充、发现复核 **必** | 业务语义/架构深审、误报漏报复盘 **必** | 独立里程碑质量评估 **必** |
| **L6 风险审批** | **必**：风险级别、必需审批、禁止自批 | 审批例外/绕过记录 **必** | 风险分类阈值、例外模式和误判 **必** | 剩余风险、发布授权与责任签署 **必** |
| **L7 发布运行** | 仅部署/迁移相关变更强检 **触** | 已部署环境健康/SLO/异常 **必**（有环境时） | 事故复盘、恢复能力、可观测缺口 **必** | 灰度策略、回滚/前滚演练、发布准备 **必** |

### 3.1 每个节奏的承诺与限制

**PR**（目标：不把新风险带入主干）：仅检查本次 Diff 与其可信影响范围；运行阻断级安全/数据/权限门禁；输出风险分级、测试结果、审查发现及关联 ID。**不能**假定“没改某模块”就完全没影响；依赖关系不明时扩大检查范围。

**Daily**（目标：及时发现累计风险）：以最后一次成功 Daily 的基线 SHA 到当前默认分支为窗口，检查合并集合交互、失败和 flaky、权限/配置漂移、高风险遗留问题及生产健康。缺一次执行要标记 missed run 并补查窗口，不得仅按最近 24 小时跳过数据。

**Weekly**（目标：系统治理）：整体架构/Feature Map 漂移、重复根因、跨模块事务、测试覆盖盲区、围栏执行成本/误报、技术债；提出 1–3 项有证据的围栏修补，并提出可以删减的重复检查。

**Milestone**（目标：确认可交付）：按批准范围运行端到端场景、状态机/权限/数据不变量、故障注入、迁移与恢复演练，给出 `PASS / CONDITIONAL / BLOCKED`。`CONDITIONAL` 仅限可接受、受控且明确签署的非关键剩余风险；安全或不可逆关键问题不可条件放行。

### 3.2 即时升级规则

在 PR、Daily、Weekly 任一节点发现已证实的高后果问题（越权、敏感数据泄露、不可恢复损坏、严重安全漏洞），必须即时创建阻断/发布冻结事件，指定负责人、截止时间和恢复标准。**不能以“周检时再说”代替当前处置**。

## 4. PR 风险分级：把负担从每次全检改为可信的差异检查

> 使用 **R0–R3** 表示变更风险；原 `risk_classify.py` 如采用 L0–L3，需建立显式映射，避免与七层 `L1–L7` 混淆。分类应以影响为主、文件路径为初始线索；无法判定时升级或人工校准，不能默认 R0。

| 等级 | 常见场景（不是排他定义） | 合并前必需门禁 | 可下沉至 Daily/Weekly |
|---|---|---|---|
| **R0 无行为变更** | 格式、注释、无语义文档修复 | 校验确属无行为变更；文档/格式检查；敏感文件特殊规则 | 全项目文档导航抽检 |
| **R1 局部低风险** | 局部业务修复、隔离 UI 行为 | 相关测试、相关静态/安全检查、轻量独立 Review（可批处理，但合并前关键不变量仍通过） | 累积漂移、跨 PR 回归 |
| **R2 跨模块/中高风险** | 公共 API、状态转换、跨模块事件、共享数据模型 | 变更影响分析、契约/集成测试、独立 Review、CODEOWNER 审批 | 项目级趋势、广域性能 |
| **R3 高后果** | 认证/租户隔离、生产数据、数据库破坏性迁移、发布权限、核心调度/执行安全 | 显式授权、专项验证、可靠审批、恢复/演练证据、独立受信 CI | 事后复盘，**不能**取代合并前保护 |

**不随风险分级降低的底线**：凭据保护、权限限制、必要的 secret scanning、恶意依赖/供应链防护、高风险文件保护和可验证执行证据。PR 行数只用于审查复杂度提醒，不能作为唯一风险指标。

**检查例外管理**：例外应写明 `issue/owner/reason/expires_at/mitigation/approver`，不能以 `continue-on-error`、禁用测试或编辑断言掩盖失败；历史遗留问题与本次引入问题分别报告，已证实严重风险仍可能阻断发布。

## 5. 六环追踪规范的必要修订

保留原始文件层级：L0 产品宪章、L1 架构/共享契约、L2 特性档案、L3 计划，以及决策、质量、发布和登记表。**本段文件 L0/L1/L2/L3 与围栏层号不是同一体系；建议在文档中重命名为 D0–D3，以消除歧义。**

1. **授权**：将“每个 REQ 均需用户确认”改为“每项新行为拥有有效授权来源”。在批准的 Feature/Milestone/Autonomy Contract 内可自主分解子任务；扩大范围、改变共享契约、越权和高风险行为仍须审批。
2. **TDD**：Bug 修复优先 Red→Green；新增行为先有可执行验收；纯重构 Green→Green；基础设施做对应配置/故障验证。不强制每个改动制造 Red。
3. **PR 同步**：同一 PR 更新本次受影响的需求、设计、实现入口、测试和索引；**发布**阶段才更新 tag、release notes、`since` 与真实部署记录。无变化环节无须机械更新。
4. **决策记录**：批准的 ADR 不得静默篡改；通过新 ADR 明确 supersede，并保留历史。
5. **检查证据**：`trace_check.py` 的引用存在性仅为结构等级；CI 运行证据是执行等级；独立 Reviewer/业务验收为语义等级。
6. **项目默认分支**：脚本由 `git symbolic-ref refs/remotes/origin/HEAD`、GitHub API 或现有配置发现，不能固定 `origin/master`；本地检查允许不联网的模式。

## 6. GitHub 仓库围栏结构：最小必需与可扩展形态

> **这是目标结构，不是现有仓库事实**。初始化时首先发现同类文件并复用，避免生成重复目录。`DEV_STANDARD.md` 保持唯一规范主文件；Agent 指令只引用，不复制全文。

```text
<repo>/
├── DEV_STANDARD.md                   # 唯一研发/围栏强制规则来源（版本化）
├── AGENTS.md                         # Codex/通用 Agent 入口：简短索引
├── CLAUDE.md                         # Claude Code 入口：简短索引，不重复规则
├── docs/
│   ├── PRODUCT.md                    # 现有产品宪章
│   ├── DESIGN.md                     # 现有架构索引
│   ├── features/<feature>/README.md # 现有特性档案
│   ├── contracts/                   # 现有共享契约
│   ├── decisions/                   # ADR
│   ├── testing/
│   │   ├── CASES.md
│   │   ├── features.json             # 需求/用例/源码/版本映射
│   │   └── evidence/                # 小体量、可审计的证据索引
│   ├── releases/                    # 真实发布记录
│   └── fencing/
│       ├── README.md                # 治理入口与责任说明
│       ├── GUARDRAIL_REGISTRY.yaml  # 当前生效围栏与验证状态 [机器可读]
│       ├── AUTONOMY_CONTRACT.md      # 任务范围、预算、升级条件模板
│       ├── RISK_POLICY.md           # R0-R3 分类及审批映射
│       ├── REVIEW_POLICY.md         # 独立 Reviewer 与 Finding Verifier
│       ├── GAP_ANALYSIS.md          # 最新可核实的缺口
│       ├── ROADMAP.md               # 待建设/修复围栏的优先队列
│       ├── EFFECTIVENESS.md         # 误报、逃逸缺陷、成本与收益
│       ├── EVAL_LOOP.md             # 事故/缺陷→回归/Eval 机制
│       ├── CHANGELOG.md             # 围栏版本、例外、退役
│       └── reports/                 # 汇总报告索引；详细报告可放 CI Artifacts
├── .github/
│   ├── CODEOWNERS                    # Owner 绑定；效力依赖 Rulesets
│   ├── pull_request_template.md
│   └── workflows/
│       ├── ci.yml                    # 优先复用现有主 CI，不盲目重建
│       ├── daily-health.yml          # 定时 + workflow_dispatch
│       ├── weekly-governance.yml     # 定时 + workflow_dispatch
│       └── milestone-gate.yml       # 显式人工/发布触发
├── .claude/agents/                   # 可选：只读独立 Reviewer
├── .agents/skills/                   # 可选：Codex 治理 Skill
├── tools/ci/                        # 现有脚本优先复用
│   ├── risk_classify.py
│   ├── trace_check.py
│   ├── doc_lint.py
│   ├── drift_check.py
│   └── tests/                       # 围栏自身的正反例测试
└── scripts/
    └── preflight.sh                 # 轻量本地前置自检
```

**可信边界**：`CODEOWNERS`、工作流文件、检查脚本都可能被有写权限的 Agent 修改；必须用受保护的 Rulesets、受限身份、必需检查来源和环境审批来保护这些机制。任何仓库内文件不能单独防止拥有仓库管理员权限的行为者修改规则。

## 7. 围栏登记表：持续维护的唯一机器可读事实源

每条围栏至少有：`id/layer/objective/risk_trigger/enforcement/schedule/owner/status/evidence/last_verified/positive_test/negative_test/exception_policy/review_due`。建议状态：`proposed → approved → pilot-warning → enforced → monitored → revised|retired`；`NOT_VERIFIED` 是证据状态而不是部署成功状态。

示例（仅用于说明结构，不代表已配置）：

```yaml
schema_version: 1
repo: AgentNexus
last_audit:
  baseline_sha: "TO_BE_DETECTED"
  checked_at: null
  coverage: []
guardrails:
  - id: GR-L4-TRACE-001
    layer: L4
    objective: "检测新增需求与测试引用断链"
    risk_trigger: ["requirements_changed", "feature_index_changed"]
    enforcement: "CI warning in pilot; required check after approval"
    schedule: ["pr", "weekly"]
    owner: "QA lead"
    status: proposed
    evidence: []
    last_verified: null
    positive_test: "正常 REQ + 存在测试引用通过"
    negative_test: "不存在的用例引用必须报错"
    exception_policy: "记录责任人、原因和到期日"
    review_due: null
```

**围栏状态变化规则**：从 proposed 到 pilot 可由批准范围内的 Agent 提交 PR；变为 `enforced` 需独立校验和有权批准者；降低检查级别、停用高风险规则只能经独立批准。AI 不得给自己签署 `enforced` 证明。

## 8. 围栏增量建设与生命周期管理

### 8.1 触发条件

- 历史缺陷重复；严重事故或 near miss；架构/业务状态语义分裂；已有检查误报频繁；新能力引入新风险；部署环境/权限发生变化；里程碑审查失败。
- 不建议“每个 Bug 都新增一个永久检查”。先判断现有检查未执行、测试不准、权限失效，还是确实缺失新围栏。

### 8.2 候选围栏必须回答的十个问题

1. 试图防止哪一种**具体错误**？
2. 证据来自哪个提交/事故/测试/业务不变量？
3. 错误的严重性、发生概率和可检测性是什么？
4. 现有围栏为何无法覆盖？
5. 是修复既有检查还是新增控制更合适？
6. 应放在执行前、PR、Daily、Weekly、Milestone 哪个节点？
7. 需要的是确定性测试、AI Review、权限、告警还是运行恢复？
8. 如何用**正常样例 + 违反样例**证明可靠？
9. 对 PR 时长、误报、维护成本有何影响？
10. 谁可批准启用、谁维护、什么情况下退役？

### 8.3 建设闭环

**Evidence → Gap → Proposal → Cost/Benefit → Authorization → Pilot → Independent Verification → Enforcement → Monitoring → Revise/Retire**。

每次最多推进少量（建议 1–3 条）高价值改进：先本地或非阻断试点，检查正确拦截及正常放行；之后经批准纳入 Required Checks。若存在严重漏洞，立即建立临时阻断控制，同时补正式规则。

### 8.4 围栏有效性指标（关注趋势，不伪造数值）

- 逃逸缺陷数及严重性、同类重复根因数。
- 关键错误被围栏成功拦截的数量、漏报与误报率。
- PR 端到端等待时间、必检执行时长、重跑成本。
- `NOT VERIFIED` 的关键要求数量、例外/绕过/到期未关闭情况。
- 批准范围内 Agent 连续自主完成任务比例，人工干预集中度。
- 发布失败/恢复事件与演练结果。

## 9. 审计方法及结论等级

### 9.1 事实收集顺序

1. 查 Git 远端、默认分支、最近有效基线及受保护文件。
2. 查项目技术栈、测试命令、现有 CI/扫描/审查/部署体系。
3. 查需求/Feature Map、架构契约、状态机、跨模块边界。
4. 查 GitHub Rulesets、CODEOWNERS、Required Checks、Secrets 可访问范围（**只检查元数据，不能读取密钥值**）。
5. 查生产/预发布环境权限、回滚机制、事件与事故记录（如无权限明确标 `NOT VERIFIED`）。
6. 将全部证据映射到围栏登记表并形成 GAP_ANALYSIS。

### 9.2 四种证据等级

- `DECLARED`：文本声称存在。
- `IMPLEMENTED`：配置/脚本/测试存在。
- `VERIFIED`：有真实执行记录和正反例测试。
- `ENFORCED`：在可信权限边界下无法正常绕过（仍记录覆盖范围与例外）。

另外单独使用 `NOT_VERIFIED` 表示未能核实。不能把 `IMPLEMENTED` 当成 `ENFORCED`。

### 9.3 三层测试追踪

- T1 **结构追踪**：REQ/用例/档案/版本引用真实存在且格式正确。
- T2 **运行追踪**：相关测试实际执行，日志/退出码和失败/跳过数量可核验。
- T3 **语义追踪**：独立 Reviewer/验收证据证明断言对应真实业务规则。

`DEV_STANDARD.md` v0.1 中 `trace_check.py` 主要覆盖 T1；`drift_check.py` 只能提供漂移线索，不能代替语义验证。

## 10. 周期审计的输入、输出、故障语义

| MODE | 输入基线 | 推荐结果 | 失败与通知 |
|---|---|---|---|
| `bootstrap` | 当前默认分支及完整仓库事实 | 七层现状矩阵、前三个缺口、ROADMAP、登记表初稿 | 不修改权限/主干；标记未知 |
| `pr` | PR base SHA → head SHA；依赖影响图 | 风险评级、受影响测试、审查 Findings、CI 状态 | 必检失败阻断；历史问题独立登记 |
| `daily` | 上次成功运行 SHA → 默认分支 HEAD | 新增问题、flaky、跨 PR 风险、未清严重项 | 失败可重跑；必要时升级重大事件 |
| `weekly` | 近一周 + 可追溯历史基线 | 架构/需求/测试漂移、围栏效益、1–3 条治理提案 | 报告后自动建 issue/受控草稿 PR（如已授权） |
| `milestone` | 已批准的目标、候选 tag/SHA、发布环境 | PASS/CONDITIONAL/BLOCKED、剩余风险与签署 | 关键失败阻断 release；不可伪造签署 |

报告通用字段：`run_id/mode/repo/default_branch/base_sha/head_sha/check_started_at/check_completed_at/scopes/tools_versions/checks/results/findings/unknowns/owner/next_actions`。不应在报告内存储 Secrets、生产客户数据或不必要个人信息。

## 11. 工作流可靠性与安全底线

- GitHub scheduled workflow 通常从**默认分支**定义运行，cron 以 **UTC** 解释，可能延迟或在高负载时错过；必须支持 `workflow_dispatch` 手动补跑，记录成功基线。
- 不要让只读审计工作流直接持有能修改主分支的凭据；用最小 `permissions`，产生 Artifact/Issue 而非默认自动 commit。
- 对 `pull_request_target` 等具有更高权限的触发器保持谨慎；不得在其高权限上下文执行不可信 PR 代码。
- 把 `CODEOWNERS` 与分支 Rulesets 的有效性视为**平台配置检查**，而不是仅检查文件是否存在。
- 安全与验证脚本的变更须由独立受信来源运行；对关键 Required Checks 约束预期提交状态的 App 来源（平台支持且适用时）。
- Hook 是辅助即时提醒/局部限制；真正部署/仓库权限需 GitHub 和执行环境 enforce。
- 规则变更要记录版本、理由、启用时点、兼容性、回滚及审批。老会话可能未重新读取文件，变更后应从新会话或检查点重新加载。

## 12. 管理责任与 RACI 简表

| 决策/交付 | Product/BA | Dev/架构 | QA | Platform/Security | 发布/Owner |
|---|---|---|---|---|---|
| 需求意图、验收标准 | **A/R** | C | C | I | I |
| 共享契约/设计边界 | C | **A/R** | C | C | I |
| 测试基线及证据质量 | C | R | **A/R** | C | I |
| CI、沙箱、权限保护 | I | C | C | **A/R** | C |
| AI Review 执行/误报复核 | C | C | **A/R** | C | I |
| 风险升级与重大例外 | C | R | R | R | **A** |
| 版本发布、恢复策略 | I | R | C | R | **A** |
| 围栏有效性/退役 | C | R | R | R | **A** |

R=执行负责；A=最终负责；C=参与咨询；I=知悉。可按团队规模合并岗位，但开发者不能自行批准需要独立审批的变更。

## 13. 里程碑准出模板

```text
Milestone: <名称>
Scope / excluded scope: <范围/非目标>
Candidate SHA / version: <SHA/tag>
Business acceptance: PASS | FAIL | NOT_VERIFIED
Architecture & contracts: PASS | FAIL | NOT_VERIFIED
Security & permissions: PASS | FAIL | NOT_VERIFIED
CI / regression / E2E: PASS | FAIL | NOT_VERIFIED
Deployment & observability: PASS | FAIL | NOT_VERIFIED
Recovery rehearsal: PASS | FAIL | NOT_VERIFIED
Critical open findings: <列举>
Noncritical residual risks: <责任人、缓解措施、到期日>
Decision: PASS | CONDITIONAL | BLOCKED
Approver / time / evidence links: <必须真实>
```

## 14. 实施顺序及验收

**M0：审计基线（只读）**：确认仓库事实、默认分支、CI、平台权限；交付矩阵、前三个价值最高的缺口，不改工作流和保护规则。

**M1：最小围栏试点**：整理 `AGENTS.md/CLAUDE.md`、建立登记表，选一项低风险高价值检查做正/反例测试并在非阻断 CI 试点；演示 Agent 能正常完成受控任务。

**M2：PR 分级 + Daily/Weekly**：仅对受影响范围施加快检，Daily 扩展回归，Weekly 架构/语义治理；观察误报及成本后决定提升强制级别。

**M3：高风险前置与可信保护**：在有权限的负责人批准后启用必要的 GitHub Required Checks、CODEOWNERS/Rulesets、隔离身份和受保护发布；检查不能自我批准/绕过。

**M4：里程碑治理闭环**：业务端到端/关键状态不变量/发布与恢复演练；正式引入逃逸缺陷与 Eval 反馈，周期性调整和退役围栏。

### 最终验收

不得声称“七层全部生效”，除非每层均记录：目标、真实控制、可复现实验、权限边界、执行负责人、最近验证证据、失败处置和下一次复核。未验证则显式报告。

---

## 附录 A：与原 `DEV_STANDARD.md` v0.1 的差异摘要

- 保留产品宪章、共享契约、特性档案、REQ ID、`features.json`、决策记录、六环追踪。
- 独立 AI Review 恢复为专门一层，避免并入泛化 PR 门禁或审批。
- 六环改为“在相应准出点同步受影响信息”，不强制每个开发 PR 含发布信息。
- Red→Green 改为根据任务类别适用；风险等级不以文件名/行数单独判定。
- 规范冲突改为明示、按授权裁决，不采用模糊的“更严格者永远优先”。
- `preflight.sh` 不应固定 `origin/master`；`trace_check.py`、`drift_check.py` 的结果不能夸大为语义验证。
- 围栏的建设、复核、退役成为持续工作，不是一次性创建文件树。

## 附录 B：来源与验证提示

- 本项目上传的三份源 Markdown 是**规范与设计材料**，不是仓库生效证据。
- 官方平台参考（使用前以当前版本复核）：
  - GitHub Rulesets: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets
  - GitHub Actions schedule: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule
  - Claude Code steering: https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
  - Codex AGENTS.md: https://learn.chatgpt.com/docs/agent-configuration/agents-md

