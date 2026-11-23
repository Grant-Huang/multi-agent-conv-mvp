# git-evidence：Git 改动证据

ACP / Coding 执行后收集 git diff 作为审查证据，无界面入口。覆盖与证据状态见 [COVERAGE.md](../COVERAGE.md) 的“Git 改动证据”一节。

## Sub-features

- 执行前快照与兜底都在**私有索引**里采集，不修改用户暂存区
- 覆盖空仓库、链接 worktree、并发
- 失败明确返回“证据不可用”，不伪造 diff

## How to get to it (user POV)

用户不直接操作。让 Coding / ACP agent 改代码，完成后在任务卡片时间线与审查证据里看到 git diff。

## Driving it with Playwright / HTTP

```bash
CARGO_TARGET_DIR=target cargo test --locked -p an-agents --test git_evidence    # RT-GIT-01～05
```

界面侧：E2E-CODEFLOW（`web/e2e/codeflow.spec.ts`）接入模拟 ACP（`an-mock-acp`），发“改代码验证：…”，在 harness 临时 git workdir 写 `hello.txt` 并断言 git/diff 证据与审查通过。

## Verify（判据）

- 采集前后用户暂存区（index）不变。
- 每条 Git 命令有 30 秒期限；超时/取消后进程组被终止并回收，临时目录已清理。
- 失败时证据字段明确标记不可用。
- 浏览器：执行者 claude、review pass、证据含 `git_diff_stat` 或 diff 产物。

## Gotchas

- 修改 `crates/agents/src/util.rs` 的子进程和 git 操作属于**高风险**，须先给方案（AGENTS.md）。
- RT-GIT-* 是后端测试；整链浏览器见 E2E-CODEFLOW。
- `workdir` 应指向独立 git 仓库，否则没有可收集的 diff。

## Backed by

源码：`crates/agents/src/util.rs`；夹具 `web/e2e/harness.mjs`
用例：RT-GIT-01～05（`crates/agents/tests/git_evidence.rs`）；E2E-CODEFLOW；证据 [R-FIRST-06](../../testing/evidence/r-first-06.md)
验证状态：后端 + 浏览器整链（WP3）。
