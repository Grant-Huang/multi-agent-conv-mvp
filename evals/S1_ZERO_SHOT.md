# s1 决策小模型零样本对照：本机测试指南

日期：2026-11-18。脚本：`scripts/s1_zero_shot.py`。整个测试只读：不改代码、配置和数据集，也不调用 llama-server。

## 1. 要回答的问题

开源决策小模型 [decisionmodels/s1-llm-auto-router](https://huggingface.co/decisionmodels/s1-llm-auto-router)（MIT 许可，ONNX 格式，183 MB，CPU 运行）固定回答七个问题。这次测试要弄清两件事：这七个问题里哪些能作为本项目路由的输入信号，以及它在 Mac mini 上实际有多快。

| 组 | s1 的答案 | 对照本项目的标注 | 用途 |
|---|---|---|---|
| A | `needs_tools`（是/否概率） | 类别是不是 L2 或 L4（"需要取数工具"那一半） | **主判据**，决定要不要往下做 |
| B | `category`（9 选 1） | 角色是不是 coding | 参考 |
| C | `stakes`（4 档） | 风险等级 | 参考。两者含义不同，所以只列交叉表 |
| D | `follow_up`、`difficulty` | 动作 | 参考 |
| E | — | 截断条数、单次判断延迟 | 判断能否接入 |

每组除了看准确率，还会看"置信度能不能把错误集中到低置信那一端"。这个指标用 AUROC 表示，大于 0.7 才算有用。

## 2. 准备（只做一次）

需要：Apple Silicon 的 Mac mini、Python 3.10 及以上、约 1 GB 磁盘空间，第一次运行时要能访问 huggingface.co。

在仓库根目录执行：

```bash
git fetch origin claude/kind-davinci-sve0mv && git checkout claude/kind-davinci-sve0mv
```

```bash
python3 -m venv ~/.venvs/s1 && source ~/.venvs/s1/bin/activate
```

```bash
pip install "onnxruntime>=1.20" "tokenizers>=0.20" "numpy>=1.26" "huggingface_hub>=0.25"
```

脚本自测时用的版本是 onnxruntime 1.29.0、tokenizers 0.23.3、numpy 2.5.3、huggingface_hub 2.4.0。虚拟环境放在仓库外（`~/.venvs/s1`），不会被误提交。之后每次新开终端，先执行 `source ~/.venvs/s1/bin/activate`。如果 pip 找不到 onnxruntime 的安装包，换成 Python 3.12 再建虚拟环境。

## 3. 冒烟：只跑 3 条

```bash
python3 scripts/s1_zero_shot.py --limit 3 --repeat 1 --label smoke
```

第一次运行会按固定版本（提交 `572c5b6c`）把模型下载到 `~/.cache/huggingface/`。能看到"已写出：…"就说明正常。冒烟结果不用发回。

## 4. 正式测试：分两轮

### 第 1 轮：空闲时

35B 服务开着没关系，但这段时间不要让它推理：不跑 an-eval，也不在界面里提问。

```bash
python3 scripts/s1_zero_shot.py --label idle --note "35B 空闲"
```

100 条，每条推理 3 遍（只用来测延迟），通常一两分钟内跑完。

### 第 2 轮：35B 正在推理时

这一轮测的是两个模型同时运行、争抢内存和算力时 s1 的延迟。

1. 开**另一个终端**，让 35B 持续推理。下面这条命令和平时跑 an-eval 一样，用你本机的 `config.toml`：

   ```bash
   cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --limit 20 --label s1-bg
   ```

2. 等它开始输出进度后，在原来的终端执行：

   ```bash
   python3 scripts/s1_zero_shot.py --label busy --note "35B 同时在跑 an-eval"
   ```

3. 如果 s1 先跑完，第二个终端的 an-eval 可以直接停掉。它的结果受到了干扰，**不要拿来和 FT 实验比**。

注意事项：
- 不要和 `scripts/ft_experiment.py` 同时跑，否则会污染 FT 的延迟数据。
- 默认用 4 个线程，和模型卡测延迟时一致。想多测一组时可以加 `--threads 2`，同时用 `--label` 区分文件名。

## 5. 要发回的文件

两轮各会在 `evals/results/` 下生成三个文件（这个目录不入库）：

- `s1-zero-shot-<时间>-idle.md` 和 `.jsonl`
- `s1-zero-shot-<时间>-busy.md` 和 `.jsonl`

把这四个文件发给我就行。`.jsonl` 里保存了每条样本的全部概率，以后要换分析口径，可以用 `--reanalyze` 直接重算，不用重跑模型：

```bash
python3 scripts/s1_zero_shot.py --reanalyze evals/results/s1-zero-shot-<时间>-idle.jsonl
```

## 6. 怎么判读

| 结果 | 结论和下一步 |
|---|---|
| A 组准确率 ≥ 75%（E1 的类别准确率），且置信度预测判错的 AUROC > 0.7 | 值得做 FT.6：用 s1 的 `needs_tools`，加上"是否生成文档"的判断，由 Harness 算出 L1–L4；低置信时交给现有路由 |
| A 组 ≥ 75%，但置信度没用 | s1 可以作为一个参考特征，不能拿来做升级门控 |
| A 组 < 75% | s1 不进入 FT 主线；FT.5 改用现有 Qwen 回答两个是/否问题，再由代码算出类别 |
| busy 轮 P95 ≤ 100 ms | 延迟可以忽略（E1 路由的 P50 是 2.7 秒） |
| busy 轮 P95 明显高于 idle | 记录下来。接入时要考虑给 s1 固定 CPU 线程数 |

两点提醒：
- 有类别标注的样本只有 28 条，A 组准确率的 95% 置信区间大约是 ±16 个百分点，只能用来粗筛，不能当最终结论。
- 有 2 条长文本审查样本（rev-long-01/02）会超过 512 token 而被截断，报告里会列出来。

## 7. 已知取舍

- **上下文的拼法是本脚本自己定的。** s1 只读上下文的前 600 个字符，所以脚本把"进行中的任务"放在最前面，再按从近到远的顺序拼对话历史。模型卡只说上下文是"简短的对话摘要"，没有规定格式。
- **只用 CPU 推理，没启用 CoreML。** 和模型卡的测法保持一致。
- **期望值统一按 reviewer 模式的标注。** 其他审查模式（`by_review_mode`）不参与这次对照。

## 8. 清理

```bash
deactivate && rm -rf ~/.venvs/s1 ~/.cache/huggingface/hub/models--decisionmodels--s1-llm-auto-router
```
