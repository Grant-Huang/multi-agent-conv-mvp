# 路由评测基线（2026-11-27，#397 P1）

**用途**：Decision Harness 的共同验收基线（第 4/5 线接口约定——路由效果以路由评测集验收）。改提示词 / 换模型前后各跑一次 `an-eval` 对比本表。

| 数据集 | 样本 | 通过 | 基线通过率 | 模型 |
|---|---|---|---|---|
| `evals/routing.jsonl`（v2） | 100 | 85 | **85.0%** | 本地 Qwen3.8-35B-A3B（Q4_K_M，llama.cpp 127.0.0.1:8200），reviewer 模式标注 |
| `evals/agents.jsonl`（`--agent-management`） | 90 参评 | 84 | **93.3%** | 同上 |

**动作分布**（routing.jsonl）：respond 25 / clarify 9 / delegate 61 / follow_up 5；两数据集合计 191 条参与评判（agents.jsonl 另有条目因夹具条件跳过；最终出错样本 1，格式重试 0）。

**复现命令**（在仓库根目录，需 `config.toml` 指向可用模型）：

```bash
CARGO_TARGET_DIR=target cargo build -p an-server --bin an-eval
CARGO_TARGET_DIR=target ./target/debug/an-eval config.toml evals/routing.jsonl
CARGO_TARGET_DIR=target ./target/debug/an-eval config.toml evals/agents.jsonl --agent-management
```

原始逐条结果与汇总（含模型原始输出）在 `evals/results/2026-11-27T18-17-54-023Z-local-reviewer.summary.json`（routing）与 `…18-37-58-712Z-local-reviewer.summary.json`（agents）——该目录在 .gitignore 内，不入库；本表即基线记录。

**延迟参考**（同机）：整轮判断+路由 P50 ≈ 7～16 s、P95 ≈ 44～54 s。

**维护规则**：改动 `router.rs`/`policy.rs`/`prompts.rs` 或路由注入物（第 5 线 S4/S5）后，对比本基线；下降超过 2 个百分点需在 PR 里写明取舍或回退。补样本走 `evals/make_dataset.py` 流程（不改 jsonl 本体）。
