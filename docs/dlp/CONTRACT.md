# 外发脱敏（DLP）接口约定 D1（草案）

> **2026-11-18 更新：本草案已被 D1 定稿取代。** 定稿接口见 `tools/dlp-egress/docs/INTERFACE.md`（api_version 1.0），
> 宿主侧实现与配置见 `docs/mcp/DLP_EGRESS_DEPLOYMENT.md`（`[egress]`，不再用 `[[mcp.servers]]` + `[dlp] server`）。
> 与本草案的主要差异：
>
> | 本草案 | D1 定稿 |
> |---|---|
> | `decision`：allow / redact / confirm / block | `ALLOW / ANONYMIZE / REVIEW / BLOCK`，以 `ready_to_send` 为准 |
> | `session_id` 映射会话 | `task_id`（由编排器提供）作为 Token 作用域 |
> | `destination` 对象、`purpose` 枚举 | `destination_id`（目的地注册表）+ `channel` 固定取值（含 `agent`）+ 自由 `purpose` |
> | 占位符 `⟦USER_1⟧` | `[CUSTOMER_001]` 等 |
> | `findings` 计数 | `entities`（类型与位置，不含原文）+ `reason_codes` |
> | 用完即撤销 | 宿主目前不主动撤销，映射按 TTL 过期（默认 24 小时） |
>
> **本草案第 1、5 节中的两条“已确认原则”已被调整**（用户 2026-11-18 决定，见 `docs/DECISIONS.md` D-016）：
> 1. 还原只用于本机：可以显示，也可以写进本机工作区文件；不用于再次外发的内容。
> 2. 不按“用途”放宽（例如“写代码只拦密钥”），改为按目的地信任等级放宽：新增“已签约处理方”（`contracted`），第三阶段实现。
>
> 以下为原草案，保留备查。

日期：2026-11-17。状态：**草案，待用户确认**。工具由用户实现（计划放在 `tools/dlp-egress/`，参照 `tools/websearch`：Python + stdio MCP），本文件写的是 **AgentNexus（Rust 侧）对它的期望**。用户实现时如有不同，以用户告知的调用方式为准，再回头更新本文件。

## 1. 边界

- 这一版是**配合式**：只有主动调用 DLP 的路径会脱敏，不是不可绕过的强制拦截。外部助手（Claude Code、Codex、Cursor 等）自己读取文件并发给它们的模型服务，本系统看不到，也管不到。
- 已确认的使用原则（2026-11-17）：**按用途区分处理**。排查、问答类做可逆匿名化；写代码类只拦密钥和凭证、不做替换（替换会让写回仓库的代码带着占位符而损坏）；**还原只用于给用户看的内容，不用于写进文件或仓库的内容**。

## 2. 接入方式

- 通过现有 `[[mcp.servers]]` 配置启动（stdio），由 Rust 代码**直接调用**，不经过模型选择。
- **这三个工具不能出现在 General 的工具清单里**：`egress_restore` 能把占位符还原成原值，若交给模型调用，被注入的内容可以借它取回敏感信息。Rust 侧会新增一个配置项（暂定 `[dlp] server = "dlp"`）指明哪个 MCP 服务是 DLP，并把它的工具从 General 可见的工具里排除。具体字段在实现时随 D1 定稿。
- 调用超时：单次 5 秒（暂定）。DLP 不可用、超时或返回格式不对时，**需要外发的路径一律不发**（宁可不发，也不裸发）；不出本机的路径不受影响。

## 3. 工具

### 3.1 `egress_evaluate`

请求（`arguments`）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `text` | string | 准备外发的原文 |
| `purpose` | string | 用途：`diagnose`（排查）、`qa`（问答）、`coding`（写代码类任务）、`llm_backend`（发给云端模型）、`web_search`（搜索词） |
| `destination` | object | 目的地：`{"kind": "acp_agent" \| "llm_backend" \| "web_search" \| "a2a" \| "node", "name": "claude"}`，用于策略和审计 |
| `session_id` | string，可选 | 复用同一映射会话：同一会话里同一个原值始终换成同一个占位符（保证"脱敏不破坏任务"）。不传则新建 |

返回（`structuredContent`）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `decision` | string | `allow`（原样可发）、`redact`（发 `text` 里的脱敏版本）、`confirm`（需要用户确认后才能发，例如命中疑似密钥）、`block`（不得外发） |
| `text` | string | 应当外发的文本（`block` 时为空） |
| `session_id` | string | 映射会话 id（还原、撤销时用） |
| `findings` | array | 命中项：`{"type": "secret" \| "email" \| "phone" \| "username" \| "hostname" \| "path" \| "ip" \| ..., "count": n, "action": "replaced" \| "blocked" \| "kept"}`。**不得包含原值** |
| `policy_version` | string | 策略版本，写进审计记录 |

### 3.2 `egress_restore`

请求：`{"session_id", "text"}`。返回：`{"text", "restored": n, "unknown_placeholders": [...]}`。

- 只还原本会话产生的占位符；不认识的占位符原样保留并列在 `unknown_placeholders` 里。
- 会话已撤销或过期：返回错误，Rust 侧显示"无法还原"，不显示占位符以外的任何猜测内容。

### 3.3 `egress_revoke`

请求：`{"session_id"}`。返回：`{"revoked": true|false}`。撤销后该会话不能再还原，映射从密钥库删除或作废。

## 4. 占位符（建议，用户可调整）

- 同一会话内稳定：同一原值 → 同一占位符。
- 带类型，便于人和模型理解："⟦USER_1⟧"、"⟦PATH_1⟧"、"⟦HOST_1⟧"。
- 不容易与正常文本冲突（建议用不常见的括号字符）。
- 保持结构（例如路径脱敏为 `~/⟦USER_1⟧/projects/x`），让帮手仍能理解"这是一个家目录下的路径"。

## 5. 各用途的处理（已确认的原则，细则由 DLP 策略决定）

| 用途 | 个人信息（用户名、邮箱、主机名、路径中的个人部分） | 密钥 / 凭证 | 代码标识符、配置值 |
|---|---|---|---|
| `diagnose` / `qa` | 替换为占位符 | `block` 或替换 | 不涉及 |
| `coding` | 不替换（避免破坏代码） | `confirm` 或 `block` | 不替换 |
| `llm_backend` | 按任务类型：问答类替换，写代码类同 `coding` | `confirm` 或 `block` | 同左 |
| `web_search` | 替换 | `block` | 不涉及 |

## 6. Rust 侧会做的（D1 定稿后实现）

- 调用前后都校验结构；`decision` 只有 `allow` / `redact` 才会外发，`confirm` 先给用户看，`block` 用大白话说明原因。
- 审计（D4 之前先写日志事件 `egress.evaluated`）：目的地、用途、`session_id`、待发内容的摘要和长度、`findings` 计数、`decision`、`policy_version`。**不记原文，也不记脱敏后的全文**。
- 用完即撤销：一次排查、一次任务结束后调用 `egress_revoke`。

## 7. Rust 侧的验收用例（草案，接入时先写）

用模拟的 MCP DLP 服务，不依赖 Python 实现：RT-DLP-01 DLP 工具不出现在 General 的工具清单和目录接口里；02 `redact` 时外发的是脱敏文本；03 `block` 时不外发且有大白话说明；04 `confirm` 时未经确认不外发；05 DLP 超时 / 不可用时需要外发的路径不发；06 同一会话里同一原值得到同一占位符（多次调用）；07 还原只作用于显示内容，写入文件的内容不经还原；08 结束后调用了撤销，之后还原失败；09 审计日志里没有原文。

## 8. 待用户确认 / 实现时请告诉我

1. 三个工具的实际参数名和返回结构（与上面不同的地方）。
2. 占位符的实际格式。
3. `session_id` 的有效期，以及密钥库文件和本机密钥放在哪里（Rust 侧不会读取它们，只用于写进使用说明）。
4. `config.example.toml` 里的接入示例。
