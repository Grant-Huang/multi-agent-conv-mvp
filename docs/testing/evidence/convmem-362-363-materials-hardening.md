# 会话资料开关取或与渲染预算加固（R-20261126-M1 / #362、R-20261126-M2 / #363）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；S 系列会话资料注入面加固（REQ-01/04 用例族） |
| 用例 | RT-RMAT-06、RT-EXEC-07（#362）；RT-JMAT-06、RT-EXEC-08（#363） |
| 来源 | 夜间评审 2026-11-26（PR #356）台账 id=R-20261126-M1 / R-20261126-M2 |
| 运行模式 | 无头 local；route / judge / exec 假模型；`cargo test -p an-server --test route_materials / judge_materials / exec_materials` |

## Red（修复前，master `300f9b7` + 仅新增测试）

```
rt_rmat_06_route_digest_without_judge_switch ... FAILED   （judge 关、route 开 → 路由无线索段）
rt_jmat_06_digest_truncates_long_title ... FAILED         （万字标题 → 清单行无界）
rt_exec_07_exec_alone_still_injects ... FAILED            （exec 单开 → 执行背景无资料块）
rt_exec_08_long_titles_bounded_by_budget ... FAILED       （万字标题×3 → 资料段爆预算）
```

## 修复

- **#362**（`conversation.rs`）：collect 条件按四个开关取或（回答 / judge / route / exec）；digest 按 `judge || route` 生成；judge 开关只控制 digest 是否进判断请求（路由线索不依赖判断开关）。
- **#363**（`session_materials.rs`）：新增 `TITLE_GIST_CHARS = 40` 标题头截（render / render_exec / render_style / 判断清单全量生效）；新增 `fit_by_dropping` 末尾整体预算兜底——按 `an_llm::estimate_tokens` 估算超限时从最低优先级段整条丢弃，**至少保留一条、绝不截断来源行**（修正首版实现会丢光条目的缺陷，保留一条语义与既有 `per_item` 地板一致）。

## Green

```
route_materials   → ok. 7 passed（含 RT-RMAT-06）
judge_materials   → ok. 5 passed（含 RT-JMAT-06）
exec_materials    → ok. 6 passed（含 RT-EXEC-07/08）
session_materials → ok. 10 passed（含 RT-SMAT-04 回归复绿）
personal_memory   → ok. 4 passed；introspection_materials → ok. 7 passed
```

SMAT-04 回归记录：首版兜底会把唯一条目也丢光（块只剩页眉、来源行消失）——已改为「至少保留一条」语义后复绿；既有断言未改。

## 边界

- fit 兜底极端场景（单条仍超预算）保留该条现状（条目级裁剪已把它压到最小形态），如实不声称严格上限。
- #362 与 #358（PR #436）同文件域，注意合入顺序（本 PR 基于 master，无堆叠）。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
