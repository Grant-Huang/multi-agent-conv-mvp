# dlp-egress：AgentNexus 统一外发脱敏 MCP 工具（MVP）

按《AgentNexus 统一外发脱敏服务：技术设计与验证方案 v0.1》实现的独立 MCP 服务。外发前调用，返回
`ALLOW / ANONYMIZE / REVIEW / BLOCK` 判定和脱敏载荷。只有 `ready_to_send=true` 时才可发送返回的载荷。
接口为 D1 定稿版（api_version 1.0，统一外壳 + 结构化返回）。

AgentNexus 通过 `[egress]` 配置由 **Rust 宿主直接调用**本服务（工具调用守卫 + 云端 LLM 检查），不交给模型。
宿主侧覆盖了工具调用与 LLM 请求；编码 agent 子进程、网络层出站等仍未覆盖，详见
[部署与使用说明](../../docs/mcp/DLP_EGRESS_DEPLOYMENT.md) 第 5 节。

## 文档

| 文档 | 给谁看 |
|---|---|
| [docs/INTERFACE.md](docs/INTERFACE.md) | 调用方：工具参数、统一返回外壳、决策语义、D1 定稿记录 |
| [../../docs/mcp/DLP_EGRESS_DEPLOYMENT.md](../../docs/mcp/DLP_EGRESS_DEPLOYMENT.md) | 部署与运维：`[egress]` 配置、audit 注意事项、enforce 行为、覆盖范围、回滚 |
| [docs/LOCAL_TEST_GUIDE.md](docs/LOCAL_TEST_GUIDE.md) | 本地测试人员：安装、复现、GLiNER 评测、AgentNexus 接入验证、结果回传 |
| [docs/ANNOTATION_GUIDE.md](docs/ANNOTATION_GUIDE.md) | 测试集编写与标注人员：样本要求、标注规范、交付流程 |
| [../../docs/mcp/DLP_EGRESS_ENFORCEMENT_PLAN.md](../../docs/mcp/DLP_EGRESS_ENFORCEMENT_PLAN.md) | 必经化方案（已批准；第一、二阶段已实施） |

## 组成

| 模块 | 作用 | 对应设计文档 |
|---|---|---|
| `detector.py` | Presidio `AnalyzerEngine` 编排全部识别器，NFKC/零宽字符归一化，跨识别器 span 合并 | 3 节 5–6 步 |
| `recognizers.py` | 通用规则：凭证结构、邮箱/电话、工单 vs 设备编号、金额、指标、机密概念、组织/人员启发式；租户主数据词典 | 8.2 节 |
| `secrets.py` | detect-secrets 封装（关闭高熵插件，扩展只返回前缀的插件结果） | 2.1 节 |
| `gliner_adapter.py` | GLiNER 适配器，**默认关闭**，只读本地模型 | 2.1 节 |
| `policy.py` + `default_policy.yaml` | 版本化 YAML 策略：目的地信任等级 × 实体类型 → 动作；取最严 | 4 节 |
| `vault.py` | 作用域 (tenant, task) 的 Token Vault：AES-256-GCM（AAD 绑定作用域）、HMAC 幂等、TTL、撤销即擦除 | 5 节 |
| `service.py` | 评估流程、JSON 类型化转换、路由字段保留、保真校验、fail closed、无正文审计日志 | 3、6 节 |
| `mcp_server.py` | stdio MCP：`egress_evaluate` / `egress_restore` / `egress_revoke` | 7 节 |

### 关键行为

- **身份不由 Agent 自报**：`tenant_id` / `actor_id` 只来自服务端配置，工具参数里没有这两个字段。
- **目的地注册表**：未登记的 `destination_id` 一律 BLOCK。`external_untrusted` = 已登记但按不可信处理，可脱敏后外发。
- **凭证**：BLOCK，替换为 `[BLOCKED_SECRET]`，永不写入 Vault，响应与日志中不出现原值。
- **机密信息**：REVIEW，替换为 `[REDACTED_CONFIDENTIAL]`，`ready_to_send=false`。
- **标识符**：客户、项目、厂区、人员、工单、邮箱、电话、金额替换为任务内稳定 Token（如 `[CUSTOMER_001]`）。
- **保真**：生产指标与设备号默认保留；金额只有调用方声明 `constraints: ["preserve:AMOUNT"]` 且策略允许时才保留；
  收件人（`to=` / `guests=` 等路由字段）不做 Token 化，可按 `allowed_recipient_domains` 校验；JSON 载荷键名、
  结构与非字符串值保持不变。校验不通过 → BLOCK。
- **Fail closed**：识别异常、空载荷、缺少密钥或身份配置（启动失败）均不会放行原文。
- **恢复**：`egress_restore` 默认关闭（`allow_restore: false`），只允许 `local_ui` 场景。恢复要求同租户、同任务、同属主、
  未过期未撤销；外部响应中伪造或越界的 Token 原样保留并列入 `unknown_tokens`。
- **接口约束**：`channel` 只能是 `email / slack / external_api / file_upload / mcp / agent`；载荷上限 256 KB；
  每次评估返回 `request_id`；三个工具都发布 output schema，返回统一外壳 `{api_version, status, data, error}`。

## 安装与运行

需要 Python 3.13 与 [uv](https://docs.astral.sh/uv/)。从仓库根目录：

```bash
cd tools/dlp-egress && uv sync --extra mcp
```

```bash
mkdir -p tools/dlp-egress/.data && python3 -c "import base64,os;print(base64.urlsafe_b64encode(os.urandom(32)).decode())" > tools/dlp-egress/.data/dlp-vault.key && chmod 600 tools/dlp-egress/.data/dlp-vault.key
```

```bash
tools/dlp-egress/.venv/bin/dlp-egress-mcp --config tools/dlp-egress/config/dlp.example.yaml
```

配置项见 `config/dlp.example.yaml`；目的地与主数据示例见 `config/destinations.example.yaml`、
`config/master_data.example.yaml`。所有配置项也可以用 `DLP_*` 环境变量提供（环境变量优先）。

## 接入 AgentNexus

不要把本服务配置成 `[[mcp.servers]]`（那会把工具交给模型）。在 `config.toml` 中配置 `[egress]`，由宿主直接调用：

```toml
[egress]
mode = "audit"   # off（默认）/ audit / enforce
dlp_command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
dlp_args = ["--config", "tools/dlp-egress/config/dlp.host.yaml"]
```

宿主用的 DLP 配置需要 `allow_restore: true`（用于恢复云端 LLM 输出中的 Token）。工具与 LLM 后端的登记、
audit 阶段的注意事项、enforce 下的行为见 [部署与使用说明](../../docs/mcp/DLP_EGRESS_DEPLOYMENT.md)。

已实测（2026-11-18）：AgentNexus 的 Rust 集成测试 `real_dlp_service_roundtrip_when_configured` 连接真实的本服务，
结果：结构化返回符合 schema；工具参数被脱敏；带凭证样式串的调用被拦下；LLM 消息被脱敏，并取得 Token 映射。

## 测试

```bash
cd tools/dlp-egress && uv run pytest --cov --cov-report=term && uv run flake8 .
```

| 测试文件 | 内容 |
|---|---|
| `test_detection.py` | 各类实体识别、凭证格式、主数据词典、span 合并、前缀型插件回归 |
| `test_service.py` | 四种决策、未登记目的地、金额约束、路由邮箱、JSON 结构保真、fail closed、Unicode 混淆 |
| `test_vault.py` | 同作用域恢复、跨任务/跨租户/跨属主/错误上下文拒绝、Token 注入、TTL、撤销、无明文落盘、重启幂等 |
| `test_gliner_adapter.py` | 假模型验证 GLiNER 接入与合并优先级；启用但未配置模型时启动失败 |
| `test_mcp_server.py` | 真实 stdio 子进程 + MCP 客户端：握手、工具发现、调用；`--config`；缺身份配置时退出 |
| `test_benchmark_gate.py` | 700 条与留出集上的**安全底线**：凭证零漏放、零危险放行、零跨作用域恢复 |
| `test_d1_interface.py` | D1 定稿：统一外壳与 output schema、request_id、通道取值（含 agent）、256 KB 上限、恢复只允许 local_ui |
| `test_annotations.py` | 标注格式解析、期望结果推导、校验与转换脚本、模板有效性 |

评测：`uv run python -m bench.run_benchmark --out bench/results`（结果见 `bench/results/SUMMARY.md`）。

测试与留出集中的凭证样式字符串都是合成的，集中放在 `bench/fake_secrets.py`，在运行时拼接，
源码里不出现完整的凭证格式，避免触发仓库的 gitleaks 扫描。评测报告中的凭证值写作 `<SECRET:长度>`，
源码与报告中的零宽字符写作 `\u` 转义（CI 的提示注入扫描会阻断不可见字符）。

## 评测结果与解读

完整数据见 `bench/results/`。下表为最终版本。

| 数据集 / 口径 | 实体 micro F1 | 决策准确率 | 脱敏文本完全一致 | 危险放行 | 凭证漏放 |
|---|---|---|---|---|---|
| 700 条 · A 带主数据 | 1.000 | 1.000 | 700/700 | 0 | 0/10 |
| 700 条 · B 无词典 | 0.993 | 1.000 | 682/700 | 0 | 0/10 |
| 留出集 36 条 · A | 0.958 | 0.972 | 33/36 | 0 | 0/5 |
| 留出集 36 条 · B | 0.862 | 0.917 | 26/36 | 0 | 0/5 |
| **1000 条模拟测试集 · A** | **0.743** | **0.848** | **652/1000** | **141** | **152/200** |
| **1000 条模拟测试集 · B** | **0.477** | **0.768** | **420/1000** | **141** | **152/200** |

1000 条模拟测试集（`dlp-gold-v1-simulated-1000`，由 AI 按 `docs/ANNOTATION_GUIDE.md` 生成，不入库）是目前
**唯一与开发隔离**的测试集，结果最能说明真实水平：凭证漏放、危险放行均远未达标，机密信息的实体识别几乎为 0，
无词典时人员 / 厂区 / 项目识别为 0。原始文件中 22 条中英混排被标为 `en`，评测时按规范改为 `zh`（只影响分语言统计）。
**按“测试集不用于调参”的约定，本次没有根据这份结果修改任何识别规则。**

两种口径下的共同结果：往返恢复 = 原文 100%，跨任务/跨租户恢复成功 0 次，指标与设备号 100% 原样保留，
未登记目的地 100% BLOCK。单条评估延迟 P95 约 6 ms（不含 GLiNER）。

**700 条上的高分不能当作识别能力的证据。** 原因：

1. 评测集词汇极少（客户 18 个、设备号 5 个、工单号 5 个），句式来自少量模板。
2. A 口径的主数据名单直接取自评测集。
3. 编写 B 口径的通用规则时，我看过评测集（公司名后缀、机密概念词与之重叠），
   之后又按评测集的失败样本修正过 PLANT / PERSON 规则。这属于在测试集上调参。

**留出集更接近真实泛化能力**：36 条人工样本（`bench/holdout.py`），词汇与句式不来自评测集。
中文无词典时 F1 只有 0.78。漏识别集中在没有行业后缀的公司名（如“松岩液压”“Acme”）、没有职务称谓的人名
（如“王晓峰”）、以及上下文较弱的英文人名。这部分正是 GLiNER 应补强的。

**留出集首次运行发现了一个凭证漏放**（HO-020：`token: ghp_...` 被判 ALLOW），原因是 detect-secrets 的 GitHub
插件只返回 `ghp` 前缀。已修复并加回归测试。修复前的留出集结果保留在 `bench/results/holdout_before_secret_fix/`。
修复后，留出集的 SECRET 类别不再是独立评估。
HO-021 原先用的是 Stripe 文档里公开的 `sk_test_...` 示例密钥，会触发 GitHub 推送保护，
已换成不符合 Stripe 格式的合成串。该样本现在只覆盖 `api_key: "..."` 规则，不再覆盖 detect-secrets 的 Stripe 插件。

## 局限与后续

- 未实现：网络层 egress 强制、编排器内强制调用、`commit` 接口（绑定载荷哈希与审批，防 TOCTOU）、人工审批流、
  附件/二进制文件解析、外部响应扫描。
- Vault 主密钥是本地文件，未接 KMS；SQLite 单机存储，不适合多实例。
- GLiNER 未参与任何评测：当前云环境禁止访问 huggingface.co，无法下载模型。
- 无词典时的组织/人员识别是启发式规则，中文精度有限；上线需要租户主数据同步，或启用经过评估的 NER 模型。
- 留出集只有 36 条，且由同一作者编写、标注。正式基准应由工业领域人员独立编写与审核（设计文档 9.3 节）。
- 评测集字段 → 请求的映射（目的地、`preserve:AMOUNT` 约束）是调用方上下文，见 `bench/run_benchmark.py` 头部说明。
