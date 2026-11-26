# AgentNexus 研发规范（DEV_STANDARD）

| 项目 | 内容 |
|---|---|
| 版本 | v0.4，2026-11-26 |
| 状态 | 第 1～8 节有效（2026-11-21 用户确认）；特性档案、需求编号与登记表扩展字段从 World Adapter 试点开始执行，其余特性迁移前沿用现有文档 |
| 地位 | **唯一的研发规范**：需求 → 设计 → 实现 → 质量 → 版本怎么对齐、风险怎么分级、谁能授权。Agent 入口文件（`AGENTS.md`、`CLAUDE.md`）只引用本文，不复制 |
| 围栏 | 七层围栏、检查节奏、证据等级、围栏登记表见 [docs/fencing/README.md](docs/fencing/README.md) |
| 模板 | 在 [docs/spec/](docs/spec/README.md)：特性档案、授权契约、交接说明、版本说明、迁移指南 |
| 修改 | 本文属 L3 治理文件：改动须说明理由、用户批准，并在 [docs/decisions/](docs/decisions/README.md) 记一条决策；每日检查结果的变化不改本文版本 |

## 1. 要解决的问题

一个特性的“为什么做、要什么、怎么做、做到哪、在哪个版本里”曾散在设计文档、决策、PLAN、CASES、功能地图、证据等多处，靠人工互相引用，改一处忘一处，出现多路不一致（例子见 [decisions/2026-11-21-doc-layers](docs/decisions/2026-11-21-doc-layers.md) 的决策依据）。

目标：**每个特性有且只有一个入口文档；需求、设计、实现、质量、版本五个环节各有唯一的位置和编号；链条能被机器检查；能自主推进的工作在授权范围内连续完成，高后果的改动在动手前就被拦住。**

## 2. 三套编号，不要混用

| 编号 | 指什么 | 定义在哪 |
|---|---|---|
| **D0～D3** | 文档层级：D0 产品宪章、D1 架构与共享契约、D2 特性档案、D3 计划与任务卡 | 本文第 3 节 |
| **L0～L3** | 变更风险等级（`tools/ci/risk_classify.py` 输出，CI 与影子审查使用） | `config/risk-policy.toml`；与草案中 R0～R3 的对照见本文第 6 节 |
| **第一层～第七层** | 七层围栏 | [docs/fencing/README.md](docs/fencing/README.md) |

## 3. 五个环节各放在哪

| 环节 | 唯一位置 | 编号 | 谁来维护 |
|---|---|---|---|
| 产品方向（D0） | [docs/PRODUCT.md](docs/PRODUCT.md) | 节、条 | 改动须用户确认 |
| 架构与跨特性约定（D1） | [docs/DESIGN.md](docs/DESIGN.md) 第 0 节；[docs/contracts/](docs/contracts/README.md) | 契约不变量编号（如 A1、S3） | 改动须出方案、用户批准 |
| 意图与需求（D2） | 特性档案 `docs/features/<特性>/README.md` 的“意图”“需求” | 特性 ID（沿用 `features.json` 的 `id`）；需求 `REQ-<特性>-NN` | 提出需求的人 |
| 设计（D2） | 特性档案“设计”一节；太长时拆同目录子文档，档案里保留要点和链接 | 章节锚点 | 实现该特性的会话 |
| 实现 | 代码；档案“实现入口”列路径；PR 写需求编号 | 提交、PR | 实现者 |
| 质量 | 用例在 [docs/testing/CASES.md](docs/testing/CASES.md)，每条需求挂用例编号；证据在 `docs/testing/evidence/` | `E2E-*`、`RT-*` | 实现者写，评审核对 |
| 版本 | git tag `vX.Y.Z` + `docs/releases/vX.Y.Z.md` | 语义化版本号 | 用户打 tag；版本说明由登记表生成 |
| 进度（D3） | [docs/PLAN.md](docs/PLAN.md) 及各特性计划：只放里程碑勾选，写需求编号或特性 ID | — | 各会话 |
| 为什么这样定 | [docs/decisions/](docs/decisions/README.md)，一条一个文件；档案“决策”一节列出相关文件 | 文件名 | 做决定的会话；来源如实标注 |

**不允许**：在计划、任务卡、决策、宪章里重新描述某个特性的意图或设计。需要时写一句要点并链接到特性档案。

### 3.1 一件事只在一处描述（单一权威来源）

上面的“唯一位置”不只适用于特性：**任何会被多处提到的内容，只在一个权威位置完整描述，其他文档引用它**（2026-11-26 用户确认，见 [决定 2026-11-26-single-source-docs](docs/decisions/2026-11-26-single-source-docs.md)）。

| 内容 | 权威位置 | 其他文档怎么写 |
|---|---|---|
| 研发线的职责 | [docs/INTEGRATION_PLAN.md](docs/INTEGRATION_PLAN.md)「五条研发线与近期汇合」表 | 只写线号，例如“第 4 线”，需要时链接到该表 |
| 特性归哪条线 | [docs/DESIGN.md](docs/DESIGN.md) 第 0.3 节“负责线”列 | 只填线号，不复述职责 |
| 宪章条款 | [docs/PRODUCT.md](docs/PRODUCT.md) | 引用条款编号（如 CH-G2c），可加一句要点 |
| 共享约定 | [docs/contracts/](docs/contracts/README.md) | 引用不变量编号（如 A7） |
| 验收用例 | [docs/testing/CASES.md](docs/testing/CASES.md) | 引用用例编号 |
| 为什么这样定 | [docs/decisions/](docs/decisions/README.md) | 引用决策文件名 |
| 研发规则 | 本文 | `AGENTS.md`、`CLAUDE.md` 只列要点并链接 |
| 其他内容 | 第一次完整写下它的地方即为权威；需要改位置时记决策 | 一句要点 + 链接 |

规则：

1. 非权威处最多写**一句要点**并链接到权威位置；不复制表格、清单或长段说明。
2. 改权威内容时，同一个 PR 里检索（`grep`）其他文档中的复述，改成引用；不属于本线的文档，开 Issue 派给负责的研发线。
3. 发现两处描述不一致时，以权威位置为准，把其他处改成引用；权威位置本身有错，按改动等级走流程修正。
4. 历史记录（证据、评审报告、已取代的决策）保留当时原文，不追改。

## 4. 特性档案与登记表

**特性档案**：每个特性一个目录 `docs/features/<特性 ID>/`，入口 `README.md`，模板见 [feature-dossier-template.md](docs/spec/feature-dossier-template.md)。必备小节：意图、需求、设计（含引用的契约）、实现入口、质量、版本历史、决策、待决定、来源。子文档必须从 `README.md` 链接，不能单独作为入口。

**登记表** `docs/testing/features.json`：保留现有字段，每个特性补：

| 新字段 | 含义 |
|---|---|
| `dossier` | 特性档案路径 |
| `status` | 设计中 / 开发中 / 已发布 / 已废弃 |
| `requirements` | 需求列表：`id`、`title`、`status`、`cases`、`since` |
| `contracts` | 引用的共享契约 |
| `decisions` | 相关决策文件名 |

校验扩展 `web/verification/check-map.mjs`（随 `npm run test:map`）：档案存在；档案与登记表的需求编号一致；“已确认”及以后的需求至少挂一个存在的用例；`contracts`、`decisions` 指向的文件存在；“已发布”的需求有 `since`，且对应版本说明存在。

## 5. 授权：每项新行为都要有有效的授权来源

| 来源 | 能授权什么 | 不能授权什么 |
|---|---|---|
| 用户在对话或 PR 里的明确批准 | 所批准的范围 | 超出所述范围的事 |
| 已确认的特性需求（`REQ-*` 状态“已确认”） | 实现该需求所需的子任务、测试、局部重构 | 改变需求含义、改共享契约 |
| 已批准的里程碑计划或任务卡 | 计划内列出的工作 | 计划外的新行为 |
| **授权契约**（模板见 [autonomy-contract-template.md](docs/spec/autonomy-contract-template.md)） | 契约写明的任务包、预算、允许动作 | 契约列出的禁止动作；触发升级条件后的继续执行 |

**始终要先停下来问人**（不受授权契约豁免）：`AGENTS.md`“先停下来问人”列出的高风险文件和操作；扩大范围；改共享契约或宪章；对外发送、删除数据、部署；降低任何检查的强度。

缺少低风险细节（命名、格式、内部结构）时，可以记录假设后继续，并在 PR 里列出假设。

## 6. 风险等级与必需检查

风险等级由 `tools/ci/risk_classify.py` 按路径和内容给出初值；**无法判断影响时按更高一级处理，不能默认最低**。PR 行数只用于提醒审查复杂度，不决定风险。

| 等级 | 典型改动 | 对应草案 | 合并前必须 |
|---|---|---|---|
| L0 | 文档、评测数据 | R0 | CI 全绿；文档检查 |
| L1 | 普通业务代码 | R1 | 相关测试；轻量审查 |
| L2 | 核心状态机、持久化、子进程、公共接口 | R2 | 影响分析、契约或集成测试、审查人不是作者 |
| L3 | 治理文件与 CI、权限策略、节点与远程执行 | R3（部分） | 用户或 CODEOWNERS 批准；先出方案 |

现有 L3 以“治理与安全文件”为主；草案中 R3 还包括认证与租户隔离、生产数据、破坏性迁移、发布权限，这些目前分散在 L2/L3 和 `AGENTS.md`“先停下来问人”里，按更严的一条执行。

**不随风险等级降低的底线**：凭据保护、权限限制、secret 扫描（`security` 作业）、依赖漏洞检查、高风险文件保护、可核对的执行证据。

**检查例外**：任何例外（包括 CI 中的 `continue-on-error`、试点期的只警告规则）都要在 [围栏登记表](docs/fencing/GUARDRAIL_REGISTRY.yaml) 写明负责人、原因、缓解措施和到期日。不得用禁用测试、改断言掩盖失败。历史遗留问题与本次引入的问题分开报告。

## 7. 一次改动怎么走

| 改动类型 | 必须做的 |
|---|---|
| **新需求或行为变化** | ① 特性档案“需求”加一行，确认授权来源（第 5 节）；② 改档案“设计”一节，触及契约先改契约（须批准）；③ 在 CASES 写可执行的验收用例，看到**业务断言**失败（Red，环境失败不算）；④ 实现到通过（Green）；⑤ 更新登记表与档案状态；⑥ PR 正文写特性 ID、需求编号、“设计一致性” |
| **修 bug（不改需求）** | 先写复现测试看到 Red，再修到 Green；PR 写所属需求或用例编号；设计不变时不改档案 |
| **纯重构** | 改前改后同一组测试都通过（Green → Green）；不改需求和用例断言 |
| **基础设施（CI、脚本、配置）** | 写正例和反例测试，证明该拦的拦住、该放的放过；涉及 L3 先出方案 |
| **新特性** | 先建档案目录并在 DESIGN 第 0.3 节登记，写明为宪章哪一条服务；再按“新需求”走 |
| **做了取舍** | 按 [决策写法](docs/decisions/README.md) 记一条，档案“决策”一节加链接 |

**同步分阶段**：PR 里只更新本次受影响的需求、设计、实现入口、测试和索引，没变化的环节不机械更新；`since`、tag、版本说明在**发版时**更新。

## 8. 证据等级：说“做到了”之前分清做到哪一步

| 等级 | 含义 | 例子 |
|---|---|---|
| DECLARED | 文档里声称有 | `AGENTS.md` 写了“先停下来问人” |
| IMPLEMENTED | 配置、脚本、测试存在 | `doc_lint.py` 已合并 |
| VERIFIED | 有真实执行记录，且正例、反例都测过 | 在某个 PR 的 CI 日志里看到它的输出 |
| ENFORCED | 在可信的权限边界下无法正常绕过 | 必需检查加分支保护，且做过绕过试验 |
| NOT_VERIFIED | 没能核实 | 本会话看不到 GitHub 仓库设置 |

追踪检查分三级：**T1 结构**（编号、文件、引用存在且格式正确，如 `check-map.mjs`、`doc_lint.py`）；**T2 运行**（相关测试实际执行，退出码和通过/失败/跳过数可核对）；**T3 语义**（独立审查或业务验收证明断言对应真实业务规则）。T1 通过不能说成 T3。

在 PR、决策、版本说明里报告完成情况时，用上面的等级说话；没运行的检查写“未运行”，不能写“通过”。

## 9. 版本与里程碑准出

- 版本号用语义化版本 `vX.Y.Z`，在 `master` 上打 tag，由用户执行。已有：`v1.0.0`（2026-11-17 论文实验版本，指向 `0d0133c`）。
- 里程碑准出时出一份版本说明（模板 [release-template.md](docs/spec/release-template.md)），给出 **PASS / CONDITIONAL / BLOCKED** 结论；CONDITIONAL 只用于有负责人、有缓解措施、有到期日的非关键剩余风险，安全或不可逆问题不能有条件放行。
- 缺少真实部署或运行证据时写 NOT_VERIFIED，不写 PASS。

## 10. 老文件迁移与交接

- 迁移：一个特性一个 PR，先试点 World Adapter；老文件分四类（并入、拆分、保留为子文档、归档），不删除，改为跳转说明；档案“来源”一节列对照表。步骤见 [migration.md](docs/spec/migration.md)。
- 交接：会话结束或到检查点时，按 [handoff-template.md](docs/spec/handoff-template.md) 写在该分支草稿 PR 的正文里；新会话从交接说明和特性档案开始，不靠对话记忆。

## 11. 机器检查现状与计划

| 检查 | 位置 | 现状（证据等级） |
|---|---|---|
| 文档链接、决策文件格式、`DECISIONS.md` 追加、新设计文档登记、PR“设计一致性” | `tools/ci/doc_lint.py`（CI `change-policy`） | VERIFIED；试点只警告，到期 2026-11-28 |
| 登记表与用例的引用 | `web/verification/check-map.mjs` | VERIFIED（T1）；需求字段待试点时扩展 |
| 特性档案必备小节、PR 写需求编号 | `doc_lint.py` | 计划：试点完成后加入，先警告 |
| 发版时版本说明与 `since` 一致 | 发版脚本或 CI | 计划：首次按本规范发版时 |

全部围栏的状态以 [GUARDRAIL_REGISTRY.yaml](docs/fencing/GUARDRAIL_REGISTRY.yaml) 为准。

## 12. 待决定

- 试点开始时间：PR #187（T-50）已于 2026-11-21 合并，试点可开始；实际首个特性档案是分层记忆（`docs/features/conversation-memory/`），World Adapter 尚未迁移。
- 基线版本号：建议 `v1.1.0`，在开始迁移前由用户打 tag（2026-11-24 远端仍只有 `v1.0.0`）。
- 论文约束：已于 2026-11-24 解除，论文复现以 `v1.0.0` 为准，见 [2026-11-24-lift-paper-constraint](docs/decisions/2026-11-24-lift-paper-constraint.md)。

## 修订记录

| 版本 | 日期 | 变化 | 决策 |
|---|---|---|---|
| v0.1 | 2026-11-21 | 草案，位于 `docs/spec/README.md` | [2026-11-21-doc-rules-enforcement](docs/decisions/2026-11-21-doc-rules-enforcement.md) |
| v0.2 | 2026-11-21 | 移到根目录作为唯一研发规范；加入编号约定、授权、风险对照与底线、例外管理、按任务类型的 TDD、同步分阶段、证据等级、里程碑结论 | [2026-11-21-dev-standard-and-guardrails](docs/decisions/2026-11-21-dev-standard-and-guardrails.md) |
| v0.3 | 2026-11-24 | 第 12 节：更新试点状态、基线 tag 现状，记录论文约束解除 | [2026-11-24-lift-paper-constraint](docs/decisions/2026-11-24-lift-paper-constraint.md) |
| v0.4 | 2026-11-26 | 新增第 3.1 节：一件事只在一处描述（单一权威来源），列出常见内容的权威位置 | [2026-11-26-single-source-docs](docs/decisions/2026-11-26-single-source-docs.md) |
