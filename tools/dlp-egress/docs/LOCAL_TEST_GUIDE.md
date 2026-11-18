# dlp-egress 本地测试指南

给负责本地验证的同事用。按顺序执行，每一步都写了“期望结果”。结果按第 8 节的格式回传。

## 0. 本次要验证什么

| 编号 | 内容 | 云端是否已测 |
|---|---|---|
| T1 | 安装、单元测试、代码检查能在本地机器复现 | 已测，需在你们的机器上复现 |
| T2 | 700 条评测集与 36 条留出集的基线结果可复现 | 已测，需复现 |
| T3 | **开启 GLiNER 后的识别效果与延迟** | **未测**（云端无法下载模型） |
| T4 | AgentNexus 启动后能发现并调用 DLP 工具 | 已测“发现”；**未测**真实 LLM 调用 |
| T5 | 新标注测试集上的效果（测试集交付后） | 未测 |

## 1. 环境要求

- macOS 或 Linux（Windows 请用 WSL2）。
- Python 3.13、[uv](https://docs.astral.sh/uv/)、git。
- T3：建议 16 GB 内存。会下载几百 MB 的模型和 PyTorch（约 1–2 GB），需要能访问 huggingface.co 和 PyPI。
- T4：Rust 工具链（与仓库 CI 一致）。

## 2. 获取代码

```bash
git clone https://github.com/Grant-Huang/multi-agent-conv-mvp.git
```

```bash
cd multi-agent-conv-mvp && git checkout master && git pull
```

（如果要测某个未合并的分支，把 `master` 换成分支名。）

## 3. T1：安装与单元测试

```bash
cd tools/dlp-egress && uv sync --extra mcp
```

```bash
uv run pytest --cov --cov-report=term
```

```bash
uv run flake8 .
```

期望结果：

- pytest 最后一行为 `N passed`，没有 failed（本文编写时 N = 60），覆盖率 ≥ 80%。
- flake8 没有任何输出。
- `test_mcp_server.py` 会真实启动 MCP 子进程，单条用例约 2–5 秒，属于正常现象。

## 4. T2：基线评测复现

输出到单独的目录，不要覆盖仓库里的 `bench/results/`：

```bash
uv run python -m bench.run_benchmark --out /tmp/dlp-baseline
```

期望结果（`/tmp/dlp-baseline/SUMMARY.md`），除延迟外应**完全一致**：

| 数据集 / 口径 | 实体 micro F1 | 决策准确率 | 脱敏文本完全一致 | 危险放行 | 凭证漏放 |
|---|---|---|---|---|---|
| 700 条 · A 带主数据 | 1.0 | 1.0 | 700/700 | 0 | 0 |
| 700 条 · B 无词典 | 0.9928 | 1.0 | 682/700 | 0 | 0 |
| 留出集 · A | 0.958 | 0.9722 | 33/36 | 0 | 0 |
| 留出集 · B | 0.8624 | 0.9167 | 26/36 | 0 | 0 |

如有不一致，请把整份 `SUMMARY.md` 和对应的 `report_*.json` 发回来。

## 5. T3：GLiNER 评测（重点）

### 5.1 安装 GLiNER

```bash
uv pip install gliner jieba jieba3
```

说明：gliner 会带入 PyTorch 和 transformers，所以没有写进锁文件，只装在本地虚拟环境里。
jieba / jieba3 用于中文分词（见 5.3）。

### 5.2 下载模型

候选模型（请先在模型页面核对**许可证**是否允许公司内部使用，再下载）：

- `urchade/gliner_multi-v2.1`（多语种，优先测试）
- `urchade/gliner_multi_pii-v1`（多语种 PII，可作对照）

```bash
uv run hf download urchade/gliner_multi-v2.1 --local-dir ~/models/gliner_multi-v2.1
```

如果提示找不到 `hf` 命令，旧版 huggingface_hub 的命令名是 `huggingface-cli download`，参数相同。
本服务以 `local_files_only=True` 加载模型，运行时不会联网，所以必须先下载到本地。

### 5.3 检查中文分词设置（重要）

根据 gliner 0.2.30 的源码，GLiNER 先把文本切成“词”再识别，默认的切词方式是 `whitespace`（按空格切）。
中文没有空格，默认设置下整句可能被当成一个“词”，识别效果会很差。请打开模型目录里的 `gliner_config.json`，查看 `words_splitter_type`：

```bash
grep -n words_splitter_type ~/models/gliner_multi-v2.1/gliner_config.json
```

请分别跑两组，对比结果：

- **组 W**：保持原值（`whitespace`，或文件中没有该字段）。
- **组 J**：复制一份模型目录，在副本的 `gliner_config.json` 中把该字段设为 `"jieba"`（没有就新增）。

```bash
cp -r ~/models/gliner_multi-v2.1 ~/models/gliner_multi-v2.1-jieba
```

> 这一步是我根据源码推断的，云端没能实际运行验证。如果组 J 加载时报错，请把报错信息原样发回来。

### 5.4 运行评测

每组用 3 个阈值各跑一次（阈值越低，召回越高、误报也越多）：

```bash
for T in 0.3 0.5 0.7; do uv run python -m bench.run_benchmark --gliner-model ~/models/gliner_multi-v2.1 --gliner-threshold $T --out /tmp/dlp-gliner-W-$T; done
```

```bash
for T in 0.3 0.5 0.7; do uv run python -m bench.run_benchmark --gliner-model ~/models/gliner_multi-v2.1-jieba --gliner-threshold $T --out /tmp/dlp-gliner-J-$T; done
```

每次输出的 `SUMMARY.md` 第二行会写明所用模型和阈值，请核对。

### 5.5 怎么看结果

重点看**留出集 · B 无词典**这一档，它最接近“没有客户名单时”的真实情况：

| 指标 | 基线（无 GLiNER） | 希望看到 |
|---|---|---|
| CUSTOMER 召回 | 0.4286 | 明显提升 |
| PERSON 召回 | 0.7143 | 提升 |
| zh F1 | 0.7797 | 提升 |
| 凭证漏放 / 危险放行 | 0 / 0 | **必须仍为 0** |
| 过度拦截 | 0 | 尽量为 0 |
| 单条延迟 P95 | 约 6 ms | 记录实际值（CPU 上可能是几十到几百毫秒） |

注意：700 条评测集的 A 口径已经是 1.0，GLiNER 加入后**可能因为误报而略降**。这不一定是坏事，要结合留出集来判断。

## 6. T5：新标注测试集（测试集交付后再做）

标注规范见 `docs/ANNOTATION_GUIDE.md`。标注文件校验并转换后，用同一个评测脚本运行：

```bash
uv run python -m bench.convert_annotations path/to/annotations.jsonl --out /tmp/dlp-new.jsonl
```

```bash
uv run python -m bench.run_benchmark --data /tmp/dlp-new.jsonl --out /tmp/dlp-new-baseline
```

开启 GLiNER 时，加上第 5.4 节中效果最好的那组 `--gliner-model` 和 `--gliner-threshold` 参数。

## 7. T4：AgentNexus 接入验证

### 7.1 编译与配置

在仓库根目录：

```bash
CARGO_TARGET_DIR=target cargo build --locked --bin agentnexus
```

```bash
mkdir -p tools/dlp-egress/.data && python3 -c "import base64,os;print(base64.urlsafe_b64encode(os.urandom(32)).decode())" > tools/dlp-egress/.data/dlp-vault.key && chmod 600 tools/dlp-egress/.data/dlp-vault.key
```

复制 `config.example.toml` 为 `config.toml`，填好 LLM 配置，并取消 `# 外发脱敏` 段落的注释。

### 7.2 发现

```bash
target/debug/agentnexus config.toml
```

另开一个终端：

```bash
curl -s http://127.0.0.1:8787/api/capabilities | grep -o '"tool_id":"dlp[^"]*"'
```

期望结果：输出 `"tool_id":"dlp.egress_evaluate"`。如果服务没有启动，看第一个终端的报错：通常是缺少密钥文件，或者 `config.toml` 中路径写错。

### 7.3 真实 LLM 调用（云端未测）

在 AgentNexus 网页中对 General 说：

> 我要把这段话发给外部供应商，请先用外发检查工具检查：“将西岸三厂的CNC-07维护记录上传，关联WO-559120，客户青川重工，良率97.5%”。目的地 vendor-api，用途 vendor_diagnostics。

期望结果：

- 任务证据中出现一次 `dlp.egress_evaluate` 调用。
- 返回 `decision` 为 `ANONYMIZE`，脱敏文本中出现 `[PLANT_001]`、`[ORDER_ID_001]`、`[CUSTOMER_001]`，`CNC-07` 和 `良率97.5%` 保持不变。
  示例主数据中有“西岸三厂”“青川重工”。
- 再试一句含 `Authorization: Bearer` 加任意长串的文本，期望结果为 `BLOCK`。

记录：模型名称、是否调用了工具、调用参数是否正确（尤其是 `destination_id`、`purpose`）、返回结果。

## 8. 结果回传格式

请把下面这张表和附件发回来（贴进 PR 评论或发给我都可以）：

```
机器：<CPU 型号 / 内存 / 操作系统>     Python：<版本>     gliner：<版本>     torch：<版本>
代码版本：<git rev-parse --short HEAD>

T1 pytest：<N passed / 失败用例名>        flake8：<无输出 / 输出内容>
T2 基线：<一致 / 不一致（附 SUMMARY.md）>
T3 GLiNER：附 6 个目录中的 SUMMARY.md（W/J × 0.3/0.5/0.7）
   组 J 是否成功加载：<是 / 否（附报错）>
T4 发现：<是 / 否>   真实调用：<模型名，是否调用，返回 decision>
问题与观察：<自由填写>
```

附件：各 `--out` 目录下的 `SUMMARY.md` 和 `report_*.json`。
报告里的凭证值已经写成 `<SECRET:长度>`，样本均为合成数据，可以直接发送。

## 9. 常见问题

| 现象 | 处理 |
|---|---|
| `dlp-egress-mcp 启动失败：缺少 DLP_VAULT_KEY…` | 按 7.1 生成密钥文件，并检查 `dlp.example.yaml` 中的 `vault_key_file` 路径 |
| `已启用 GLiNER，但未安装 gliner 包` | 在 `tools/dlp-egress` 目录执行 `uv pip install gliner` |
| 加载模型时尝试联网或报 `LocalEntryNotFoundError` | 模型未完整下载，或 `--gliner-model` 没有指向本地目录 |
| `uv sync` 之后 gliner 不见了 | `uv sync` 会把环境恢复到锁文件状态，重新执行 5.1 即可 |
| 评测输出 `errors` 不为 0 | 说明有请求按 fail closed 被拦截，请附上 stderr 日志 |
