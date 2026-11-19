# 07 模型与后端配置、路由评测

适合管理员/开发者。所有配置在 `config.toml`（模板 `config.example.toml`，字段写错会有 `W_CONFIG_UNKNOWN_KEY` 提示）。

## 场景 A：对接本地 llama.cpp（最简）

```toml
[llm]
base_url = "http://127.0.0.1:8098/v1"
model = "local"
structured_mode = "json_schema"   # 推荐；不支持时 json_object，再不行 prompt_only
max_retries = 2                   # 输出不合格时把错误反馈给模型重试的次数
```

## 场景 B：不同角色用不同模型

```toml
[models]
conversation = "qwen3-4b"   # 前台“这一轮做什么”，输出很短，适合小而快
reply = "qwen3-8b"          # 直接回答用户
router = "qwen3-1.7b"
general = "qwen3-8b"
reviewer = "qwen3-8b"
```

不同角色在不同端口的实例上：用 `[models.base_urls]`。

## 场景 C：多后端与槽位（推荐的进阶写法）

写了 `[backends.*]` 就不能再用 `[models.base_urls]`。

- **方案 A 全本地**：一个后端开多槽位（`llama-server -np 6`），`interactive_slots = [0,1]` 给前台，`background_slots = [2,3,4,5]` 给后台任务，`[routing]` 全指向它。
- **方案 B 前台本地、后台云端**：本地后端只给前台（`background_slots = []`），云端后端 `kind = "openai"`，`api_key_env = "CLOUD_API_KEY"`（**密钥只从环境变量读**），用 `max_inflight`、`rpm`、`tpm`、`reserved_interactive` 限流，`[routing]` 里 `general`/`reviewer` 指向云端。

分段超时：`connect_secs` / `first_token_secs` / `idle_secs` / `total_secs`；排队上限 `queue_secs`（前台默认 30 秒、后台 600 秒）。

> 云端后端属于“非本机地址”，若开启 `[egress]` 的 `enforce`，必须登记，见 [05](05-research-mcp-egress.md)。

## 场景 D：前台提速（可选实验开关，默认关）

| 开关 | 位置 | 作用 |
|---|---|---|
| `front_structured_thinking` | `[backends.<名>]` | 关闭前台结构化请求的“思考” |
| `front_reason_first` | 同上 | 先写 ≤40 字依据再给结论 |
| `front_thinking_escalation` | 同上 | 格式不合格/风险 high/角色无可用 agent 时，开思考重判一次 |
| `risk_rules` | `[conversation]` | 规则判风险，只升不降 |
| `route_class_questions` | `[conversation]` | 路由回答两个是/否问题，类别由代码算出（本机实验未通过，见 DECISIONS） |
| `route_class_from_tools` | `[conversation]` | general 任务类别由工具清单算出 |

不写这些配置时，请求与原来完全一致。真实模型下的准确率与延迟需在你本机用评测验证。

## 场景 E：评测路由效果（换模型后对比）

```bash
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --review-mode reviewer --label 模型简称
```

只跑“前台判断 + 路由分类”，**不执行任务**。输出整体通过率，动作/角色/类别/工具/风险/跟进准确率，混淆矩阵，误判类型（多派、漏派、多问…），按标签通过率与延迟。结果写入 `evals/results/`。

| 选项 | 说明 |
|---|---|
| `--review-mode reviewer\|direct\|auto` | 对应 `[conversation].review_requests` |
| `--tag 标签` | 只跑某类样本 |
| `--limit N` | 限制条数 |
| `--label 名称` | 结果标注，便于对比 |
| `--variant E4` | 临时套用前台提速实验变体，不必改配置 |

数据集：`evals/routing.jsonl`（90 条，由 `evals/make_dataset.py` 生成；补样本请改脚本）、`evals/routing-v1.jsonl`（旧基线 26 条）。完整步骤与报告模板见 `evals/EVAL_PROMPT.md`。

## 场景 F：任务相关参数

| 参数 | 默认 | 作用 |
|---|---|---|
| `[orchestrator].max_review_rounds` | 1 | 审查不通过后最多返工几轮 |
| `[orchestrator].history_messages` | 12 | 每轮带最近多少条消息 |
| `[orchestrator].task_timeout_secs` | 1800 | 单任务超时（排队不算） |
| `[orchestrator].approval_risk` | high | 达到该风险等级需人工审批 |
| `[agents.general].max_concurrent` | 4 | 同时执行任务数 |
| `[agents.general].max_steps` | 6 | 工具循环最大步数 |
| `[conversation].review_requests` | reviewer | “审查/评估”类请求：reviewer 一律交独立审查；direct 对话内直接答；auto 短内容直接答、长内容交后台 |
