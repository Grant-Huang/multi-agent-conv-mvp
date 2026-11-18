# 第三轮评测指南（AG.4 助手管理 + FT 前台提速）

本文件交给执行评测的 agent 使用：前半是**执行步骤**，后半是**结果评估方法**（怎么判定有效、怎么归因、怎么下结论）。
前两轮的结果和问题见下文「背景」。

---

## 交给 agent 的提示词

把下面整段发给执行评测的 agent（Claude Code 等），先把 `<…>` 换成实际值。

```text
请对 AgentNexus 做第三轮评测（AG.4 助手管理 + FT 前台提速），评测对象是本机模型 <模型文件名>。
评测方法和判定口径见 evals/ROUND3_GUIDE.md，请先完整阅读该文件，然后按「执行步骤」和「结果评估」进行。

要求：
- 评测过程中不要修改任何代码、提示词、配置默认值和数据集；发现问题只记录、只提建议。
- 一次只跑一个评测，评测期间这台机器上不要有别的程序使用同一个模型服务。
- 每次运行都把 stderr 保存成日志文件（指南里给了命令），出现超时要按指南查明原因。
- 原始数字必须如实报告；归因后的"调整后"数字只能放在原始数字旁边作补充，不能替代。
- 报告写到 evals/reports/<日期>-round3.md，并把 evals/results/ 下本轮生成的 .jsonl 和 .summary.json 一并交给我；
  最后在对话里给我一段 10 行以内的结论，逐条回答指南「要回答的问题」里的 Q1～Q4。
```

---

## 背景（前两轮）

| 轮次 | 内容 | 结果 |
|---|---|---|
| 第一轮（FT.3，2026-11-17） | `routing.jsonl` 100 条，E0～E5 各一次 | E0 78%，E1 76%；E1 整轮 P95 11.4 秒；单次结果差距在误差内 |
| 第二轮（FT.3b + AG.4，2026-11-17～18） | E0 两次、E1 一次；`agents.jsonl` 一次；`routing.jsonl` 开 / 关助手管理各一次 | E0 81% / 80%，E1 76%（P95 5.1 秒）；`agents.jsonl` 75/90（83.3%，门槛 96%）；routing 开 86% / 关 84% |

第二轮暴露的问题，本轮要查清或复核：

1. **150 秒超时**：`agents.jsonl` 6 条、E0 第二次 5 条报 `E_LLM_TIMEOUT`，提示"等待 0 秒仍未收到第一个 token"。
   这里的"0 秒"是排队之后剩下的等待时间，不是实际等了多久。时间实际花在排队等模型服务的位置上（`crates/llm/src/lib.rs`：排队时间也计入调用期限）。
   怀疑上一条样本的生成没有停，占住了位置，但没有证据。
2. **撤销时多填助手名**：und-01～05 共 5 条判对了"撤销"，但 `agent_id` 填了某个助手。运行时撤销不看这个字段（`crates/runtime/src/agent_chat.rs` 中 `AgentOp::Undo => ChangeRequest::Revert`），按现行口径仍算错。
3. **同一配置两次结果差别大**：E0 两次之间有 15 条结果不同（去掉超时后 11 条）。

---

## 要回答的问题

- **Q1（AG.4）**：在当前代码上，`evals/agents.jsonl` 能否达到 ≥ 96%（90 条最多错 3 条）？没达到时，错误里环境、口径、模型各占多少？
- **Q2（AG.4）**：打开 `agent_management` 后，`evals/routing.jsonl` 的准确率是否低于关闭时？
- **Q3（FT）**：关思考（E1）比开思考（E0）低多少？E1 的整轮 P95 是否稳定 ≤ 6 秒？关思考后稳定翻错的是哪些样本？
- **Q4（环境）**：150 秒超时的直接原因是什么（排队多久、被谁占住）？

---

## 执行步骤

### 0. 准备并记录环境（写进报告开头）

```bash
git fetch origin && git checkout claude/serene-cerf-lpx1bs && git pull origin claude/serene-cerf-lpx1bs
```

```bash
git log --oneline -1
```

```bash
cargo build --release --bin an-eval
```

报告开头要记录：

- 代码版本（上面 `git log` 的输出）。
- 模型文件名和量化方式；推理服务版本（`llama-server --version`）和启动参数（上下文长度、`--parallel`、是否 `--jinja`）。
- `config.toml` 中的 `[llm]`（`base_url`、`model`、`temperature`、`structured_mode`、`max_retries`、`timeout_secs`）、`[models]`，以及后端的 `queue_secs`、`deadline_secs`、`first_token_secs` 等超时设置（没写就记"默认"）。
- 机器型号、内存；评测期间是否有其他程序在用模型服务（要求：没有）。
- `temperature` 的取值。前两轮沿用用户配置，本轮**保持不变**，不要为了评测改成 0，否则和前两轮不可比。

### 1. 每次运行前

1. **重启模型服务**，清掉上一次可能残留的生成。
2. 预热：用 `--limit 1` 跑一条，结果不计入。
3. 运行命令统一带上日志：`RUST_LOG=warn` 会把失败的模型调用（事件 `llm.call`，含 `queue_wait_ms`、`ttft_ms`、`outcome`）写到 stderr。

### 2. 运行清单（按顺序；开 / 关交替，抵消机器状态的漂移）

下面 `<cfg>` 是用户的 `config.toml` 路径。每条命令跑完记下结果文件名（`evals/results/` 下按时间命名）。

| 序号 | 目的 | 命令 | 预计耗时 |
|---|---|---|---|
| R1 | Q1 | `RUST_LOG=warn cargo run --release --bin an-eval -- <cfg> evals/agents.jsonl --agent-management --label R3-AG4-a 2> evals/results/R3-AG4-a.log` | 约 20 分钟 |
| R2 | Q2 | `RUST_LOG=warn cargo run --release --bin an-eval -- <cfg> evals/routing.jsonl --agent-management --label R3-on-a 2> evals/results/R3-on-a.log` | 约 30 分钟 |
| R3 | Q2 | `RUST_LOG=warn cargo run --release --bin an-eval -- <cfg> evals/routing.jsonl --label R3-off-a 2> evals/results/R3-off-a.log` | 约 30 分钟 |
| R4 | Q1 | 同 R1，`--label R3-AG4-b`，日志 `R3-AG4-b.log` | 约 20 分钟 |
| R5 | Q2 | 同 R2，`--label R3-on-b`，日志 `R3-on-b.log` | 约 30 分钟 |
| R6 | Q2 | 同 R3，`--label R3-off-b`，日志 `R3-off-b.log` | 约 30 分钟 |
| R7 | Q3 | `RUST_LOG=warn cargo run --release --bin an-eval -- <cfg> evals/routing.jsonl --variant E1 --label R3-E1-a 2> evals/results/R3-E1-a.log` | 约 10 分钟 |
| R8 | Q3 | `RUST_LOG=warn cargo run --release --bin an-eval -- <cfg> evals/routing.jsonl --variant E0 --label R3-E0-a 2> evals/results/R3-E0-a.log` | 约 30 分钟 |
| R9 | Q3 | 同 R7，`--label R3-E1-b`，日志 `R3-E1-b.log` | 约 10 分钟 |

合计约 3.5 小时。时间不够时，优先级是 R1、R4（Q1），然后 R7、R8、R9（Q3），最后 R2、R3、R5、R6（Q2）。

说明：
- R1～R6 不带 `--variant`，按用户配置运行，与第二轮的 AG4 系列可比。R7～R9 用 `--variant`，与 FT 系列可比。两组不要混在一起比较。
- 每条命令最后都会打印一行汇总；`.summary.json` 和 `.jsonl` 会写到 `evals/results/`。

### 3. 每次运行后立即做有效性检查

- 看 `.summary.json` 里的 `failed`（出错条数，也就是模型没有给出判断的条数）：
  - `failed` ≤ 2：有效。
  - `failed` ≥ 3：**这次运行无效**。先按第 4 步查原因，再重启模型服务重跑这一项。报告里保留无效那次的记录和原因。
- 检查 `failed` 时同时记下这次的整轮 P50 / P95。

### 4. 出现超时时查原因（Q4）

```bash
grep 'llm.call' evals/results/R3-AG4-a.log | grep -i timeout
```

对每条超时记录：`queue_wait_ms`（排队等了多少毫秒）、`ttft_ms`（首个 token 用时）、`slot_id`、`outcome`。判断方法：

- `queue_wait_ms` 接近整次期限（例如 100 秒以上）：时间花在排队上，说明位置被占。再看**上一条样本**的日志：
  - 上一条也是超时或被取消，基本可以确定是"上一条的生成没有停，占住了位置"。
  - 同时看推理服务自己的日志，确认那段时间有没有一个长时间生成的请求。
- `queue_wait_ms` 很小、`ttft_ms` 很大：时间花在预填充或推理服务本身变慢，记下当时的提示词长度（`prompt_tokens`）。

只记录现象和证据，不要改代码或配置去规避。

---

## 结果评估

### 1. 先算原始数字（以 `.summary.json` 为准）

每次运行报告：整体准确率、动作准确率、`failed`、整轮 P50 / P95；`agents.jsonl` 另报 `agent_op`、`agent_id`；`routing.jsonl` 另报角色、类别、风险。

### 2. 再给每条失败归因

逐条看 `.jsonl` 中 `pass` 为 false 的样本，分成三类，**一条只归一类**：

| 归因 | 判定方法 | 说明 |
|---|---|---|
| 环境 | `error` 不为空（如 `E_LLM_TIMEOUT`） | 模型没有给出判断，不能说明它判得对不对 |
| 口径 | 仅限 `agents.jsonl`：期望操作是 `undo` 或 `list`，模型给出的动作和操作都对，只是 `agent_id` 非空 | 运行时撤销 / 列出不使用 `agent_id`，行为不受影响；但现行口径算错 |
| 模型 | 其余 | 真正判错 |

怀疑某条标注本身有问题（期望值可以商量）时，在报告里单独列出并说明理由，但**仍按"模型"计数**，不擅自改口径。

报告里给出两种数字：
- **原始准确率**：按现行口径，如实报告。
- **调整后准确率**：去掉"环境"类后的分母上计算 `模型以外都算对` 的比例，即 `(原始正确数 + 口径类条数) / (总数 − 环境类条数)`。只作参考，用来回答"如果没有超时、口径统一后离门槛还差多少"。

### 3. 判定

| 问题 | 判定规则 | 来源 |
|---|---|---|
| Q1 达标 | `agents.jsonl` **每一次有效运行**的原始准确率都 ≥ 96%（最多错 3 条） | 用户 2026-11-16 确认的门槛 |
| Q2 不下降 | 打开时两次的平均整体准确率 ≥ 关闭时两次的平均，或者差距不超过 2 个百分点**且**没有"打开两次都错、关闭两次都对"的样本 | 原规则是"不得低于打开前"；"2 个百分点 + 没有稳定翻错"是对单次波动的解释口径，**结论里要注明用的是这个口径，交用户确认** |
| Q3 达标 | E1 的平均整体准确率不低于 E0 的平均，且 E1 每次整轮 P95 ≤ 6 秒 | `scripts/ft_experiment.py` 中用户 2026-11-17 确认的验收标准 |

计算平均时，把本轮和前两轮同一配置的有效结果合在一起，同时给出只用本轮的数字。前两轮的数字在「背景」一节；E0 第二轮第二次有 5 条超时，归为有效但需注明。

### 4. 理解波动（下结论前必读）

- 100 条样本、准确率约 80% 时，单次结果的抽样误差约 ±4 个百分点（√(0.8×0.2/100) ≈ 4%）。**两组差 2～3 个百分点不能说明谁更好**，要看多次平均和逐条稳定性。
- 逐条稳定性：同一配置多次运行中**每次都对**的叫"稳定对"，**每次都错**的叫"稳定错"，其余叫"不稳定"。
- 关思考的真实损失 = "E0 稳定对且 E1 稳定错"的样本。第二轮找到 7 条：
  - 风险判低 3 条：doc-04、doc-06、art-01；
  - 类别 2 条：calc-05、doc-07；
  - 该反问却直接回答 1 条：clar-06；
  - 角色 1 条：rev-repo-01。

  另有 mq-mix-01、rev-file-02 两条：E0 一次对、一次超时，E1 错；按脚本口径（超时视为无数据）也算翻错，合计 9 条。
  本轮复核这些样本是否仍稳定翻错，并列出新增的。
- 三种配置都稳定错的样本（第二轮有 snip-01、snip-02、calc-03、file-04、file-05、risk-02～04、rev-02、rev-05）与开不开思考无关，单独列出，供以后改提示词或复核标注。

### 5. 辅助分析脚本

脚本只读结果文件，不改任何东西。把下面内容存到临时目录（不要放进仓库），用 `python3 -I` 运行：

```python
#!/usr/bin/env python3
"""第三轮评测辅助分析：每次运行的概况、失败归因、同组多次运行的逐条稳定性、两组之间的稳定翻错。
用法：python3 -I analyze.py 组名=文件1.jsonl,文件2.jsonl 组名=文件3.jsonl ...
例如：python3 -I analyze.py E0=a.jsonl,b.jsonl E1=c.jsonl,d.jsonl
结果以 .summary.json 为准，本脚本的百分位算法可能与 an-eval 略有差别。"""
import json
import sys


def load(path):
    """读取一次运行的逐条结果，按样本 id 建索引"""
    with open(path, encoding="utf-8") as f:
        return {r["id"]: r for r in (json.loads(line) for line in f if line.strip())}


def cause(r):
    """失败归因：环境 / 口径 / 模型（规则见指南「结果评估」第 2 节）"""
    if r.get("error"):
        return "环境"
    exp = r.get("expect") or {}
    if (
        exp.get("expect_agent_op") in ("undo", "list")
        and r.get("got_action") == "manage_agents"
        and r.get("checks", {}).get("agent_op") is True
        and r.get("checks", {}).get("agent_id") is False
    ):
        return "口径"
    return "模型"


def p(values, q):
    """简单百分位（最近秩）"""
    v = sorted(values)
    return v[min(len(v) - 1, int(len(v) * q))] if v else 0


groups = {}
for arg in sys.argv[1:]:
    name, files = arg.split("=", 1)
    groups[name] = [(f, load(f)) for f in files.split(",")]

for name, runs in groups.items():
    print(f"== 组 {name}")
    for path, rs in runs:
        fails = [r for r in rs.values() if not r["pass"]]
        by = {}
        for r in fails:
            by.setdefault(cause(r), []).append(r["id"])
        ok, n = len(rs) - len(fails), len(rs)
        env, rule = len(by.get("环境", [])), len(by.get("口径", []))
        adj = (ok + rule) / (n - env) * 100 if n > env else 0
        t = [r["turn_ms"] for r in rs.values()]
        print(f"  {path}: 原始 {ok}/{n} = {ok / n * 100:.1f}%，调整后 {adj:.1f}%，"
              f"整轮 P50 {p(t, .5)} / P95 {p(t, .95)} ms")
        for c, ids in by.items():
            print(f"    {c} {len(ids)}: {' '.join(ids)}")
    # 逐条稳定性：环境类失败（超时等）视为"这次没有数据"，不计入对错；全部是环境失败的样本单独计数
    ids = set.intersection(*(set(rs) for _, rs in runs))
    stable_ok, stable_bad, no_data = set(), set(), set()
    for i in ids:
        valid = [rs[i]["pass"] for _, rs in runs if not rs[i].get("error")]
        if not valid:
            no_data.add(i)
        elif all(valid):
            stable_ok.add(i)
        elif not any(valid):
            stable_bad.add(i)
    unstable = len(ids) - len(stable_ok) - len(stable_bad) - len(no_data)
    print(f"  稳定对 {len(stable_ok)}，稳定错 {len(stable_bad)}，不稳定 {unstable}，全部超时无数据 {len(no_data)}")
    print(f"  稳定错：{' '.join(sorted(stable_bad))}")
    groups[name] = (runs, stable_ok, stable_bad)

names = list(groups)
for a in names:
    for b in names:
        if a != b:
            flip = sorted(groups[a][1] & groups[b][2])
            print(f"== {a} 稳定对、{b} 稳定错（{len(flip)}）：{' '.join(flip)}")
```

用法示例：

```bash
python3 -I /tmp/analyze.py E0=evals/results/<R8>.jsonl E1=evals/results/<R7>.jsonl,evals/results/<R9>.jsonl
```

```bash
python3 -I /tmp/analyze.py on=evals/results/<R2>.jsonl,evals/results/<R5>.jsonl off=evals/results/<R3>.jsonl,evals/results/<R6>.jsonl
```

```bash
python3 -I /tmp/analyze.py AG4=evals/results/<R1>.jsonl,evals/results/<R4>.jsonl
```

前两轮的 `.jsonl` 如果还在本机，可以一起放进对应的组里。

### 6. 报告结构（`evals/reports/<日期>-round3.md`）

1. 环境（第 0 步要求的全部内容）
2. 运行清单：序号、标签、结果文件名、`failed`、是否有效、无效原因
3. Q1～Q4 逐条回答：原始数字 → 归因 → 调整后数字（参考）→ 判定 → 依据
4. 稳定翻错与稳定错样本清单（附每条的用户消息、期望、实际）
5. 超时调查记录（每条超时的 `queue_wait_ms`、`ttft_ms`、前一条样本的情况、结论或"证据不足"）
6. 疑似标注问题（单独列出，说明理由）
7. 建议（只提建议，不改代码）

---

## 本轮评测之后（不属于评测 agent 的工作）

根据结论，由用户决定以下改动是否进行。改动按 `docs/testing/README.md` 先写用例、跑出失败再实现：

- AG.4：撤销 / 列出时在程序里清空模型填的 `agent_id`；提示词补"没点名就留空""列出 / 有哪些助手 → 列出"。
- 超时提示改为写明实际排队了多久。
- FT.4（C 方案）：确定性规则"要写文件、发送或删除的任务，风险至少为中"，补类别示例。
- 是否把 `agent_management` 改为默认打开。
