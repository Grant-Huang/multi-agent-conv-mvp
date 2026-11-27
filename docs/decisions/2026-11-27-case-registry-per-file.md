# 用例登记改为“一用例一文件”，过渡期 CASES.md 用 union 合并，巡检提醒冲突 PR

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 来源 | 用户确认：<br>- 用户指出“合并 queue 好像不能控制交叉冲突”，并要求“你不要自己合并，由各线合并，但是你要找一下解决方案”；<br>- 对助手提出的三项（一用例一文件、过渡期 union 合并、巡检提醒冲突 PR）答复“3 个都可以”。<br>具体格式为助手建议、用户采纳 |
| 状态 | 有效（设计已确认，待第 2 线实施） |
| 影响范围 | `docs/testing/features.json`、`docs/testing/CASES.md`、`web/verification/check-map.mjs`、`check-experience.mjs`；根目录 `.gitattributes`；派工巡检；各线登记用例的方式 |
| 相关设计 | [testing/CASE_REGISTRY.md](../testing/CASE_REGISTRY.md)（权威设计）；[fencing/INTEGRATION_PIPELINE.md](../fencing/INTEGRATION_PIPELINE.md) |

**一句话结论**：用例登记从两个大文件改为 `docs/testing/cases/` 下一用例一文件、`docs/testing/features/` 下一特性一文件，汇总表由脚本生成不入库；切换前 `CASES.md` 用 git `merge=union` 减少冲突；巡检每 4 小时在有冲突的 PR 上提醒作者所在线。审查会话不再合并任何 PR。

## 背景

六条研发线并行开 PR，几乎每个功能 PR 都要在两个公共登记文件里追加用例。

## 问题

只要一个 PR 先合并，其余所有 PR 都会冲突。各线要反复手工解冲突、重跑 CI，形成集成瓶颈。合并队列只能保证按顺序、对最新主干验证，不能消除这种冲突。

## 决策依据

1. 2026-11-27 00:25 UTC 核对：待合并 8 个 PR 中 6 个处于 `dirty`。本地试合并的冲突文件：
   - #445：`features.json`、`docs/features/conversation-memory/README.md`
   - #439：`crates/server/tests/exec_materials.rs`、`CASES.md`
   - #413、#423、#415、#408：`features.json`
2. 过去 3 天的提交数：`features.json` 126 个，`CASES.md` 128 个（`git log --since='3 days ago'`）。
3. `ai-desktop` 一个特性有 132 条用例，只按特性拆不能解决同一条线内部的冲突。
4. 读取这两个文件的程序只有 `web/verification/check-map.mjs` 与 `check-experience.mjs`，改造面小。
5. “每次改动新增一个小文件、汇总由工具生成”是多人并行追加清单的常见做法（如 Python 的 towncrier、JavaScript 的 changesets 用于变更记录）。属于业界惯例，未另行引用资料。
6. 用户意见，见“来源”。

## 定了什么

1. **新格式与迁移**：按 [CASE_REGISTRY.md](../testing/CASE_REGISTRY.md) 第 2～4 节，由第 2 线实施。
2. **过渡措施**：根目录 `.gitattributes` 设 `docs/testing/CASES.md merge=union`，切换后删除。
3. **巡检**：每 4 小时列出 `dirty` 的 PR 与冲突文件，在 PR 上留一条提醒（8 小时内不重复），不替作者合并或修改分支。
4. **合并权**：审查会话不再合并任何 PR，包括自己开的 L0 文档 PR，由各线或用户合并。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 只按特性拆文件 | `ai-desktop` 有 132 条用例，第 1 线的 PR 之间仍会冲突 |
| 只靠合并队列 | 队列只排序和验证，不消除文本冲突 |
| `features.json` 也用 union 合并 | JSON 数组按行合并会产生非法 JSON |
| 由审查会话代各线解冲突 | 用户要求由各线负责；代改别人分支容易越界 |

## 影响与后续

- 已派第 2 线实施（见决策同批 Issue）。
- 切换时同步 `AGENTS.md`、`DEV_STANDARD.md` 第 3、3.1 节（L3，需用户批准）。
- 巡检提示已更新。

## 出问题先看

- 切换后某用例“不存在”：先查 `cases/` 下的文件名与 `id` 是否一致，再查 `feature` 是否存在于 `features/`。
- union 合并后出现重复行：在 `CASES.md` 中人工删除重复的那一行。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-27 | 新建 | 用户确认 |
