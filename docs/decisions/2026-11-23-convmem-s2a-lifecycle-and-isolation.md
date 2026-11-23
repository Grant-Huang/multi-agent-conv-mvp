# conversation-memory 5.2a：个人记忆生命周期与隔离（POST 端点、kind 补列、夹具头口径）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23（America/Vancouver） |
| 来源 | **用户确认**（2026-11-23 对话四项拍板：范围切 5.2a 先行；批准新增 POST 端点与 kind 补列；隔离用 `x-desktop-principal` 夹具头口径；决定 48 红线在 5.2b 落地为断言） |
| 状态 | **有效** |
| 影响范围 | 特性 conversation-memory（第 5 线 5.2a）；`store.rs`（补列迁移 + create）、`desktop_api.rs`（POST + 作用域）、`session_materials.rs`（记忆段）、`conversation.rs` / `lib.rs`（说话人传播）；RT-UMEM-01～05 |
| 相关设计 | [conversation-memory/README.md](../features/conversation-memory/README.md) 第 2、3、4 节；决定 45/46/47（DECISIONS.md 第九节）；[ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) §5 memories |

**一句话结论**：5.2 先做「生命周期与隔离」半张卡——用户主动保存走新增 `POST /api/desktop/v1/memories`，`desktop_memories` 补 `kind` 列（fact 默认，preference/style 为 5.2b 预留），记忆与注入按 `x-desktop-principal` 夹具头作用域（无头回退 `local`，不动 evidence 的 `alice` 契约），过期不入注入；预览与下一会话生效闸门留 5.2b。

## 决策依据

1. 用户 2026-11-23 四项拍板（对话记录，选项均为推荐项被选中）。
2. 决定 45/47（用户确认）：个人记忆能看能改、画像存记忆层加 preference/style 类型——`kind` 列即该类型的落地。
3. 代码事实：T-78 后记忆只有种子 + PATCH/DELETE，**无保存路径**；`expires` 存了但无任何执行；`principal_from` 无头默认 `alice`（evidence ACL 契约，不能动）；chat 与 B1 受理是 `handle_message` 仅有的两个生产调用点。
4. 决定 46（用户确认部分）：预览→确认→下一会话生效；本卡不实现闸门，只预留 kind，避免无 UI 入口时造半套闸门。

## 定了什么

1. **范围**：5.2a（本卡）= 保存 / 纠正 / 撤回 / 隔离 / 过期不入注入；5.2b（另卡）= preference/style 预览、确认、下一会话生效、生效前撤销、风格注入 ≤200 token、决定 48 红线断言。
2. **接口**：新增 `POST /api/desktop/v1/memories`（title/body 必填，expires 缺省「长期」，kind 缺省 fact，非法 kind → 400）；公开接口变更理由：用户主动保存没有写路径（T-71/T-72 只有种子 + 纠错 + 撤回）。
3. **迁移**：`desktop_memories` 补 `kind TEXT NOT NULL DEFAULT 'fact'`，沿 store.rs `pragma_table_info` + `ALTER TABLE` 先例；SCHEMA 同步加列；无破坏性 DDL。
4. **隔离口径**：`principal_or_local`——记忆 CRUD、chat 回答注入、B1 受理分流按 `x-desktop-principal` 夹具头，无头回退 `LOCAL_USER`（`local`）；evidence ACL 的 `principal_from`（无头 `alice`）**不动**。
5. **注入语义**：本人未撤回 + 未过期 + `kind = fact` 的记忆进回答资料块「个人记忆」段（含标题与有效期来源行）；过期判定只认可解析的 ISO 日期前缀早于今天，「长期」等自由文本永不过期；列表契约（T-71 断言）不动。预算改为**逐条均分截断**——来源行永不截断（来源可查优先于摘要完整，RT-SMAT-04 期间发现整块截断会切碎来源行，按此修订）。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| a+b 一张卡 | 预览/生效闸门依赖第 1 线 UI 入口，本卡不依赖即可交付 |
| 无头默认 `alice` | 破坏 T-78 既有 `local` 契约与 RT-ADMEM 断言 |
| 注入侧按会话读任务回执方式读记忆（窗口化） | 记忆是长期资料，不该被消息窗口挤出；按用户作用域全量（上限 3 条） |
| 列表过滤过期条 | 改动 T-71 冻结契约；5.2a 只在注入侧过滤 |
| POST 让客户端提供 source/id | 来源是溯源性字段，服务端生成「用户保存 · 日期」 |

## 影响与后续

- 已落地：RT-UMEM-01～05（CASES「个人记忆生命周期与隔离」）；`features.json` REQ-02 → 已确认。
- 后续：5.2b 立项需用户确认预览/生效细则（决定 46 助手建议部分 + 48 红线）；保存入口 UI 归第 1 线，到位后补 E2E。

## 出问题先看

- 「我存的记忆没生效」：先看 `session.materials` 事件的 `skipped`（`expired` / `not_fact` / `over_limit`），再查说话人 principal 是否一致（夹具头 vs 无头）。
- 旧库升级：`pragma_table_info('desktop_memories')` 查 `kind` 列；迁移在 `Store::open` 内。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建 | 5.2a 立项与实现 |
