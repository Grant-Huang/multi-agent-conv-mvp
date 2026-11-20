# AgentNexus MVP 工程约定（通用）

本文件是所有 AI 编码 agent 共用的规则入口。具体设计见 `docs/DESIGN.md`，当前计划见 `docs/PLAN.md`。
注意：本文件是指导，不是安全边界；禁止事项由 CI、分支保护和权限配置强制执行。
围栏落地说明见 `docs/fencing/README.md`；**提交 / 开 PR 前的自检清单见 [`docs/fencing/PREFLIGHT.md`](docs/fencing/PREFLIGHT.md)**。

## 开始前
1. 读 `docs/DESIGN.md`（架构）和 `docs/PLAN.md`（当前里程碑）；排查问题或改动已有设计前，先看 `docs/DECISIONS.md`（设计决策和理由）。
   MOM 业务工作台的 UI / UE 设计与开发还须遵守 [主界面设计原则](docs/mom/UI_UX_PRINCIPLES.md)，其场景验收要求作为界面变更评审基线。
2. 需求没有验收条件时，先提问，不要开始写代码。
3. 查看 `git status` 和 `git diff`，确认不会覆盖用户未提交的改动。

## 修改范围
- 不改与当前任务无关的模块。
- 不随意改公开接口：HTTP API、`config.toml` 字段、`AgentAdapter` trait。必须改时先说明理由。
- 修改数据库表结构必须附迁移，参考 `crates/runtime/src/store.rs` 中补列的写法。
- 新增或改变功能前，先按 `docs/testing/README.md` 创建端到端用例和可执行测试，再改实现；新行为须有业务断言 Red→Green 证据，环境失败不算 Red。既有行为补录标为基线。
- 验收用例定稿后只改业务代码，不改断言迎合实现；同步 `docs/testing/features.json`、功能地图和证据状态。
- 单次改动尽量集中在一个 crate 内、一个目标内。
- `web/src/bindings/` 下的 TypeScript 类型由 `an-core` 的 Rust 类型生成，不要手改：改了 `an-core` 的导出类型或 `web/` 用到的接口类型后，运行 `CARGO_TARGET_DIR=target cargo test -p an-core` 重新生成并一起提交（CI 会检查是否最新）。

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
4. **PR 正文**：含变更说明、**非目标**、Case/Feature ID、风险与回滚、验证证据；模板见 `.github/pull_request_template.md`。
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
- 定下来的事统一记在 `docs/DECISIONS.md`，不要散落在 PLAN 或各设计文档里。包括：用户拍板的选项、方案取舍、验收标准、放弃的做法、实施中偏离原方案的地方、待用户决定的冲突。
- 用简单的话写清：问题、定了什么、为什么、没选什么、出问题先看；写法见该文件开头的「怎么维护」。
- 没得到用户明确表态的，标「助手建议」，不要写成用户已确认；编号全文连续、只增不改。
- 决定被推翻时把原条目状态改为「已取代」，不删除。

## 日志
新增埋点使用 `docs/DESIGN.md` 第 10 节定义的事件名，并关联 `task_id`。
