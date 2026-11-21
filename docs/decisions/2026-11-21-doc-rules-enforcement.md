# 文档规则写进 AGENTS.md 等入口文件，并由 CI 文档检查先警告、后强制

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-21 |
| 来源 | 用户确认（“批准 P5”） |
| 状态 | 有效；CI 检查处于过渡期（只警告），约一周后改为阻断，届时另记一条 |
| 影响范围 | `AGENTS.md`、`CLAUDE.md`、`REVIEW.md`、`FIX.md`、`.github/pull_request_template.md`、`.github/workflows/ci.yml`、`tools/ci/doc_lint.py` |
| 相关设计 | [文档体系整理计划](../DOC_SYSTEM_PLAN.md) 第 4 节（P5 方案）；[decisions/README.md](README.md)（决策写法） |

**一句话结论**：把“先读宪章和契约、新决策一条一个文件并写清依据、新特性先登记”写进所有 agent 开工必读的入口文件；再加一个 CI 文档检查，过渡期只提示，覆盖那些已经在跑、不会重读入口文件的老会话。

## 背景

这个项目由多个 AI agent 会话并行开发。会话开始时，Claude Code 通过 `CLAUDE.md` 导入 `AGENTS.md`，Cursor 自动读取根目录 `AGENTS.md`；夜间评审和修复 agent 另外按 `REVIEW.md`、`FIX.md` 工作。`docs/` 下新增的规范文档不会被自动读取。

2026-11-21 已经合并的文档整理（PR #188、#192）新增了产品宪章、共享契约、单文件决策目录，但入口文件里仍写着“定下来的事统一记在 `docs/DECISIONS.md`”。

## 问题

1. 新会话按旧入口文件工作，会继续往 `DECISIONS.md` 末尾追加编号，也不会去读宪章和契约。
2. 已经在跑的老会话不会重读入口文件，只能在碰巧读到某个文件时才跟上新规则。
3. 夜间评审不检查宪章、契约和决策写法，偏离了也没人提示。

## 决策依据

1. **入口文件是唯一会被自动读取的规则来源**：Claude Code 在会话开始时加载 `CLAUDE.md` 及其导入的 `AGENTS.md`（[Claude Code 文档](https://code.claude.com/docs/en/memory)）；Cursor 自动读取根目录 `AGENTS.md`（[Cursor Rules 文档](https://cursor.com/docs/rules)）。
2. **老会话会读 CI 和合并冲突的输出并照着改**：2026-11-21 Cursor 会话在 PR #187（T-50）合并 master 时，读到 `DECISIONS.md` 开头的试行说明，就改用了 `docs/decisions/` 单文件写法；但它只做对一半（仍在 `DECISIONS.md` 末尾加了一行，决策文件缺依据等小节）。说明“被动读到”有效但不可靠，需要一个每次 PR 都会出现的提示。
3. **检查规则在真实分支上有效**：在 T-50 分支上运行 `tools/ci/doc_lint.py`，正确指出其决策文件“开头表格缺字段：影响范围、相关设计”“缺必备小节：背景、问题、决策依据……”；在 PR #188、#192 合并后的历史上运行，没有误报链接问题（2026-11-21 实测）。
4. **先警告再强制**：入口文件改动和 CI 规则同时阻断，会让还没改用新写法的进行中分支突然无法合并；过渡期只提示，给各会话一次自行修正的机会（[decisions/2026-11-21-doc-layers](2026-11-21-doc-layers.md) 定下的推进方式）。
5. **用户意见**（2026-11-21）：“批准 P5”。

## 定了什么

- `AGENTS.md`：
  - 开头和“开始前”第 1 条改为阅读顺序：宪章 → DESIGN 第 0 节 → 相关共享契约 → 所属特性的权威设计 → PLAN → 决策记录；
  - “修改范围”加一条：新特性先在 DESIGN 第 0.3 节登记；触及契约或宪章先出方案；
  - “决策记录”改为：新决策写到 `docs/decisions/`，写清依据，必要时附简单设计；`DECISIONS.md` 只给旧条目标“已取代”。
- `CLAUDE.md`：压缩后需要重读的文件加上宪章和契约。
- `REVIEW.md` 第 2 节、`FIX.md` 开头：必读清单加上宪章、契约、决策目录；评审核对是否偏离宪章或契约、新决策是否写清依据。
- PR 模板：新增“设计一致性”一节（属于哪个特性、是否触及契约、是否偏离宪章、新增了哪些决策）。
- CI：新增 `tools/ci/doc_lint.py`，在 `change-policy` 作业中运行，`continue-on-error`，脚本默认退出码 0。

## 简单设计

| 对象 | 怎么知道新规则 |
|---|---|
| 新开的会话 | 开工时自动读 `AGENTS.md`（Claude Code 经 `CLAUDE.md` 导入） |
| 已在跑的老会话 | 推 PR 时在 CI 的“文档检查”步骤里看到警告 |
| 夜间评审、修复 agent | `REVIEW.md`、`FIX.md` 的必读清单 |
| 写 PR 的人 | PR 模板“设计一致性”一节 |

`doc_lint.py` 的检查项：

| 规则 | 检查什么 | 范围 |
|---|---|---|
| `broken-link` | 相对链接和锚点是否有效 | 新增行 |
| `decision-name` / `decision-header` / `decision-section` | 决策文件命名、开头表格字段、必备小节 | `docs/decisions/` 下新增文件 |
| `decisions-append` | 是否往 `DECISIONS.md` 追加了 `### N.` 条目（含重号提示） | 新增行 |
| `design-unregistered` | 新增设计文档或特性档案是否在 DESIGN 第 0.3 节登记 | 新增文件 |
| `pr-design-consistency` | PR 正文是否有“设计一致性”一节 | 非纯测试 PR |

过渡期结束时：CI 命令加 `--strict`，去掉 `continue-on-error`（L3 改动，另记决策）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 只改文档，不加 CI | 老会话不会重读入口文件（依据 2） |
| CI 一上来就阻断 | 进行中的分支会突然无法合并（依据 4） |
| 检查存量文档的所有坏链 | 会给每个 PR 带来大量与改动无关的警告；只查新增行 |
| 检查决策写得好不好（例如依据是否充分） | 机器判断不了；交给评审 |

## 影响与后续

- 约一周后（2026-11-28 前后）评估警告数量和误报，决定是否切换为阻断。
- 2a 规范（特性档案、登记表、交接说明、版本清单）定稿后，`doc_lint.py` 的“设计文档登记”规则会随特性档案目录一起调整。
- 已在跑的会话：建议各团队到检查点后开新会话，见 [文档体系整理计划](../DOC_SYSTEM_PLAN.md)。

## 出问题先看

- CI `change-policy` 作业里“文档检查（只警告）”步骤的输出，或构件 `risk-report` 里的 `doc-lint.json`。
- 本地复现：`python3 tools/ci/doc_lint.py --base origin/master`（PR 正文用环境变量 `PR_BODY` 传入）。
- 误报：看 `tools/ci/tests/test_doc_lint.py` 里对应规则的用例，补一个复现用例后再改规则。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-21 | 新建 | [DOC_SYSTEM_PLAN](../DOC_SYSTEM_PLAN.md) P5 |
