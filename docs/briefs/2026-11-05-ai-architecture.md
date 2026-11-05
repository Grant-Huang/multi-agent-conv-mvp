# 2026-11-05：近 24 小时的 Agent 讨论，以及对 AgentNexus 的借鉴

检索时间：2026-11-05。搜索用默认的 Exa（未传 `--provider`），每条查询 3 个结果。正文用本机抓取，失败再走 Jina。下面只写这次实际返回的页面和摘要，页面没打开成功的条目单独标出。

## 今天和架构直接相关的几篇

### 调度员留下，工人在后台跑

[LangChain：Agents Are Going Async](https://www.langchain.com/blog/agents-are-going-async)（2026-11-05，本机抓取成功）

异步子代理把系统拆成两半：编排者留在用户面前，保持对话上下文；子代理在后台做长任务，做完再汇报。工具如果一直堵住这一轮，能同时做的事就受限于一轮循环的长度。后台工作量则可以随工人数量增加。文中把 Claude Code 和 OpenAI 的个人代理都算进这个趋势。

和本项目的对应：Conversation 已经是唯一对用户说话的入口，coding / general 在后台跑，结果再交回来。这篇把「后台工人做完再汇报」写成了产品形态。还没做的是一个会话里同时挂多个长任务，并在工人结束时把结果送回同一次对话，而不是各开各的会话。

### 派活写成程序，不靠模型自觉

[少数派：深度拆解 DeepSeek Super-Agent](https://sspai.com/post/118524)（文中日期 2026-11-05，本机抓取成功）

这是第三方阅读仓库和提示词后的拆解，不是 DeepSeek 官方说明。文中写：README 标明它是研究原型。作者的判断是，它把原本靠模型自觉的规矩挪进程序：派活、筛消息、记状态、发现卡住，都由程序做；模型只判断这条消息接不接。文中称，中性提示下单个 Session 在 5 轮里没有一次主动叫子代理；明确鼓励后 5 轮里有 4 轮才用上。Router 用轻量模型，Worker 用强模型，每次判断只给截断后的消息、有限工单和有限上下文。

和本项目的对应：这和「模型负责理解，程序负责决定」是同一方向。前台判断已经是结构化 JSON，能不能执行由策略决定。不必照搬它的飞书和 GitHub 工单。可以记住的是：分派不能指望模型自己想起要调用子代理。

### 沙箱是按需资源，状态不放在运行时里

[Pulse 笔记：别再给每个 Agent 塞一个 Sandbox](https://genai-insight.com/pulse/notes/2026-11-05-next-gen-agent-architecture-beyond-sandbox/)（2026-11-05，本机抓到的是笔记，不是文中所说的 1.5 万字原文）

笔记摘要的拆分是：Runtime 只算下一步，不碰数据库和网络；Gateway 统一模型协议和凭证；Queue 负责顺序、租约、重试、取消；数据库是事实来源；Sandbox 懒分配，不需要命令行的任务就不给。凭证不进入 Runtime。提出操作的 Agent 不能当自己的最终裁判。

和本项目的对应：任务状态已经在 SQLite，不在 agent 进程里。审查和执行分开，也符合「执行者不当自己的裁判」。节点负责本机权限，凭证不必进模型上下文。Queue 里的租约和死信，本项目还没有；当前单任务派发用不上，任务拆分时再看。

### 编排文档仍然是同一道选择题

[OpenAI：Orchestration and handoffs](https://developers.openai.com/api/docs/guides/agents/orchestration)（本机抓取成功，页面无「今天」的日期）

Handoff 把对话交给专家。Agents as tools 让管理者留下，专家只是一次有边界的调用。同一查询还返回了 Agents SDK 的另一页，以及一个小仓库的 handoff 基准：小模型如果工具调用格式不稳，路由就会差。那个基准页的本机正文里混有 GitHub 导航。

和本项目的对应：已经选了 agents as tools。路由评测继续用固定题集，不改成「让模型自己决定要不要交接对话」。

## 同日出现、但不改当前设计的

- [Google Cloud：用 AlphaEvolve 和 Harbor 优化 agent skill](https://cloud.google.com/blog/topics/developers-practitioners/how-to-build-an-agent-skill-optimizer-with-alphaevolve-and-harbor)（2026-11-05）。讲的是给编码 agent 的技能说明做自动评估。本项目的能力边界在配置和评测集里，不靠自动改技能文本。
- [CEREBRO 2026-11-05](https://stevengonsalvez.com/cerebro/2026-11-05)。当日信号汇总。其中一条是 Claude Code 的后台子代理不再每 30 秒重读整段对话来刷新状态行，改由短笔记生成，单次成本大约降到原来的 1%。这和本项目把进度事件节流、不让进度去拉整份任务列表是同一类成本问题。
- [PaperDog：Prime Agent](https://paperdog.org/blog/papers-prime-agent)。搜索摘要写了权重 / 上下文 / 持久 REPL / 磁盘的分层，以及 ARC-AGI-3 上的数字。本机和 Jina 都返回 HTTP 500，正文没有读到，数字不采用。

## 这次不采纳的做法

公共 SearXNG、本机浏览器抓取、把 `r.jina.ai` 当成第二个搜索源，都没有进这次运行。Exa 没有结果时不会自动改走 DuckDuckGo。
