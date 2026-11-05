# 路由评测说明（交给本地 Claude 执行）

把下面「评测 prompt」整段发给本地的 Claude Code，它会按步骤跑评测并写出报告。
跑之前请先把 `<…>` 里的内容换成实际值。

---

## 评测 prompt

```text
请对 AgentNexus 的"前台判断 + 路由"做一次评测，评测对象是新模型 <模型名称，例如 Qwen3.x-30B-A3B Q5_K_M>。
评测方法和要求见 evals/EVAL_PROMPT.md，请先完整阅读该文件，然后按其中的「执行步骤」进行。

背景：
- 上一次评测（旧提示词 + 27B Q5 模型，evals/routing-v1.jsonl，26 条）：动作准确率 76.9%，角色 62.5%，类别 50.0%，
  格式重试 0，出错 0，延迟 P50 35.1s / P95 84.3s（当时直接回答的内容和判断一起生成，所以很慢）。
- 这次的代码改动：提示词改为按优先级的硬规则、Turn Controller 能看到工具清单、说明了默认代码仓库；
  判断和回答拆成两步（判断只输出动作）；新增审查模式配置 [conversation] review_requests。

要求：
- 评测过程中不要修改任何代码、提示词和数据集；发现问题只记录、只提建议。
- 怀疑标注本身有问题时，单独列出，不要计入模型错误的归因。
- 报告写到 evals/reports/<日期>-<模型简称>.md，最后在对话里给我一段 10 行以内的结论。
```

---

## 执行步骤

### 0. 记录环境（写进报告开头）

- 模型文件名和量化方式；llama.cpp 版本（`llama-server --version`）和启动参数（上下文长度、`--parallel`、是否 `--jinja`）
- `config.toml` 中的 `[llm]`（`base_url`、`model`、`temperature`、`structured_mode`、`max_retries`）和 `[models]`
- 机器型号、内存
- 代码版本：`git log --oneline -1`

建议评测时把 `[llm] temperature` 设为 `0`，减少随机性带来的波动；如果保持 0.2，请在报告里注明。

### 1. 冒烟测试

```bash
curl -s http://127.0.0.1:8098/v1/models
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --limit 3 --label smoke
```

3 条都能跑完（不论对错）再继续。如果出现 `E_LLM_HTTP` 或格式重试很多，先排查 `structured_mode`（见 README「小模型调优建议」）。

### 2. 与上次对比（同一套 26 条）

```bash
cargo run --release --bin an-eval -- config.toml evals/routing-v1.jsonl --label <模型简称>-v1set
```

注意：这次的提示词也改了，所以和上次的差异 = 模型差异 + 提示词差异，报告里不要把提升全部归给模型。

### 3. 主评测（v2，90 条，三种审查模式各跑一次）

```bash
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --review-mode reviewer --label <模型简称>
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --review-mode direct   --label <模型简称>
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --review-mode auto     --label <模型简称>
```

三种模式只有审查类样本（标签 `review`）的期望不同，其余样本期望相同。

### 4. 稳定性

再跑一次 reviewer 模式，对比两次逐条结果（`evals/results/*.jsonl` 的 `pass` 字段）。两次结果不一致的样本列为"不稳定"。

### 5. 延迟（直接回答的体验）

评测工具只测"判断"和"路由"的耗时，不测生成回答。请另外在界面里手动问 3 个应直接回答的问题（例如"解释一下 Rust 的所有权机制"），记录：

- 从发送到看到第一个字的时间
- 回答全部显示完的时间
- 文字是否逐步显示（流式是否生效）

### 6. 审查回答的质量（direct / auto 模式）

在 `review_requests = "direct"` 下，在界面里实际问 `rev-01`、`rev-04`、`rev-06` 三个问题，评价直接回答的质量：是否找到了关键问题、有无明显错误、是否给出修改建议。再在 `reviewer` 模式下问同样的问题，对比独立审查的结果和耗时。

### 7. 逐条分析错误

对每个 `pass = false` 的样本，打开 `evals/results/<…>.jsonl` 中对应的那一行，看 `raw_outputs`（模型原始输出）、`got_task_goal`、`got_question`，按下表归因：

| 归因 | 含义 | 典型表现 |
|---|---|---|
| A 规则缺失或边界模糊 | 提示词没有覆盖这种情况，或规则之间界限不清 | 模型的选择在提示词里"说得通" |
| B 信息缺失 | 模型缺少做出正确判断所需的信息 | 追问一个系统其实已经知道的东西 |
| C 优先级冲突 | 同时命中多条规则，模型选了优先级低的 | 应派发却直接回答，且回答本身没错 |
| D 模型能力 | 规则清楚、信息完整，模型仍然判断错误 | 忽略明确规则、理解错用户意思 |
| E 标注争议 | 期望本身值得商榷 | 单独列出，说明理由 |
| F 系统问题 | 格式失败、超时、程序错误 | 有 `error` 字段 |

### 8. 报告模板

```markdown
# 路由评测报告：<模型名称>（<日期>）

## 结论（5 行以内）
是否建议采用；最主要的 1–2 个问题；建议的审查模式。

## 环境
（第 0 步记录的内容）

## 总体指标
| 指标 | v1 集（26 条） | v2 reviewer | v2 direct | v2 auto | 上次 27B（v1 集） |
|---|---|---|---|---|---|
| 整体通过率 | | | | | — |
| 动作准确率 | | | | | 76.9% |
| 角色准确率 | | | | | 62.5% |
| 类别准确率 | | | | | 50.0% |
| 工具准确率 | | | | | — |
| 风险准确率 | | | | | — |
| 跟进：任务 id / 类型 | | | | | — |
| 格式重试 / 出错 | | | | | 0 / 0 |
| 判断延迟 P50 / P95 | | | | | — |
| 路由延迟 P50 / P95 | | | | | — |

## 误判类型（v2 reviewer）
多派、漏派、多问、少问、漏跟进、错跟进、角色错、类别错、工具错、风险错：各几条，占比。

## 动作混淆矩阵（v2 reviewer）

## 按标签
（标签 | 通过率），重点看 regression（上次出错的 6 条）、boundary（边界样本）、follow-up、review-*。

## 逐条错误
| id | 用户消息 | 期望 | 实际 | 归因 | 依据（引用原始输出） | 建议 |

## 审查模式对比
三种模式下 review 类样本的通过率；第 6 步的质量对比；推荐哪种模式及理由。

## 稳定性
两次结果不一致的样本。

## 延迟与体验
第 5 步的记录。

## 标注争议
## 改进建议（按收益排序，只提建议，不改代码）
```

---

## 参考：达标线（建议值，可按需要调整）

| 指标 | 建议达标线 | 说明 |
|---|---|---|
| 动作准确率 | ≥ 90% | |
| 漏派（under_delegate） | ≤ 3% | 该交给后台却直接回答：可能给出编造的时间、计算结果 |
| 多派（over_delegate） | ≤ 5% | 不该派却派了：多花时间，但结果通常没错 |
| 多问（over_clarify） | ≤ 5% | 不必要的追问影响体验 |
| 角色准确率 | ≥ 90% | |
| 跟进任务 id | 100% | 跟进错任务会取消或改错别的任务，代价很高 |
| 风险：high 样本 | 100% 判为 high | 漏判会绕过人工审批 |
| 格式最终出错 | 0 | |
| 判断延迟 P95 | ≤ 3 秒 | 判断只输出动作，应该很快 |

## 参考：评测集构成（v2，90 条）

| 类别 | 标签 | 条数 | 期望 |
|---|---|---|---|
| 闲聊、常识、建议、示例代码 | chat / knowledge / advice / snippet | 18 | respond |
| 缺少关键信息 | clarify | 9 | clarify |
| 计算、时间 | calc / time | 9 | delegate → general（calculator / current_time） |
| 文件与文档 | file / doc | 12 | delegate → general，L2 / L3 / L4 |
| 改代码仓库 | coding | 12 | delegate → coding |
| 高风险 | risk | 4 | delegate，风险 high |
| 审查请求 | review-short / review-long / review-file / review-repo | 12 | 随审查模式变化 |
| 跟进进行中的任务 | follow-up | 8 | follow_up（5 条）/ 无关新请求（3 条） |
| 多轮对话 | multi-turn | 6 | 结合上下文 |

其中 `regression` 标签是上次 27B 出错的 6 条，`boundary` 标签是容易混淆的边界样本。
