# conversation-memory 5.3：内省发现按需进入会话（确定性相关性门 + 待核实标注 + 只读）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23（America/Vancouver） |
| 来源 | **用户确认**（2026-11-23 对话四项拍板：确定性 bigram 匹配 / 待核实标注注入 / 过闸门即可候选 / 2 条独立计） |
| 状态 | **有效** |
| 影响范围 | 特性 conversation-memory（第 5 线 5.3）；`session_materials.rs`（发现段 + 相关性门）、`store.rs`（只读 list 复用）、`conversation.rs`（传当前提问）、`config.rs`（条数上限）；RT-UFND-01～05 |
| 相关设计 | REQ-conversation-memory-04；INT 基线 S0–S8（`docs/PLAN.md` 业务内省节）；决定 49（内省只读、结论可点回原始记录）；[conversation-memory/README.md](../features/conversation-memory/README.md) |

**一句话结论**：过闸门且未被人工否决的 supported/inconclusive 发现，按「当前提问 ↔ topic+hypothesis」的字符二元组 dice 系数（阈值常量）确定性匹配，取前 2 条进资料块「内省发现」段；inconclusive 强制带「待核实」标注；contradicted 不注入；注入全程只读，发现不自动晋升为任何记忆或规则。

## 决策依据

1. 用户 2026-11-23 四项拍板（对话，推荐项全中）。
2. REQ-conversation-memory-04 原文三个硬约束 → 三条验收（相关性门 / 待核实标注 / 不自动晋升）。
3. 代码事实：`introspection_findings` 表有 `verification`（supported/inconclusive/contradicted）、`gate_score`（闸门）、`verdict`（人工 confirmed/rejected）；json 含 `hypothesis` 与 cooccur 形态的 `case_ids`；`similar_feedback`（M1.2）已有 `char_grams` + `dice` 同款算法；INT 默认 `[introspection] enabled=false` → 无候选时注入自然为空。
4. 决定 49（用户确认主题）：内省只读、每条结论引用可点开的原始记录——来源行带 finding id 与证据 case id 即该语义在会话侧的延伸。

## 定了什么

1. **相关性门**：dice（字符二元组，去空白）≥ `FINDING_RELEVANCE_THRESHOLD`（常量 0.15）才候选；按分数取前 `session_materials_findings_max`（默认 2）条，独立计数不挤任务回执/记忆；不占模型槽位。
2. **候选范围**：`gate_score` 非空（过闸门，推送与否不影响）且 `verdict != "rejected"`（人工否决优先）；`contradicted` 不注入；`inconclusive` 注入并强制「待核实」标注。
3. **渲染**：资料块第三段「内省发现：」——`来源：{finding_id}（{topic} · 证据 {case_ids|template} · 待核实?）\n摘要：{hypothesis}`；预算与资料块共享逐条均分。
4. **只读**：收集与注入路径不写 `desktop_memories`、不写 findings/labels 表；RT-UFND-05 负向断言。
5. **事件**：`session.materials` 增加 `findings` 字段（来源 id）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| 模型判断相关性 | 占前台槽位、延迟与成本；确定性匹配可测可复现（用户拍板） |
| inconclusive 直接排除 | REQ 原文要求「明确标注」而非屏蔽（用户拍板标注注入） |
| 仅已推送主窗口的发现 | 按需进入会话独立于主动推送（用户拍板过闸门即可） |
| 注入时自动把发现存为个人记忆 | REQ-04 明文禁止自动晋升；只读 |

## 影响与后续

- 后续：内省页到会话的引用回链（第 1 线 UI）；5.4 组织范围（等身份）。
- adoption 形态 json 无 case_ids（INT S7 既有写入），来源行回退用 template 标注；补 case_ids 归 INT 后续卡。

## 出问题先看

- 「发现没进来」：`session.materials` 事件 `findings` 字段与 `skipped`（`not_gated` / `rejected` / `contradicted` / `not_relevant` / `over_limit`）；确认 `[introspection] enabled` 与闸门是否已跑（`gate_score` 是否非空）。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建 | 5.3 立项 |
| 2026-11-24 | 5.3b 补强：adoption json 携带证据 case_ids（RT-UFND-06） | 本决策"影响与后续" |
