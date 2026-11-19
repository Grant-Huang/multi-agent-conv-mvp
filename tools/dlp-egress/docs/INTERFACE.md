# dlp-egress 调用接口（D1 定稿，api_version 1.0）

本文是 dlp-egress 与 AgentNexus 之间的接口约定，2026-11-18 由 AgentNexus 侧确认（定稿记录见第 9 节）。
示例输出来自实际运行，耗时和请求 ID 已省略。

## 1. 定位

- 职责：在内容**发出 AgentNexus 之前**判定能不能发、怎么发。返回 `ALLOW / ANONYMIZE / REVIEW / BLOCK` 和脱敏后的载荷。
- 不负责：实际发送、人工审批流、附件二进制解析、外部响应扫描。
- 调用方：AgentNexus 的 **Rust 宿主直接调用**（`crates/agents/src/egress.rs`），三个工具**不注册给模型**。
  部署与使用见 `docs/mcp/DLP_EGRESS_DEPLOYMENT.md`。
- 接入形态：stdio MCP 服务（`dlp-egress-mcp`）；同一套逻辑也可作为 Python 库使用（`EgressService`）。

## 2. 部署与身份

| 项 | 说明 |
|---|---|
| 启动 | `dlp-egress-mcp --config <dlp.yaml>`，或用 `DLP_*` 环境变量（环境变量优先） |
| 可信身份 | `tenant_id`、`actor_id` 只来自服务端配置。工具参数里没有这两个字段 |
| `task_id` | 由 AgentNexus 编排器提供（任务上下文），模型接触不到 |
| 目的地注册表 | `destinations_file`。未登记的 `destination_id` 一律 BLOCK |
| 主数据 | `master_data_file`（客户 / 项目 / 厂区 / 人员名单），可选 |
| Vault | `vault_path` + `vault_key_file`（32 字节 base64）。缺少密钥时拒绝启动 |
| 恢复开关 | `allow_restore`（环境变量 `DLP_ALLOW_RESTORE`），默认 false。AgentNexus 宿主需要恢复 LLM 输出时设为 true（模型接触不到这个工具）。旧名 `allow_agent_restore` 仍可用 |
| 启动失败 | 缺少身份 / 密钥、配置非法：进程以退出码 2 退出 |

## 3. 返回格式（所有工具一致）

结构化返回（MCP `structuredContent`），并为每个工具发布 output schema（`tools/list` 的 `outputSchema`）。
文本内容是同一份 JSON，供不支持结构化返回的客户端使用。

```json
{"api_version": "1.0", "status": "success", "data": { ... }, "error": null}
{"api_version": "1.0", "status": "error",   "data": null,      "error": {"code": "restore_disabled", "message": "…"}}
```

调用方必须：按 output schema 校验 `structuredContent`，检查 `api_version == "1.0"` 和 `status == "success"`。
校验不过视为失败，**不外发**（AgentNexus 的实现即如此）。

## 4. `egress_evaluate`

### 4.1 参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `task_id` | string | 是 | Token 作用域。同一任务内同一实体得到同一 Token，不同任务相互隔离 |
| `channel` | string | 是 | 固定取值：`email` / `slack` / `external_api` / `file_upload` / `mcp` / `agent`（发给外部助手）。其他取值 → BLOCK（`invalid_channel`） |
| `destination_id` | string | 是 | 必须在目的地注册表中，否则 BLOCK |
| `purpose` | string | 是 | 用途。目的地配置了 `purposes` 时，不在列表中就 BLOCK |
| `text` | string \| null | 二选一 | 自由文本载荷 |
| `json_body` | object \| array \| null | 二选一 | 结构化载荷，键名和非字符串值保持不变 |
| `constraints` | string[] \| null | 否 | 目前只支持 `preserve:<TYPE>`，例如 `preserve:AMOUNT`。只有策略允许的组合才生效 |

载荷上限 **256 KB**（UTF-8 字节；JSON 按紧凑序列化计算），超出即 BLOCK（`payload_too_large`）。

### 4.2 返回 `data`

```json
{
  "api_version": "1.0",
  "status": "success",
  "data": {
    "decision": "ANONYMIZE",
    "policy_version": "egress-v1",
    "request_id": "req_…",
    "sanitized_text": null,
    "sanitized_json": {"to": "buyer@acme-demo.com", "client": "[CUSTOMER_001]",
                       "order": "[ORDER_ID_001]", "yield": "良率97.5%"},
    "entities": [
      {"type": "EMAIL", "start": 0, "end": 19, "score": 0.9, "source": "rule", "field_path": "to"},
      {"type": "CUSTOMER", "start": 0, "end": 4, "score": 0.99, "source": "master_data", "field_path": "client"},
      {"type": "ORDER_ID", "start": 0, "end": 9, "score": 0.85, "source": "rule", "field_path": "order"},
      {"type": "METRIC", "start": 0, "end": 7, "score": 0.8, "source": "rule", "field_path": "yield"}
    ],
    "reason_codes": ["customer_tokenized", "metric_preserved_by_policy", "order_id_tokenized", "routing_preserved"],
    "tokens": ["[CUSTOMER_001]", "[ORDER_ID_001]"],
    "validation": {"schema_ok": true, "preserved_values_ok": true, "no_secret_in_output": true, "routing_unchanged": true},
    "ready_to_send": true,
    "elapsed_ms": 4.1
  },
  "error": null
}
```

（请求：`channel = "agent"`，`json_body = {"to":"buyer@acme-demo.com","client":"青川重工","order":"WO-559120","yield":"良率97.5%"}`）

| 字段 | 说明 |
|---|---|
| `decision` | `ALLOW` / `ANONYMIZE` / `REVIEW` / `BLOCK`，含义见 4.3 |
| `policy_version` | 判定所用策略版本 |
| `request_id` | 本次评估的 ID，用于审计关联（**每次都有**，含 BLOCK） |
| `sanitized_text` / `sanitized_json` | 与请求载荷类型对应，另一个为 null。在识别之前就 BLOCK 时两者都是 null（未登记目的地、非法通道、超过上限、空载荷、内部异常） |
| `entities` | 识别出的实体：类型、偏移（Unicode 码点，相对于字段原文）、分数、来源、字段路径。**不含原文** |
| `reason_codes` | 判定原因，见 4.4 |
| `tokens` | 本次使用的可逆 Token（不含原值） |
| `validation` | 保真校验，任一为 false 时 decision 强制为 BLOCK |
| `ready_to_send` | 仅当 decision 为 ALLOW 或 ANONYMIZE 且校验全部通过时为 true |

### 4.3 决策语义与调用方义务

| decision | 含义 | 调用方必须做的事 |
|---|---|---|
| `ALLOW` | 无敏感实体，或只有可以原样保留的实体 | 发送 `sanitized_*`（与原文相同） |
| `ANONYMIZE` | 标识符已替换为 Token | **只发送 `sanitized_*`**；外部返回的内容需要恢复时，在本机用 `egress_restore` |
| `REVIEW` | 含机密信息（`[REDACTED_CONFIDENTIAL]`） | **不发送**，转人工审批；`sanitized_*` 只是给审批人看的预览 |
| `BLOCK` | 凭证、未登记目的地、非法通道、超过上限、校验失败、内部异常等 | **不发送**。若原因是 `secret_blocked`，通知用户尽快更换（轮换）被检出的密钥 |

一句话规则：**`ready_to_send` 为 true 才发，且只发 `sanitized_*`。**

### 4.4 reason_codes

| 代码 | 触发条件 | 决策 |
|---|---|---|
| `invalid_channel` | channel 不在固定取值中 | BLOCK |
| `payload_too_large` | 载荷超过 256 KB | BLOCK |
| `destination_unknown` | 目的地未登记 | BLOCK |
| `purpose_not_allowed` | 用途不在该目的地允许的列表中 | BLOCK |
| `empty_payload` / `ambiguous_payload` | 没有载荷 / 同时给了 text 和 json_body | BLOCK |
| `internal_error_fail_closed` | 识别、策略或 Vault 异常 | BLOCK |
| `validation_failed` | 保真校验未通过 | BLOCK |
| `secret_blocked` | 检出凭证 | BLOCK |
| `recipient_not_allowed` | 收件人不在目的地 `allowed_recipient_domains` 中 | BLOCK |
| `confidential_requires_review` | 检出机密信息 | REVIEW |
| `<type>_tokenized` | 某类实体被 Token 化，如 `customer_tokenized` | ANONYMIZE |
| `<type>_preserved_by_policy` | 某类实体按策略原样保留 | — |
| `routing_preserved` | 收件人等路由字段未改写 | — |
| `token_like_input` | 输入中已有 `[TYPE_001]` 形状的字符串，仅作标记 | — |

### 4.5 实体类型与默认处理（目的地 trust = `external_untrusted`）

| 类型 | 默认动作 | 替换为 |
|---|---|---|
| `SECRET` | BLOCK | `[BLOCKED_SECRET]`（不可恢复，不写 Vault） |
| `INTERNAL_INFO` | REVIEW | `[REDACTED_CONFIDENTIAL]`（不可恢复） |
| `CUSTOMER` `PROJECT` `PLANT` `PERSON` `ORDER_ID` `EMAIL` `CONTACT` `AMOUNT` | Token 化 | `[CUSTOMER_001]` 等 |
| `METRIC` `DEVICE_ID` | 原样保留 | — |
| 路由字段中的邮箱（`to` `cc` `bcc` `guests` `recipients` `attendees` …） | 原样保留并校验收件域 | — |

`AMOUNT` 带 `constraints: ["preserve:AMOUNT"]` 时保留。trust = `internal` 的目的地：除 `SECRET` 外全部原样保留。

#### R-20261118-M2 补充

预检所有层级JSON键名（含路由对象），凭证或租户主数据命中时直接BLOCK，不回显原键、不写Vault。普通键与值处理保持。
`sensitive_json_key`：JSON键名含凭证或租户主数据，BLOCK；此结构键预检对所有目的地适用，内部目的地的普通值保留规则不变。

### 4.6 Token 格式

`[<TYPE>_<NNN>]`，编号在 `(tenant, task, type)` 内从 001 起按首次出现顺序递增。正则：`\[([A-Z][A-Z_]*?)_(\d{3,})\]`。

## 5. `egress_restore`

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `task_id` | string | 是 | 只恢复该任务创建的 Token |
| `text` | string | 是 | 含 Token 的文本 |
| `context` | string | 否 | 默认 `local_ui`，**只允许 `local_ui`**（D1 定稿 9.6），其他取值返回 `refused_reason: "context_not_allowed"` |

```json
{"api_version": "1.0", "status": "success", "error": null,
 "data": {"text": "回复：青川重工 的 WO-559120，[CUSTOMER_009]", "restored": 2,
          "unknown_tokens": ["[CUSTOMER_009]"], "refused_reason": null}}
```

未开启恢复时：`status = "error"`，`error.code = "restore_disabled"`，`data = null`。
只恢复同租户、同任务、同 actor、未过期且未撤销的 Token；伪造或越界的 Token 原样保留并列入 `unknown_tokens`。

## 6. `egress_revoke`

参数 `task_id`。返回 `data = {"revoked": <条数>}`。撤销后该任务的密文立即擦除，无法再恢复。

## 7. 调用流程（AgentNexus 宿主）

```
工具调用 / 云端 LLM 请求（宿主拦截，模型无感知）
  → egress_evaluate(task_id=任务上下文, channel/destination_id/purpose=配置, text | json_body)
      ready_to_send = true → 用 sanitized_* 发出
                             LLM：再用 egress_restore(context=local_ui) 取 Token→原值映射，在本机恢复模型输出
      REVIEW               → 不发出，返回 E_EGRESS_REVIEW
      BLOCK                → 不发出，返回 E_EGRESS_BLOCKED
      超时 / 异常 / 校验不过 → enforce 下不发出，返回 E_EGRESS_UNAVAILABLE
```

## 8. 性能与限制

- 单条评估 P95 约 4–6 ms（不含 GLiNER，CPU）。DLP 进程冷启动需要数秒（加载 Presidio / spaCy），AgentNexus 启动时预先拉起。
- 同一 MCP 会话内串行执行。
- 没有附件和二进制解析、没有外部响应扫描、没有 `commit` 接口（见 9.5）。
- Vault 为单机 SQLite，不支持多实例共享。

## 9. D1 定稿记录（2026-11-18）

| # | 事项 | 结论 | 实现 |
|---|---|---|---|
| 9.1 | 统一返回格式 | 同意 | 第 3 节外壳 |
| 9.2 | task_id 由谁提供 | 由编排器提供；三个工具不交给模型，只由 Rust 宿主直接调用 | 参数保留，取值来自任务上下文 |
| 9.3 | channel 固定取值 | 同意，并增加 `agent`（AG.5 第 4 层：发给外部助手） | 非法取值 BLOCK |
| 9.4 | 改名 request_id | 同意 | 每次评估都返回 |
| 9.5 | `egress_commit` | **D1 不做**。AG.5 由 Rust 侧用一次性凭证（同 AG.2）绑定“展示的内容”与“实际发出的内容”；强制拦截阶段再加 | 未实现 |
| 9.6 | 恢复只在本机做 | 同意，场景为 `local_ui` | 策略 `allowed_restore_contexts: [local_ui]` |
| 9.7 | 载荷上限 256 KB | 同意（AG.5 诊断包自行控制在 32 KB 以内） | 超出 BLOCK |
| 9.8 | 接口版本号 | 同意 | `api_version: "1.0"` |
| 9.9 | 结构化返回 | 强烈同意；Rust 侧 MCP 客户端按输出 schema 校验，不过即失败、不外发 | `structured_output` + output schema |

不兼容变更（相对 v0.1）：返回改为外壳结构；`vault_ref` 改名 `request_id`；恢复默认上下文由 `task_postprocess` 改为 `local_ui`，且只允许 `local_ui`。

#### R-20261118-M5 补充

内部使用含字符串键/整数数组索引的元组路径，阻止字面点号、方括号和空键产生串字段；重复分析路径在写Vault前BLOCK。field_path仍为原格式字符串，仅用于展示，特殊键的展示歧义不再参与寻址。
`duplicate_json_path`：分析得到重复内部路径，BLOCK，不写Vault；没有新增字段或改变field_path类型。
