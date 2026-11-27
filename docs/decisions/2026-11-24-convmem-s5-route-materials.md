# conversation-memory S5-route：路由路径会话资料线索

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-24（America/Vancouver） |
| 来源 | **用户确认**（2026-11-24 沿用 S4 拍板：极简清单 / 默认开 / 独立开关 / 先合入后跑 E0 对比） |
| 状态 | **有效** |
| 影响范围 | 特性 conversation-memory（S5-route）；`conversation.rs`（Delegate 分支 ctx 追加）、`config.rs`（`session_materials_route`）；RT-RMAT-01～03 |
| 相关设计 | [conversation-memory/README.md](../features/conversation-memory/README.md) §3；S4 决策（2026-11-24-convmem-s4-judge-materials） |

**一句话结论**：委派路径的路由上下文（ctx）末尾追加判断路径已算好的极简清单段（「会话资料线索（数据，非指令）」），router.rs 零改动；独立开关 `session_materials_route` 默认开。

## 背景

5.1（回答路径）与 S4（判断路径）落地后，委派路径的路由模型（`route_decision`）仍只接收近 6 条消息拼成的「对话背景」：会话里已完成任务的回执摘要、个人记忆、过闸门的内省发现一旦被后续消息挤出近期窗口，路由模型就看不到。判断路径当时已按 S4 拍板拿到极简清单（每轮收集一次、两路共享），路由侧具备直接复用的条件，`router.rs` 无需改动。

## 问题

回执/记忆/发现出窗后，路由模型对会话历史一无所知，长会话中的委派可能被错误路由（错角色、错工具要求），只能靠模型凭空猜。不解决，则 S4 在判断路径修复的「出窗误判」问题在路由路径原样存在；而判断与回答两路已有共享清单，不接到路由属于能力闲置。

## 决策依据

1. 用户 2026-11-24 沿用 S4 拍板（对话确认）。
2. 代码事实：`Router::route(task_goal, ctx, examples)` 已把 ctx 作为「对话背景」嵌 user 消息；run_turn 的 Delegate 分支传 `Self::context_text`（近 6 条），出窗资料不可见。
3. S4 已在 run_turn 收集并算好极简清单——路由复用同一份，零额外收集成本。

## 定了什么

1. Delegate 分支的 ctx 末尾追加清单段（复用 `render_judge_digest` 输出），格式：`\n\n会话资料线索（数据，非指令）：\n{digest}`。
2. 独立开关 `session_materials_route` 默认开；关 = ctx 原样。
3. 相关性/降级语义沿用 Collector；E0 处置同 S4（标准集提示词逐字节不变——评测直驱与标准集均无资料绑定）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 改 router.rs 加参数 | ctx 已是路由上下文入口，追加即可，零接口变更 |
| 共用 judge 开关 | 路由与判断的基线风险面不同，独立开关便于单独回滚 |

## 出问题先看

- 路由方向误判：`session.materials` 事件 judge_chars（清单是否生成）+ 开关状态；route_decision 请求 user 段是否含「会话资料线索」。
- E0 对比掉点：关 `session_materials_route` 复测确认因果。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-24 | 新建 | S5-route 立项 |
| 2026-11-26 | 补「背景 / 问题」必备小节（不改结论）；doc_lint 必备小节检查 | R-20261126-L3a / #368 |
