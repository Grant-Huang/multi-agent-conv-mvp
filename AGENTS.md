# AgentNexus MVP 工程约定（通用）

本文件是所有 AI 编码 agent 共用的规则入口。具体设计见 `docs/DESIGN.md`，当前计划见 `docs/PLAN.md`。
注意：本文件是指导，不是安全边界；禁止事项由 CI、分支保护和权限配置强制执行。

## 开始前
1. 读 `docs/DESIGN.md`（架构）和 `docs/PLAN.md`（当前里程碑）。
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
- 不得通过关闭检查（`#[allow]`、跳过测试）消除错误；确有必要时写明理由。
- 不得绕过 CI，不得直接推送 `main`。
- 不得批准自己写的改动。

## 先停下来问人（高风险）
- 修改 `crates/runtime/src/orchestrator.rs`、`store.rs`、`registry.rs`（任务状态、执行组、持久化）之前，先给出方案。
- 修改 `crates/agents/src/acp.rs`、`util.rs` 中的子进程和 git 操作之前，先给出方案。
- 修改权限策略、删除数据、对外发送、部署。

## 完成的定义
声称完成前必须：
- `CARGO_TARGET_DIR=target cargo test` 通过；
- `CARGO_TARGET_DIR=target cargo clippy --all-targets` 无警告；
- `cd web && npm run build` 通过；
- 修 bug 附回归测试；
- 涉及子进程或异步操作的改动，说明超时、取消和清理策略；
- 在回复中贴出上述命令的实际输出。

## 日志
新增埋点使用 `docs/DESIGN.md` 第 10 节定义的事件名，并关联 `task_id`。
