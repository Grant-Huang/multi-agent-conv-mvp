# 外发脱敏（DLP）接口约定 D1（定稿）

日期：2026-11-18。状态：**定稿**（用户 2026-11-18 确认：9 项建议全部同意，两处缺口按建议补）。

- 工具一侧以 `tools/dlp-egress/docs/INTERFACE.md`（v0.1，master `8fd24e2`）为基础，本文第 3 节列出的改动由工具作者按本约定实现，接口版本记为 **`api_version = "1"`**。
- 本文写的是 **AgentNexus（Rust 侧）怎样调用它、对它有什么要求**。两份文档冲突时：工具的参数和返回以 `INTERFACE.md` 为准，AgentNexus 的调用方式和义务以本文为准，发现冲突就同时改两份。
- Rust 侧先用模拟的 MCP 服务实现和测试（第 7 节），不依赖 Python 实现完成。

## 1. 边界

- **配合式，不是强制拦截**：只有主动调用 DLP 的路径会脱敏。外部助手（Claude Code、Codex、Cursor 等）自己读文件、自己发给它们的模型服务，本系统看不到也管不到。强制化见 D5。
- **按用途区分处理**（2026-11-17 确认）：排查、问答类做可逆匿名化；写代码类只拦密钥和凭证、不做替换（替换会让写回仓库的代码带着占位符而损坏）；**还原只用于给用户看的内容，不用于写进文件或仓库的内容**。
- 工具本身的已知缺陷（代码审查 2026-11-18：R-20261118-H1 嵌套对象不检测、M2～M5）由工具一侧修复。修复前，Rust 侧只以**纯文本**（`text`）调用，不用 `json_body`。

## 2. 接入方式

### 2.1 配置（新增 `[dlp]` 一节，不放进 `[[mcp.servers]]`）

```toml
[dlp]
command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
args = ["--config", "tools/dlp-egress/config/dlp.yaml"]
timeout_secs = 5          # 单次调用超时
```

- **理由**：放在 `[[mcp.servers]]` 里的工具会被登记为能力，有可能进入 General 的工具清单。`egress_restore` 能把 Token 还原成原值，交给模型调用时，被注入的内容可以借它取回敏感信息。单独一节可以从配置层面保证这三个工具**永远不会**出现在任何模型可见的工具清单、能力目录或 `GET /api/capabilities` 里。
- 不写 `[dlp]`：所有需要外发的 DLP 路径**一律不发**，界面说明"没有配置外发脱敏，不能发送"；不出本机的功能不受影响。
- 进程：按需启动 stdio 子进程，复用 `an-agents` 现有的 MCP 客户端；空闲时保持，服务关闭时结束进程组。同一时间串行调用（与工具"实例内串行"一致）。

### 2.2 失败一律不发（fail closed）

以下任一情况，需要外发的路径**不发送**，并用大白话说明：没有配置、进程起不来、超时、返回不是合法结构（不符合第 4 节的 schema）、`status = "error"`、`api_version` 不是 `"1"`、`ready_to_send` 不是 `true`。

## 3. 相对 INTERFACE v0.1 的改动（工具一侧实现）

| # | 结论 | 具体要求 |
|---|---|---|
| 9.1 | 统一返回外壳 | 三个工具都返回 `{"status": "success" \| "error", "api_version": "1", "data": {…} \| null, "error": {"code", "message"} \| null}` |
| 9.2 | `task_id` 由编排器提供 | 参数名不变。AgentNexus 只从 Rust 代码调用这三个工具、不交给任何模型，`task_id` 由 Rust 生成（第 5 节） |
| 9.3 | `channel` 改为枚举并校验 | `email` / `slack` / `external_api` / `file_upload` / `mcp` / **`agent`**（新增：发给外部助手）/ **`clipboard`**（新增：交给用户自己复制带走）。取值不在枚举内：BLOCK，`reason_codes` 含 `channel_invalid` |
| 9.4 | `vault_ref` 改名 `request_id` | 只用于审计关联 |
| 9.5 | 暂不做 `egress_commit` | AgentNexus 自己把"给用户预览的内容"和"实际发出的内容"绑定（一次性同意凭证 + 内容摘要，见第 6 节）。强制化（D5）时再加 |
| 9.6 | 还原只在本机做 | 只由 Rust 调用，`context` 固定为 `local_ui`；`allow_agent_restore` 保持关闭 |
| 9.7 | 载荷上限 256 KB | 超出：BLOCK，`reason_codes` 含 `payload_too_large`。AgentNexus 自己发的诊断包不超过 32 KB |
| 9.8 | 返回带版本号 | 外壳里的 `api_version`，工具名不变 |
| 9.9 | 结构化返回 | 启用 MCP `structuredContent`，并为三个工具声明 output schema。AgentNexus 用现有的 schema 校验（`crates/agents/src/mcp.rs`），不合格按失败处理 |
| 缺口 A | 补排查场景常见的个人信息类型 | 新增实体类型 **`USERNAME`**（本机用户名）、**`HOSTNAME`**（主机名）、**`IP`**、**`FILE_PATH`**（路径里的个人部分，例如 `/Users/zhang/` 中的 `zhang`），对 `external_untrusted` 目的地默认 Token 化。AgentNexus 在调用前也会先做确定性处理（第 5 节），两层都做 |
| 缺口 B | 写代码类"只拦密钥、不替换" | 不改接口，用目的地配置实现：目的地注册表新增字段 `profile: secrets_only`，这类目的地只处理 `SECRET`（BLOCK），其余实体原样保留。写代码类外发（D3 起）登记成这种目的地 |

## 4. AgentNexus 使用的调用

### 4.1 `egress_evaluate`

AgentNexus 发出的参数：

| 参数 | 取值 |
|---|---|
| `task_id` | Rust 生成，见第 5 节 |
| `channel` | `agent`（第 4 层，发给外部助手）或 `clipboard`（第 3 层，交给用户复制） |
| `destination_id` | 第 4 层：`agent:<助手 id>`，例如 `agent:claude`；第 3 层：`user:clipboard` |
| `purpose` | `diagnose` |
| `text` | 待发全文（只用 `text`，见第 1 节） |
| `constraints` | 不传 |

AgentNexus 读取 `data` 里的：`decision`、`ready_to_send`、`sanitized_text`、`reason_codes`、`entities`（只用类型和计数）、`tokens`（只用个数）、`policy_version`、`request_id`。

处理规则：

| 结果 | AgentNexus 的处理 |
|---|---|
| `ready_to_send = true`（ALLOW / ANONYMIZE） | 预览和发送都只用 `sanitized_text`，原文不出现在预览里，也不发送 |
| REVIEW | 不发送。界面说明"内容里有需要人工判断的机密信息"，展示 `sanitized_text` 作为参考；本期不做审批流 |
| BLOCK，含 `secret_blocked` | 不发送。提示"内容里有密钥或凭证，已拦下；如果这是真实的密钥，建议尽快更换" |
| BLOCK，其他原因 | 不发送。按 `reason_codes` 给大白话说明（例如目的地没有登记） |

### 4.2 `egress_restore`

只用于把帮手的回复还原后**显示给用户**：`{"task_id", "text": <帮手回复>, "context": "local_ui"}`。还原结果只进界面，不写文件、不进模型历史、不作为修改配置的输入（帮手给出的修改请求按 Token 原样校验，含 Token 的修改请求视为不合格）。`unknown_tokens` 原样显示。

### 4.3 `egress_revoke`

一次排查结束（显示完帮手的回复、发送失败或用户关闭卡片）后调用；同意凭证过期未用也要撤销。撤销失败只记日志，不影响界面。

## 5. AgentNexus 侧的约定

- **`task_id`**：每次"预览"生成一个新的 `diag-<uuid>`，同一次预览内的评估、发送后的还原、撤销都用它。
- **调用前的确定性处理**（不依赖 DLP 的识别能力）：家目录换成 `~`；环境变量只保留名字，不带值；主机名、本机用户名换成 `<host>`、`<user>`。处理完再交给 DLP。
- **工具不出现在任何模型可见的位置**：General 的工具清单、能力目录接口、对话判断的 schema 里都没有。
- **审计**：日志事件 `egress.evaluated`（`task_id`、目的地、`decision`、`reason_codes`、实体类型计数、`policy_version`、`request_id`、待发内容的长度和摘要），`agent.diagnose_sent`（目的地、内容摘要和长度）。**不记原文，也不记脱敏后的全文**。数据表留给 D4。

## 6. 预览与发送的绑定（替代 9.5 的 commit）

1. 预览：Rust 组装待发全文 → 第 5 节处理 → `egress_evaluate` → 得到 `sanitized_text`，再拼上给帮手的说明文字，形成**完整的待发内容**；连同目的地说明、同意凭证（一次有效、5 分钟过期、绑定内容摘要）返回给界面。
2. 用户看到的就是将要发出的全部内容，一字不差。
3. 发送：凭证校验通过后，发送服务端保存的那份内容（不接受界面传回内容）；凭证用过即作废。

## 7. Rust 侧验收用例

用模拟的 MCP DLP 服务（不依赖 Python 实现）。编号和内容见 `docs/testing/CASES.md` 的 RT-DLP、RT-AGDIAG 部分，实现前先写并跑出失败。

## 8. 工具一侧需要做的事（清单）

1. 第 3 节表格里的全部改动（9.1、9.3、9.4、9.7、9.8、9.9、缺口 A、缺口 B）。
2. 目的地注册表示例里加上 `user:clipboard`（trust `external_untrusted`，purposes `[diagnose]`）和 `agent:*` 的写法说明（或逐个登记 `agent:claude`、`agent:codex-acp`、`agent:cursor`、`agent:zcode`）。
3. 修复代码审查 2026-11-18 的 R-20261118-H1、M2～M5。
4. 改完后更新 `INTERFACE.md`，并告知 AgentNexus 侧，以便用真实工具跑一次接入验收。
