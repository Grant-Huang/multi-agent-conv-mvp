# AG.4 对话里管理助手证据

日期：2026-11-17。用户批准：AG.4 方案（含修改高风险文件 `store.rs`、前台判断 schema）；开关 `[conversation] agent_management` 默认关闭，评测达标后再决定；确认凭证随卡片存进本机数据库；不做按单个任务临时指定助手。

## Red → Green

- `cargo test -p an-server --test agent_chat`：实现前 `1 passed; 7 failed`。RT-AGCHAT-01（开关关闭时不变）为守护用例；其余失败均为业务断言，例如 RT-AGCHAT-02 回复为"未知 action：manage_agents"、没有卡片。实现后 `8 passed`。
- `cargo test -p an-runtime --test agent_chat_migration`：实现前 RT-AGCHAT-08 失败（打开旧库后没有 `card_json` 列）；实现后 `1 passed`。
- 浏览器：E2E-AGCHAT、E2E-AGCHOICE 实现前失败（对话里找不到卡片）；实现后全部浏览器用例 `23 passed`。

如实记录两处测试搭建 / 文字调整（断言均未改）：
1. `agent_chat.rs` 的 Sandbox 原来只在启动服务时设置 PATH、随即恢复；而"是否已安装"在处理请求时判断，导致测试里 claude 被当成未安装。改为整条用例期间保持 PATH（用例本来就串行），用例结束时恢复。这是本次新写用例的搭建问题。
2. RT-AGCHAT-05 把模型请求序列化成 JSON 后查找卡片消息正文，正文里的英文双引号被转义而找不到。改为业务文字统一用「」（如"点「确认」后生效"），与其他提示风格一致。

## 改动

- `an-core`：`TurnDecision::ManageAgents`、`AgentOp`、`ChatCard`（发现 / 选择 / 引导 / 确认 / 结果）、`ChoiceOption`、`MessageView`（`Message` 本身不变，原有构造它的测试无需改动）。已重新生成 TS 类型。
- `store.rs`：`messages` 补 `card_json` 列（旧库自动补列）；`add_message_with_card`、`update_message_card`、`message_card`、`list_message_views`。
- `agent_chat.rs`（新）：`AgentAdmin`——候选 id、提示词里的名称对照、向导（不调用模型）、确认卡、会话内提议、确认后写回结果卡并更新原确认卡状态。
- `conversation.rs`：开关打开时 schema 加 `manage_agents` 和 `agent_op` / `agent_id` / `agent_role` 三个枚举（候选每轮生成），提示词末尾追加"管理助手"一节（不改原提示词函数签名）；`agent_id` 不在候选里按格式不合格重试。开关关闭时与以前完全相同（RT-AGCHAT-01）。
- 服务端：消息列表返回 `card`；新增 `POST /api/conversations/{id}/agent-proposals`。
- 前端：`ChatCards.tsx` 在对话里渲染五类卡片；`ChatView` 接受"卡片可选"的消息类型（原有组件测试无需改动）。
- 评测：`evals/make_agents_dataset.py` → `evals/agents.jsonl`（正例 54、反例 36）；`an-eval --agent-management`，新增 `agent_op`、`agent_id` 两项检查。已在本环境确认能解析全部 90 条（无可用模型，未评分）。

## 边界

- 卡片消息正文是大白话，送给模型的历史只用正文；卡片数据（含确认凭证）不进模型上下文（RT-AGCHAT-05）。凭证存进本机数据库，本机能访问 HTTP 接口的程序能读到，与"没有登录鉴权、只监听本机"的现有边界相同。
- 向导全部由代码完成，不调用模型；确认仍需凭证，过期返回 410 并把卡片标记为已过期（RT-AGCHAT-07）。

未验证：真实模型对 manage_agents 的识别准确率（用户本机跑 `evals/agents.jsonl`，门槛 96%）；打开开关后原有 `evals/routing.jsonl` 是否下降。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked --no-fail-fast：462 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：exit=0
web npm test：Tests 21 passed (21)
web npm run build：✓ built in 283ms
web npm run test:map：Feature map valid: 23 features, 23 browser cases
web playwright（全部浏览器用例）：23 passed
```

偶发失败记录：前一次全量运行中原有用例 `host_guard` RT-HOST-02 再次出现 `ConnectionReset`（AG.3 时也出现过一次）。单独连跑 30 次全部通过，只在全量并行负载下偶发；本次没有改动 Host 防护或 an-mom。建议另行修复这条用例对连接关闭时序的依赖。

本地结果不代替 GitHub CI。


---

# AG.4 第三轮评测后的修改（2026-11-19，DECISIONS 第 42 条）

## Red → Green

- `cargo test -p an-server --test agent_chat`：
  - 实现前：`8 passed; 2 failed`。
    - RT-AGCHAT-10：撤销时助手名仍为 `claude`；
    - RT-AGCHAT-11：提示词缺"不要反问"等规则。
  - 实现后：`10 passed`。

## 改动

- `crates/runtime/src/conversation.rs`：解析管理助手判断时，撤销 / 列出的助手名在校验候选之前清空。
- `crates/runtime/src/agent_chat.rs`：管理助手一节补四条规则（见 DECISIONS 第 42 条）。只在 `agent_management` 打开时出现，开关关闭时提示词不变（RT-AGCHAT-11 后半段检查）。

## 还没验证的

- 第二步是提示词修改，效果只能用真实模型评测证明：需要在本机重跑 `evals/agents.jsonl`，按 `evals/ROUND3_GUIDE.md` 至少两次。
- 第一步预计能解决第三轮的 3 条口径错误（und-01、und-04、und-05）。第二步针对的 10 条模型错误能改对多少，以重跑结果为准。
