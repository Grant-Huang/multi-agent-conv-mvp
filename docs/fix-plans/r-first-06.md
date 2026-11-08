# R-FIRST-06：Git 兜底证据采集不触碰用户暂存区

基线：origin/master e0811a2。任务来源：docs/review/2026-11-07.md 交接第 3 项及台账 open 条目，重开 0 次。

## 问题回顾

`crates/agents/src/util.rs::collect_git_changes` 在基线/结束快照无法获得的兜底路径直接执行 `git add -N .`，把用户未跟踪文件加入真实索引。即便不真正暂存内容，也会改变 intent-to-add 状态；不能把证据采集当作修改用户仓库索引的授权。当前 snapshot_worktree 正常路径已使用临时 GIT_INDEX_FILE，兜底路径没有同样隔离。

## 推荐方案与备选

- 仅改 util.rs 的 Git 子进程部分及新增回归测试；复用/提取私有临时索引生命周期 helper，明确任何 add/read-tree/write-tree 都使用该次独立 GIT_INDEX_FILE。只读 rev-parse/diff 可使用真实索引，但不得执行会改变其内容或刷新它的写操作。
- 推荐兜底也在私有索引中执行 intent-to-add，然后从该索引读 diff/stat。初始化应保留原有已暂存/未暂存边界；空仓库、没有 index、worktree 的 git-path index、带空格的路径均正确处理。遵守 .gitignore，不把忽略文件作为新交付物。
- 保留所有原始索引字节、已暂存内容和标记；不得用“最后把索引写回”补救，因为会覆盖用户并发暂存的新改动。
- 临时索引用独立路径，drop/显式清理覆盖成功、Git 失败、future 取消和超时；清理本次 index 与 lock，不删除真实 index/index.lock，不终止别人的 Git 进程。评估现有 git_with_index::Command::output 的超时与取消方式，限定本次进程生命周期，不顺手重写 ACP/Coding。
- 同类检查：搜所有 add、read-tree、write-tree、update-index 及 GIT_INDEX_FILE；正常 snapshot_worktree 保持隔离。R-FIRST-07 的共享目录证据混淆单独处理，不宣称本方案解决并发任务隔离。

备选：快照失败时明确返回“证据不可用”，完全不补抓未跟踪文件。风险更小，但会改变证据能力与用户可见交付说明，需负责人选择；不允许静默声称“无改动”。

## 风险、迁移与回滚

路径规则对 util.rs 为 L2；同时它属于 AGENTS.md 明确列出的“子进程和 git 操作”高风险文件。按 FIX.md 第 0.5 节及第 4 节只写方案，未改业务代码。无数据库迁移，不改 AgentAdapter、HTTP、config.toml；回滚会重新引入索引污染，必须人工决定，不能自动 reset/clean 用户目录。

## 验收

Given 临时 Git 仓库包含 staged、unstaged、untracked、ignored 文件和已有 intent-to-add 标记，When 强制进入 fallback，Then 能收集正确 diff，真实索引的字节哈希和 git diff --cached、git ls-files --stage/debug 前后相同，工作区文件内容不变，ignored 不进入证据。

另覆盖：空仓库无索引、链接 worktree、路径含空格、Git 失败、取消/超时和两个采集并发；每次清理其私有 index/lock，不能留下本次子进程；用户同时暂存的新变更不被覆盖。先写业务断言 Red，再实现 Green，不修改现有测试断言，最后运行 FIX.md 全套检查。

## 需要人决定

是否同意采用“兜底同样使用私有临时索引”的方案，允许修改 util.rs 的相关 Git 子进程实现？推荐同意；备选为降级报告证据不可用。未明确同意前不实施。
