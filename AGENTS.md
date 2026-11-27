# AgentNexus MVP 工程约定（通用）

本文件是所有 AI 编码 agent 共用的规则入口。研发规范（需求→设计→实现→质量→版本、风险分级、授权、证据等级）见根目录 `DEV_STANDARD.md`，七层围栏与现状见 `docs/fencing/README.md`；产品定位与不能破的原则见 `docs/PRODUCT.md`（产品宪章），系统全景与各特性的设计入口见 `docs/DESIGN.md` 第 0 节，当前计划见 `docs/PLAN.md`。本文只列必须一直遵守的要点，细则以 `DEV_STANDARD.md` 为准。
注意：本文件是指导，不是安全边界；禁止事项由 CI、分支保护和权限配置强制执行。
围栏落地说明见 `docs/fencing/README.md`；**提交 / 开 PR 前的自检清单见 [`docs/fencing/PREFLIGHT.md`](docs/fencing/PREFLIGHT.md)**。

## 开始前
1. 按顺序读：`DEV_STANDARD.md`（研发规范）→ `docs/PRODUCT.md`（产品宪章：做什么、不做什么、不能破的原则）→ `docs/DESIGN.md` 第 0 节（系统全景与产品特性设计索引）→ `docs/contracts/` 中与本任务相关的共享契约 → 本任务所属特性的权威设计（见第 0.3 节）。再读 `docs/PLAN.md`（当前里程碑）；排查问题或改动已有设计前，先看决策记录（`docs/decisions/` 与 `docs/DECISIONS.md`）。
   MOM 业务工作台的 UI / UE 设计与开发还须遵守 [主界面设计原则](docs/mom/UI_UX_PRINCIPLES.md)，其场景验收要求作为界面变更评审基线。
2. 需求没有验收条件、或找不到有效的授权来源（`DEV_STANDARD.md` 第 5 节）时，先提问，不要开始写代码。
3. 查看 `git status` 和 `git diff`，确认不会覆盖用户未提交的改动。
4. 查看派给本线的 open Issue：GitHub 标签 `dispatch` 加 `line:N`（N 为本线编号，见 `docs/INTEGRATION_PLAN.md`「研发线与近期汇合」表）。先做 `severity:high`。**开始做一个 Issue 时必须先在 Issue 留言认领**（写明本线、计划和预计 PR）；暂时不做的也要留言说明原因和预计时间。完成的 PR 正文写 `Closes #N`，合并后 Issue 自动关闭。**每次醒来、每完成一张单、每次播报或进入待命前，都要重新查一次本线开着的派工单**，不能只按本线自定的触发条件行动。

## 修改范围
- 不改与当前任务无关的模块。
- **一件事只在一处描述**：研发线职责、特性负责线、宪章条款、契约、用例、决策等都有唯一权威位置（见 `DEV_STANDARD.md` 第 3.1 节）；其他文档最多写一句要点并链接。改权威内容时，同一个 PR 里把别处的复述改成引用。
- 新特性或新主线开工前，先在 `docs/DESIGN.md` 第 0.3 节登记，写明为 `docs/PRODUCT.md` 哪一条服务；改动触及共享契约（`docs/contracts/`）或宪章时，先出方案、经用户批准。
- 不随意改公开接口：HTTP API、`config.toml` 字段、`AgentAdapter` trait。必须改时先说明理由。
- 修改数据库表结构必须附迁移，参考 `crates/runtime/src/store.rs` 中补列的写法。
- 新增或改变功能前，先按 `docs/testing/README.md` 创建端到端用例和可执行测试，再改实现；新行为须有业务断言 Red→Green 证据，环境失败不算 Red。既有行为补录标为基线。
- 验收用例定稿后只改业务代码，不改断言迎合实现；同步 `docs/testing/features.json`、功能地图和证据状态。
- 单次改动尽量集中在一个 crate 内、一个目标内。
- `web/src/bindings/` 下的 TypeScript 类型由 `an-core` 的 Rust 类型生成，不要手改：改了 `an-core` 的导出类型或 `web/` 用到的接口类型后，运行 `CARGO_TARGET_DIR=target cargo test -p an-core` 重新生成并一起提交（CI 会检查是否最新）。

## 跨线分工
- 研发线的划分以 `docs/INTEGRATION_PLAN.md`「研发线与近期汇合」表为准；线的数量和职责会变，派工前重读，不凭记忆。
- 发现明确属于其他线的工作（缺陷、改进、测试缺口、文档），不要顺手做：开 GitHub Issue 派给对应线，标签 `dispatch` + `line:N`（可加 `severity:high|medium|low`），标题以“[第N线]”开头，正文写来源、问题、证据、验收标准。跨线的写明主责线和配合线。
- 拿不准归属时标 `line:2`（独立复跑与集成协调，负责问题归属），或先问用户。
- **一个 Issue 只有一个主责线**：只挂主责线的 `line:N`，配合线写在正文里。主责线负责关闭（有 PR 用 `Closes #N`；没有 PR 的贴结论与证据后关闭）；配合线完成自己那部分只留言。工作能拆开时开总 Issue 加按线的子 Issue，子 Issue 全关后由主责线或派单方关闭总 Issue。公告单用“[各线通知]”开头、标签 `announcement`，不关闭，按月或超过 30 条评论时换新单。细则见 `docs/decisions/2026-11-25-dispatch-to-workstreams.md` 第 10 条。
- 本线任务依赖其他线先交付时，同样开 Issue 派过去，并在本线 PR 或 Issue 里引用。
- 分诊结论自己落地，不停在“建议”：判定已修复的、与其他 Issue 重复的，分诊方直接关闭并写明依据（重复的标 duplicate 并指向保留的 Issue）；写了“拟派第 N 线”的，当场开出派工 Issue 并在原 Issue 贴链接。拿不准是否该关的，留言说明后标 `line:2`。
- 派工时限：派工后 **8 小时内**留言认领（或说明暂不做的原因和预计时间）；认领后 `severity:high` 3 天内、`severity:medium` 7 天内要有 PR 或进展说明，`low` 不限。超时由自动巡检（每 4 小时）在 Issue 上提醒并汇总给用户，不自动改派。
- **控制在制品**：每条线同时待合并（非草稿）的 PR 最多 3 个（含 L0）；全仓风险等级 L1～L3 的待合并 PR 超过 12 个时暂停开新开发任务，先评审、修 CI、消冲突。`severity:high` 缺陷、CI 与安全修复例外，PR 正文写明原因。细则见 [`docs/fencing/INTEGRATION_PIPELINE.md`](docs/fencing/INTEGRATION_PIPELINE.md)。
- **PR 只合进 master**：PR 的目标分支必须是 `master`，不做堆叠 PR（以别人尚未合并的分支为目标）。依赖别人尚未合并的改动时，等对方合并后把 master 合进本分支。合进其他分支的改动不会关闭 Issue，承接的 PR 一旦关闭，改动就会丢失（2026-11-26 #386、#387 的事故）。
- **每条线同时开着的派工单最多 5 张**（`severity:high` 缺陷除外）；超出的由派单方标 `queued` 排队，等该线消化后再开始。
- **合并与冲突**：风险等级以 CI `change-policy` 给出的为准。L0 在 CI 全绿后由作者所在线自行合并；L1～L3 按审批要求等人批准。自己的 PR 落后主干或冲突时，把 master 合进本分支（不 rebase、不强推、不动别人的分支），重跑提交前自检再等 CI；冲突涉及“先停下来问人”的文件或需要在两种行为间取舍时，停下来在 PR 里说明并问第 2 线或用户。
- **测试基于基线**：长程测试、验收复跑、体验走查基于最近一次夜间全绿的基线 tag，报告写明所用 tag；开发不因测试冻结。

## 禁止
- 不得修改或删除已有测试来让失败的测试通过。
- 不得通过关闭检查（`#[allow]`、跳过测试）消除错误；确有必要时写明理由，并在 PR 中加 `## 测试变更说明`（≥ 20 字）。
- 不得绕过 CI，不得直接推送 `master`（或 `main`）。
- 不得批准自己写的改动。

## 先停下来问人（高风险）
- 修改 `crates/runtime/src/orchestrator.rs`、`store.rs`、`registry.rs`（任务状态、执行组、持久化）之前，先给出方案。
- 修改 `crates/agents/src/acp.rs`、`util.rs` 中的子进程和 git 操作之前，先给出方案。
- 修改权限策略、删除数据、对外发送、部署。
- 修改 `.github/**`、`config/risk-policy.toml`、`tools/ci/**`、本文件（L3）之前，先说明理由；合并须 CODEOWNERS 批准。

## 提交前自检
在 **push、开 PR、或声称完成** 之前必须做完；细则与可复制命令见 [`docs/fencing/PREFLIGHT.md`](docs/fencing/PREFLIGHT.md)。摘要：

1. **范围**：`git diff --stat origin/master`；跑 `python3 tools/ci/risk_classify.py --base origin/master`。L2/L3 或高风险路径须已获批准。
2. **质量**（与 CI 一致，贴真实输出）：
   - `CARGO_TARGET_DIR=target cargo fmt --all -- --check`
   - `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings`
   - `CARGO_TARGET_DIR=target cargo test --locked --workspace`
   - `cd web && npm run typecheck && npm test && npm run test:map && npm run build`
3. **策略**：用准备好的 PR 正文作 `PR_BODY`，跑  
   `python3 tools/ci/change_size_check.py --base origin/master` 与  
   `python3 tools/ci/injection_scan.py --base origin/master`。  
   **block** 必须先消除或按规则补说明段（大改动 / 测试删除 / 测试变更 / 注入扫描）。
4. **PR 标题与正文写给人看**：标题用 `[第N线][风险等级] 一句白话结果（Closes #N）`，不用分支名或内部编号当标题；正文先写三行以内的“给审查人”（做了什么、为什么、需要你做什么），再写“需要你决定”“怎么看效果”，然后是技术细节（变更说明、**非目标**、Case/Feature ID、风险与回滚、验证证据）。缩写与编号第一次出现时附说明或链接，未登记到 `docs/GLOSSARY.md` 的不得出现在标题和“给审查人”里；派工 Issue 同样适用。模板见 `.github/pull_request_template.md`。
5. **自检未通过不得 push / 不得声称完成。**

## 完成的定义
声称完成前必须：
- 已完成上一节「提交前自检」；
- `CARGO_TARGET_DIR=target cargo test --locked` 通过；
- `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets` 无警告；
- `cd web && npm run build` 通过；涉及功能地图时 `npm run test:map` 通过；
- 修 bug 附回归测试；新行为有 Red→Green 证据；
- 涉及子进程或异步操作的改动，说明超时、取消和清理策略；
- 在回复中贴出上述命令的实际输出。

## 决策记录
- 定下来的事记在 `docs/decisions/`，一条一个文件，按该目录 README 的模板写，不要散落在 PLAN 或各设计文档里。包括：用户拍板的选项、方案取舍、验收标准、放弃的做法、实施中偏离原方案的地方、待用户决定的冲突。
- `docs/DECISIONS.md` 是 2026-11-21 以前的历史档案，只给旧条目标「已取代」，不再追加新条目（CI 文档检查会提示）。
- 每条决策写给没参加讨论的新接手的人：必须写清**决策依据**（数据、代码事实、测试或评审编号、外部资料、用户意见，都要能核对；推断标「推断」）；决定涉及结构、接口、状态或流程时，附简单设计，或引用设计文档的具体章节并概括要点。
- 没得到用户明确表态的，来源标「助手建议」，不要写成用户已确认。
- 决定被推翻时把原文件状态改为「已取代」并写明被哪个文件取代，不删除。

## 日志
新增埋点使用 `docs/DESIGN.md` 第 10 节定义的事件名，并关联 `task_id`。
