# AgentNexus 七层围栏：Claude Code / Codex 实操指南

**版本**：v1.0（试点操作指南，2026-11-21）  
**配套主文档**：`AGENT_GUARDRAILS_MASTER.md`  
**适用范围**：已有 GitHub 仓库；需要独立权限的部分必须由仓库 Owner/管理员执行或批准。

> **重要**：以下是可复制的参考步骤与配置骨架，不表示当前 AgentNexus 仓库已有这些文件或 GitHub 权限。先审计，后按实际技术栈适配；特别不要原样覆盖 `.github/workflows/ci.yml`、`CODEOWNERS` 或已有 `AGENTS.md`。

## 0. 一张图理解操作流程

```text
仓库只读发现（bootstrap）
    ↓ 现有能力矩阵 + 三个最重要缺口
规范/登记表初始化（治理 PR；不修改主干）
    ↓ 选一条可验证的高价值围栏
小范围试点（warning + 正反例测试）
    ↓ 独立 Reviewer + owner 验收
正式门禁（Required Checks / Rulesets / 最小权限）
    ↓
PR 快检 ──┬── 每日增量（回归、变更交叉效应）
           ├── 每周治理（架构、围栏有效性）
           └── 里程碑（端到端、恢复与授权）
    ↓
漏报/误报/事故 → 修复/新增/合并/退役围栏
```

## 1. 操作准备

**访问权限**：仓库读权限可做 `bootstrap`；创建分支/PR 需要写权限；配置 Rulesets/Secrets/部署环境通常需要管理员或明确授权。独立 Reviewer 的权限应尽可能只读。

**必备信息**：仓库 URL、默认分支、CI 入口、工程技术栈、构建/测试命令、产品宪章/Feature Map、核心共享契约、已有审批者、发布环境。缺失项先记 `NOT_VERIFIED`。

**本地预检**（在仓库根目录）：

```bash
git status --short
git remote -v
git branch --show-current
git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || true
find .github/workflows -maxdepth 1 -type f 2>/dev/null | sort
find tools/ci -maxdepth 2 -type f 2>/dev/null | sort
```

查看当前状态不要求 `git fetch`，尤其只读审计不应有隐式网络或变更操作。若默认分支元信息不存在，从 GitHub UI/API 或已知仓库配置核实。

## 2. 先把主文档放到仓库，但不要立即视为强制规则

推荐：创建 `docs/fencing/`，把主文档以 `docs/fencing/GOVERNANCE_PROPOSAL.md` 形式提交**提案 PR**；批准后再把应当强制执行的条款融入 `DEV_STANDARD.md`，避免产生两个同时自称权威的规范。

文件与责任分配：

| 文件 | 谁更新 | 什么情况下更新 | Agent 的权限 |
|---|---|---|---|
| `DEV_STANDARD.md` | 规范 Owner + 独立审批 | 规则原则变化 | 只能提案/PR，不自动修改受保护主干 |
| `docs/fencing/GUARDRAIL_REGISTRY.yaml` | 平台/QA Owner；Agent 辅助 | 围栏新增、验证、改版、退役 | 可在受控 PR 修改；状态晋级需证据 |
| `docs/fencing/GAP_ANALYSIS.md` | 审计 Agent | bootstrap 和周检 | 可以报告事实；不能编造平台状态 |
| `docs/fencing/ROADMAP.md` | 治理 Owner + Agent | 周度评审 | 可提出/更新候选项 |
| `docs/fencing/EFFECTIVENESS.md` | QA + 平台 | 每周/月 | 可计算指标，不可填虚假数据 |
| `docs/fencing/CHANGELOG.md` | 治理 Owner | 每次正式规则变更 | 只能记录真实已批准变更 |
| `AGENTS.md` / `CLAUDE.md` | 开发 Owner | 项目入口有实质变化 | 保持短，链接规范 |

**规范版本**：不要为“每天检查结果变化”递增 `DEV_STANDARD.md` 版本；它是治理原则而非每日审计日志。

## 3. 给 Claude Code / Codex 的首次审计指令

下面可以直接复制到两个工具的会话中，建议在仓库根目录开启**独立只读或 plan 会话**：

```text
你是本仓库 Agent 自主开发围栏审计员。先读 DEV_STANDARD.md、
AGENTS.md、CLAUDE.md（若存在）和 docs/fencing/AGENT_GUARDRAILS_MASTER.md。
MODE=bootstrap。不要修改代码、配置、CI、GitHub Rulesets、Secrets 或主干。

必须先发现真实默认分支、技术栈、CI 作业、测试入口、产品宪章、
架构/契约、Agent 指令和已有的权限保护配置。

按七层输出矩阵：目标、现有配置、执行证据、可信强制程度、
未验证事项、责任人、风险、建议动作。

每条事实标示 DECLARED/IMPLEMENTED/VERIFIED/ENFORCED/NOT_VERIFIED。
不能通过文件存在就声称 GitHub 分支保护、Review 或发布门禁生效。

只提出价值最高的三个增量围栏，每个必须包含：目标错误、复现证据、
现有机制为何没拦住、在哪个检查节奏执行、正反例测试、预期成本、
实施/回滚方案、审批边界。

输出 GAP_ANALYSIS 和 ROADMAP 的建议内容；不直接大范围改写文档。
```

**验收**：审计结果必须给出真实 SHA、CI 名称、对应配置文件和无法看到的 GitHub/生产权限项；出现“全绿”但无执行证据时退回重做。

## 4. 统一 Agent 入口（不要复制整份规范）

### 4.1 `AGENTS.md` 建议片段

```markdown
# AgentNexus: Project Agent Instructions

Before changing code, read DEV_STANDARD.md and the relevant feature dossier,
contracts, and current guardrail registry. Follow the currently approved rules.

- Work only within the authorized feature/scope; do not modify production data,
  repository protections, secrets, or mandatory CI without explicit approval.
- Determine change risk R0–R3 and run the appropriate evidence-based checks.
- Do not weaken tests or mark unrun checks as passed.
- Record actual commands, exit codes, tests, skipped checks, and open risks.
- For independent review, use a fresh context and identify reproducible findings.
- Treat docs/fencing/GUARDRAIL_REGISTRY.yaml as current guardrail inventory,
  not proof that those controls are enforced.

Read: docs/fencing/README.md; docs/fencing/RISK_POLICY.md;
docs/fencing/REVIEW_POLICY.md.
```

### 4.2 `CLAUDE.md` 建议片段

```markdown
# Claude Code entrypoint

The shared project policy is in DEV_STANDARD.md and AGENTS.md.
Read the relevant feature, contract, tests and docs/fencing/ registry before edits.
Do not reproduce the policy here. For review, use a fresh restricted reviewer context.
```

**注意**：指令入口让工具知道规则，不能确保其遵守。Claude Code Hooks、权限设置和 Reviewer 可协助约束；Codex 的权限/沙箱配置需按当前产品环境确认。CI 保护与生产授权仍需要独立实施。

## 5. 用风险而不是文件数量决定 PR 检查范围

### 5.1 PR 流程

1. 检查变更来源：授权/REQ 或说明无行为变化。
2. 计算 Diff、变更依赖和风险 R0–R3；识别不确定风险。
3. 选测试：R0 格式/文档；R1 相关单元/构建；R2 契约与集成；R3 专项安全/恢复/审批。
4. 独立 Reviewer 按风险审查，发现项记录证据/置信度/是否本次引入。
5. GitHub Rulesets 和 Required Checks 决定是否可以合并。
6. PR 模板呈现测试、跳过项、风险和回滚，不强制提前填写未发生的发布信息。

### 5.2 PR 模板例子（建议合并进现有模板）

```markdown
## 关联
Feature ID: 
REQ / scope authorization: 
Risk: R0 | R1 | R2 | R3 (reason): 

## 变更及影响
Changed behavior / non-goals:
Affected modules / shared contracts:

## 验证证据
Commands + exit codes:
Tests passed/failed/skipped:
Integration / security checks:
Independent review findings:
NOT_VERIFIED / exceptions:

## 发布与回滚（仅部署相关变更填写）
Migration/rollback strategy:
Owner approval:
```

### 5.3 动态测试选择的保护

- 对修改核心权限/状态机/CI 脚本的 PR **始终**运行相关强制专项测试。
- 若 `risk_classify.py` 无法准确映射影响，回退为更广范围的测试而非直接跳过。
- 高风险变更不得以 Daily/Weekly 的后置检查代替合并前检查。
- 不要让 Agent 随意更新测试快照/断言来消除真实失败。

## 6. 先建立可运行的周期工作流：不直接修改仓库的参考 YAML

### 6.1 Daily（报告优先）

> 下面 YAML 是**最小触发器骨架**；调用的脚本是计划接口，必须由 Agent 按真实仓库实现并验证，不能直接复制后认为已完成自检。

```yaml
name: Daily Guardrail Health
on:
  schedule:
    - cron: '30 9 * * *'  # UTC；按团队需要换算本地时区
  workflow_dispatch:
permissions:
  contents: read
jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Daily incremental audit
        run: |
          python3 tools/ci/guardrail_audit.py --mode daily --output audit-report.json
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: daily-guardrail-audit
          path: audit-report.json
          if-no-files-found: warn
```

`guardrail_audit.py` 在原始资料中**尚未实现**，是建议建立的统一 CLI。创建前必须说明数据源、baseline 的持久化方式、无上次成功 SHA 时的回退策略和失败行为。

### 6.2 Weekly（治理审计，不自动修改主干）

```yaml
name: Weekly Guardrail Governance
on:
  schedule:
    - cron: '30 10 * * 1'  # UTC Monday
  workflow_dispatch:
permissions:
  contents: read
jobs:
  governance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Governance review
        run: |
          python3 tools/ci/guardrail_audit.py --mode weekly --output weekly-report.json
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: weekly-guardrail-report
          path: weekly-report.json
          if-no-files-found: warn
```

如果需要 AI 语义审查，应该作为单独受控作业/服务，通过凭据最小化、安全数据筛选和预算限制接入；不要把高权限密钥直接暴露给不可信 PR 代码。自动生成治理 PR 只能在获授权之后单独配置身份和权限。

### 6.3 Milestone（显式触发，不使用无批准的生产发布）

```yaml
name: Milestone Gate
on:
  workflow_dispatch:
    inputs:
      milestone:
        description: 'Approved milestone ID'
        required: true
        type: string
permissions:
  contents: read
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Milestone verification
        env:
          MILESTONE: ${{ inputs.milestone }}
        run: |
          python3 tools/ci/guardrail_audit.py --mode milestone --milestone "$MILESTONE" --output milestone-report.json
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: milestone-guardrail-report
          path: milestone-report.json
          if-no-files-found: warn
```

**适配警告**：当前示例只是编排接口，不包含特性业务验证、性能测试、恢复演练或 GitHub 审批配置。应先在试点分支实现测试并验证工作流 YAML，再逐步关联 Required Checks。

### 6.4 为何不建议把定时审计设成所有 PR 的 Required Check？

因为每日/每周审计不一定在每个 PR 的 SHA 上运行，它们检查的是不同对象；强制所有 PR 等待一个不对应当前提交的定时结果会导致无意义阻塞。PR 的 Required Checks 应由针对该 PR/合并候选 SHA 的事件工作流产生。重大周期发现则单独升级为合并/发布冻结控制。

## 7. 真正启用 GitHub 强制门禁：管理员操作清单

1. GitHub 仓库 **Settings → Rules → Rulesets**（或仓库已有等效保护入口）；识别默认分支与现存规则，不覆盖既有配置。
2. 对默认分支要求 PR、必要的 reviewer/CODEOWNER approval，并禁止不受控的绕过；根据组织权限与仓库套餐确认能力。
3. 将已验证的 **PR 事件工作流**中的必要作业设为 Required Status Checks；核对名称稳定、对应提交 SHA、执行来源可信。
4. 对 `.github/workflows/`、`tools/ci/`、权限策略与主规范设 Owner 审核路径，并测试规则是否真的被平台执行。
5. 发布环境单独配置 approval/protected environment/最小凭据范围，禁止普通代码 Agent 无条件使用生产凭据。
6. 使用一个安全试验 PR 验证：故意使测试失败/修改受保护文件，确认真的无法绕过；然后撤销试验变更。
7. 将验证证据（链接、SHA、测试时间、负责人）加入 registry；不能只填“已完成”。

**提示**：GitHub 的 CODEOWNERS 只有结合审批/分支保护才成为有效门禁；仓库级管理员/可绕过主体的能力仍需审查。必要时使用预期状态检查来源机制，减少伪造状态风险。

## 8. 如何针对项目启发 Agent 自动提出“合适的增量围栏”

每周给 Agent 一个以**错误模式**为中心的任务，而不是“再建 10 条新围栏”：

```text
MODE=weekly。对比本周已合并 PR、失败测试、缺陷、回滚、审查发现、
架构/契约变更、已有围栏误报。

按影响程度输出最多 3 项提案：
1. 具体真实错误或可信薄弱点 + 文件/SHA/issue/测试证据；
2. 为什么现有围栏没挡住（未启用/漏测/错误规则/权限缺口）；
3. 最小修复：优先完善旧围栏，其次增加新围栏；
4. 应放在哪个节奏，为什么不能更早/更晚；
5. 反例如何被拦截、正例如何通过、怎么测误报；
6. 成本、维护者、过期/退役标准；
7. 如需改 CI/权限/安全规则，输出申请，不擅自启用。

只把有证据且未重复的候选写入 ROADMAP；
不要为了提高“围栏数量”添加泛化规则或改写主规范。
```

**AgentNexus 高价值候选检查**（由既有设计主题推导，仍需仓库证据）：Session 生命周期是否规范、任务执行成功是否等于业务持久化成功、ACP/MCP 断连与重试幂等、Agent 取消/超时状态机、真实/Mock 混淆、跨 workspace 权限隔离、Rust 资源释放与并发、独立 Review 误报。这些不是项目已存在的缺陷事实，而是优先审计的候选风险域。

## 9. 独立 AI Review 的实际部署原则

- Developer 与 Reviewer 应是**不同运行上下文**；Reviewer 尽量只读，不能编辑 PR 来让自己的发现“修复”。
- Review 覆盖差异代码、需求验收和相关契约；给出文件/行号/最小复现/证据；判定 introduced / pre-existing / uncertain。
- Finding Verifier 独立复现高严重度报告，避免 AI Review 的无依据断言反复触发修复循环。
- 不以“Reviewer 给出通过”代替测试；重要行为需要确定性测试、手工验证或业务验收。
- 复审范围优先检查新增 Diff 和尚未关闭发现；超过合理轮数升级人类技术判断，而非无限循环。

## 10. 三个周期任务的运营手册

| 时间点 | 自动任务 | 人类只需要看什么 | 触发后续什么 |
|---|---|---|---|
| PR | 变更分类、相关 CI、独立审查（按风险） | R2/R3 设计/高风险授权、争议发现 | 合并/修正/退回 |
| Daily | 增量回归、跨 PR 交互、失败趋势、权限配置变动 | 新出现的高严重度或持续失败 | issue、升级或临时阻断 |
| Weekly | 系统语义/架构漂移、重复根因、围栏性价比 | 1–3 条最值得处理的缺口及退役建议 | 治理 PR、下一周计划 |
| Milestone | 端到端/故障注入、部署和恢复准备 | 是否达到业务准出、剩余风险谁承担 | PASS / CONDITIONAL / BLOCKED |

**推荐会议制度**：每日无需单独召开围栏会议，只让高风险摘要进入工程例会；每周由技术、QA 和平台共同看 15–30 分钟治理提案（时长按团队调整）；里程碑由发布责任人组织正式签署。以上是建议制度，不是现有团队事实。

## 11. 真实的首次试点任务（示范）

以 `REQ → CASES` 追踪检查为例（仅作为低风险候选）：

1. 从实际项目确认现有 `trace_check.py` 的真实路径、字段格式与调用方式。
2. 先写两条单元测试：合法引用通过；无效引用产生预期失败。
3. 发现其默认告警模式下退出码为 0，不能作为阻断证据；将 warning 与 strict 行为写入测试。
4. 在 CI 的单独非阻断作业试点，收集至少若干真实 PR 的误报/漏报证据（不规定虚假的样本数）。
5. 独立 Reviewer 审查是否正确处理现有登记表、档案、测试目录及意外缺失文件。
6. 经 QA/平台 Owner 批准后将适用部分设为 Required Check；不把 T1 结构检查宣称为 T3 语义验证。
7. Registry 中记录实际 PR、CI、验证 SHA、启用日期、责任人和复核日期。
8. 下次周检查看是否减少追踪断链、是否妨碍正常变更，决定优化或退役。

## 12. 文件持续更新 SOP

### 每日（自动，报告为主）

- 保存 CI Artifact（检查范围、baseline SHA、发现、执行结果）；严重缺陷建 issue。
- 不每天改 `DEV_STANDARD.md`，也不自动覆盖 GAP_ANALYSIS 全文。

### 每周（Agent 提议、负责人审阅）

- 合并/更新 `GAP_ANALYSIS.md` 的可核实事实。
- 调整 `ROADMAP.md` 中优先级与 Owner。
- 更新 `EFFECTIVENESS.md` 的真实指标及样本数。
- 若有围栏变更，提交小 PR 更新 `GUARDRAIL_REGISTRY.yaml` 和 `CHANGELOG.md`。

### 里程碑（正式审查）

- 签署准出报告，核对强制围栏状态；明确剩余风险、发布版本及恢复计划。
- 只在政策本身需要改变时升级 `DEV_STANDARD.md` 版本。

**并发控制**：同一时刻尽量只允许一个治理 PR 编辑 registry 主索引；多 Agent 并行审查输出独立 artifact，然后聚合，防止覆盖或冲突。

## 13. 验收清单（第一次上线必须实际验证）

- [ ] 默认分支、关键环境、Owners、GitHub 保护规则均有真实证据或明确 NOT_VERIFIED。
- [ ] 七层每层有当前状态、覆盖范围、例外和下一次检查日期。
- [ ] PR 风险分级能够从 Diff 和语义影响提出合理等级；无法判定时不默认低风险。
- [ ] 至少一个新围栏完成正反例验证，正常 PR 不被无故阻塞。
- [ ] PR Required Checks 由可信配置执行，而非仅开发 Agent 自报。
- [ ] Daily 能在错过计划运行后从上次成功 SHA 补查。
- [ ] Weekly 能识别重复根因，也能提议删除冗余围栏。
- [ ] Milestone 在缺少真实部署证据时输出 NOT_VERIFIED/BLOCKED，不伪造 PASS。
- [ ] 禁止 Agent 自动降低必检强度、修改管理员规则或访问生产凭据。
- [ ] registry、roadmap、evidence 和规范主文件各有唯一维护机制。

## 14. 常见误区与排查

| 现象 | 常见原因 | 处理 |
|---|---|---|
| 文件生成齐全但规则没生效 | 无 Required Checks/Rulesets | 到 GitHub 平台核验并反向验证阻断 |
| 每个 PR 都非常慢 | 将每日/里程碑全量测试压进 PR | 按依赖影响选择检查，保留强制安全底线 |
| Agent 总在等确认 | 把每个子任务都当成独立授权 | 用明确 Autonomy Contract 授权任务包、预算和升级条件 |
| 测试全绿仍发生业务错误 | 只有 T1/T2，缺少语义验证 | 加契约不变量、验收场景和独立 Review |
| 周报越来越厚，没人读 | 每天重写全量分析 | 只记录增量、高严重度和 1–3 条治理提案 |
| 同类缺陷重复发生 | 只补代码，没有修根因/围栏 | 将根因映射到围栏并做反例测试 |
| Reviewer 无限挑毛病 | 缺证据门槛、误报复核、轮数限制 | 引入 Finding Verifier 和明确升级条件 |
| 两个 Agent 相互覆盖治理文件 | 没有单写入者/PR 协调 | 独立报告、单个治理 PR 聚合 |

## 15. 建议给项目负责人的第一条执行命令

```text
请以 MODE=bootstrap 执行 docs/fencing/AGENT_GUARDRAILS_MASTER.md 和
IMPLEMENTATION_PLAYBOOK.md 所定义的首次审计。
先只读，不更改代码和仓库设置；给出七层事实矩阵、GitHub 保护配置的
可验证/不可验证项、PR 检查重分配建议、三个最有价值的增量围栏提案。
每项建议有正反例验证计划、实施成本及授权要求。
在我批准试点之前不要建立大量文件、开启自动修改或强化 Required Checks。
```

---

### 官方资料索引（实施前应以项目环境核验）

- GitHub Rulesets: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets
- GitHub Actions schedule: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule
- Claude Code CLAUDE.md / skills / hooks / subagents: https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- Codex AGENTS.md: https://learn.chatgpt.com/docs/agent-configuration/agents-md
