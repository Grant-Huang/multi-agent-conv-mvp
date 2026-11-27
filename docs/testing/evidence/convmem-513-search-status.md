# search_history 状态条件修复（R-20261127-M1 / #513）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory；REQ-conversation-memory-05 用例族 |
| 用例 | RT-L2H-01（补任务回执断言） |
| 来源 | 夜间评审 2026-11-27（PR #497）台账 id=R-20261127-M1（已复现：`stored status="COMPLETED" hits=0`） |
| 运行模式 | 无头 local；`cargo test -p an-server --test history_search` |

## 缺陷（已复现）

`search_history` 回执查询条件写 `status = 'completed'`（小写字面量），库里存的是 `TaskStatus::Completed.as_str()` = `"COMPLETED"`（大写）——任务回执检索永远为空。#401 承诺的「任务回执」检索结果缺失，RT-L2H-01 无回执断言（漏测）。

## 修复

状态条件改为绑定参数 `?4 = TaskStatus::Completed.as_str()`，不依赖字面量大小写。

## Red→Green

Red（修复前 + 仅新增断言）：rt_l2h_01 补回执断言后 FAILED——`任务回执应命中` 但搜索结果无回执条目。

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search rt_l2h_01
（修复前 stash 口径：FAILED——状态条件小写不命中）
```

Green（修复后）：

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test history_search
test result: ok. 6 passed; 0 failed; ... finished in 0.05s
```

（含新增断言：COMPLETED 任务回执命中 kind=任务回执、msg_id=task-l2h-01。）

## 门禁

fmt / clippy（-D warnings）/ 全量 --no-fail-fast：pass（全量见 PR 评论）
