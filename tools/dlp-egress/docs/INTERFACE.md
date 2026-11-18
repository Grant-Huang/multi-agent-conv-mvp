# dlp-egress 调用接口说明（v0.1，供 D1 接口定稿）

本文描述 `tools/dlp-egress` **当前已实现**的调用方式（以 master `8fd24e2` 为准），并在第 9 节列出定稿前需要拍板的事项和我的建议。
示例输出均来自实际运行，耗时字段已省略。

## 1. 定位

- 职责：在内容**发出 AgentNexus 之前**判断能不能发、怎么发。
  返回 `ALLOW / ANONYMIZE / REVIEW / BLOCK` 和脱敏后的载荷。
- 不负责：实际发送、人工审批流、附件二进制解析、外部响应扫描。
- 当前接入形态：stdio MCP 服务（`dlp-egress-mcp`）。同一套逻辑也可以作为 Python 库调用（`EgressService`）。
- 当前**不是强制拦截**：调用方不调用就不会生效。强制化方案见 `docs/mcp/DLP_EGRESS_ENFORCEMENT_PLAN.md`（待审批）。

## 2. 部署与身份

| 项 | 说明 |
|---|---|
| 启动 | `dlp-egress-mcp --config <dlp.yaml>`，或用 `DLP_*` 环境变量启动，环境变量优先 |
| 可信身份 | `tenant_id`、`actor_id` 只来自服务端配置。工具参数里**没有**这两个字段，Agent 无法自报或伪造 |
| 目的地注册表 | `destinations_file`。未登记的 `destination_id` 一律 BLOCK |
| 主数据 | `master_data_file`（客户、项目、厂区、人员名单），可选 |
| Vault | `vault_path` + `vault_key_file`（32 字节 base64）。缺少密钥时拒绝启动 |
| 启动失败 | 缺少身份或密钥、配置非法：进程以退出码 2 退出，不会以不安全的配置运行 |

AgentNexus 配置（`config.toml`）：

```toml
[[mcp.servers]]
id = "dlp"
transport = "stdio"
command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
args = ["--config", "tools/dlp-egress/config/dlp.example.yaml"]
timeout_ms = 30000
[[mcp.servers.tools]]
name = "egress_evaluate"
capability_id = "dlp.egress_evaluate"
description = "外发前检查载荷：返回 ALLOW/ANONYMIZE/REVIEW/BLOCK 与脱敏载荷"
effect = "workspace_write"          # evaluate 会写本地 Vault
input_types = ["application/json"]
output_types = ["application/json"]
```

## 3. 工具一览

| 工具 | 用途 | 谁可以调用 |
|---|---|---|
| `egress_evaluate` | 外发前评估并脱敏 | Agent / 编排器 |
| `egress_restore` | 把响应里的 Token 换回原值 | **默认关闭**（`allow_agent_restore: false`）。建议只由本地可信 UI 或编排器后处理调用 |
| `egress_revoke` | 撤销某任务全部 Token 映射并擦除密文 | 编排器（任务结束或取消时） |

所有工具都以**一段 JSON 字符串**作为 MCP 文本内容返回（`structured_output=false`）。

## 4. `egress_evaluate`

### 4.1 参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `task_id` | string | 是 | Token 作用域。同一任务内同一实体得到同一 Token，不同任务相互隔离 |
| `channel` | string | 是 | `email` / `slack` / `external_api` / `file_upload` / `mcp`。**当前只写入审计，不参与判定，也不校验取值** |
| `destination_id` | string | 是 | 必须在目的地注册表中，否则 BLOCK |
| `purpose` | string | 是 | 用途。若目的地配置了 `purposes`，不在列表中就 BLOCK |
| `text` | string \| null | 二选一 | 自由文本载荷 |
| `json_body` | object \| array \| null | 二选一 | 结构化载荷（工具参数、API 请求体）。键名和非字符串值保持不变 |
| `constraints` | string[] \| null | 否 | 目前只支持 `preserve:<TYPE>`，例如 `preserve:AMOUNT`。只有策略允许的组合才生效，`SECRET`、`INTERNAL_INFO` 永远不能被申请保留 |

`text` 与 `json_body` 同时为空或同时给出：BLOCK（`empty_payload` / `ambiguous_payload`）。

### 4.2 返回

```json
{
  "decision": "ANONYMIZE",
  "policy_version": "egress-v1",
  "sanitized_text": null,
  "sanitized_json": {
    "to": "buyer@acme-demo.com",
    "client": "[CUSTOMER_001]",
    "order": "[ORDER_ID_001]",
    "yield": "良率97.5%"
  },
  "entities": [
    {"type": "EMAIL", "start": 0, "end": 19, "score": 0.9, "source": "rule", "field_path": "to"},
    {"type": "CUSTOMER", "start": 0, "end": 4, "score": 0.99, "source": "master_data", "field_path": "client"},
    {"type": "ORDER_ID", "start": 0, "end": 9, "score": 0.85, "source": "rule", "field_path": "order"},
    {"type": "METRIC", "start": 0, "end": 7, "score": 0.8, "source": "rule", "field_path": "yield"}
  ],
  "reason_codes": ["customer_tokenized", "metric_preserved_by_policy", "order_id_tokenized", "routing_preserved"],
  "tokens": ["[CUSTOMER_001]", "[ORDER_ID_001]"],
  "vault_ref": "req_f4b4df789909484a",
  "validation": {"schema_ok": true, "preserved_values_ok": true, "no_secret_in_output": true, "routing_unchanged": true},
  "ready_to_send": true,
  "elapsed_ms": 4.1
}
```

（请求：`json_body = {"to":"buyer@acme-demo.com","client":"青川重工","order":"WO-559120","yield":"良率97.5%"}`）

| 字段 | 说明 |
|---|---|
| `decision` | `ALLOW` / `ANONYMIZE` / `REVIEW` / `BLOCK`，含义见 4.3 |
| `policy_version` | 判定所用的策略版本，审计时用 |
| `sanitized_text` / `sanitized_json` | 与请求载荷类型对应，另一个为 null。BLOCK 且未进入识别阶段（目的地未登记、空载荷、内部异常）时两者都是 null |
| `entities` | 识别出的实体：类型、偏移（Python Unicode 码点，相对于该字段原文）、分数、来源（`rule` / `master_data` / `detect_secrets` / `gliner`）、JSON 字段路径。**不含原文** |
| `reason_codes` | 判定原因，见 4.4 |
| `tokens` | 本次使用的可逆 Token（不含原值） |
| `vault_ref` | 当前实现是本次请求 ID，**不能**用来查映射（见 9.4） |
| `validation` | 保真校验结果，任一为 false 时 decision 强制为 BLOCK |
| `ready_to_send` | 仅当 decision 为 ALLOW 或 ANONYMIZE 且校验全部通过时为 true |

### 4.3 决策语义与调用方义务

| decision | 含义 | 调用方必须做的事 |
|---|---|---|
| `ALLOW` | 无敏感实体，或只有按策略可以原样保留的实体 | 发送 `sanitized_*`（与原文相同） |
| `ANONYMIZE` | 标识符已替换为 Token | **只发送 `sanitized_*`，不能发原文**。外部返回的内容如需恢复，走 `egress_restore` |
| `REVIEW` | 含机密信息（`[REDACTED_CONFIDENTIAL]`） | **不发送**。转人工审批。`sanitized_*` 只是给审批人看的预览 |
| `BLOCK` | 凭证、未登记目的地、校验失败、内部异常等 | **不发送**。若原因是 `secret_blocked`，通知用户尽快更换（轮换）被检出的密钥 |

一句话规则：**`ready_to_send` 为 true 才发，且只发 `sanitized_*`。**

### 4.4 reason_codes

| 代码 | 触发条件 | 对应决策 |
|---|---|---|
| `destination_unknown` | 目的地未登记 | BLOCK |
| `purpose_not_allowed` | 用途不在该目的地允许的列表中 | BLOCK |
| `empty_payload` / `ambiguous_payload` | 没有载荷 / 同时给了 text 和 json_body | BLOCK |
| `internal_error_fail_closed` | 识别、策略或 Vault 异常 | BLOCK |
| `validation_failed` | 保真校验未通过 | BLOCK |
| `secret_blocked` | 检出凭证 | BLOCK |
| `recipient_not_allowed` | 路由收件人不在目的地 `allowed_recipient_domains` 中 | BLOCK |
| `confidential_requires_review` | 检出机密信息 | REVIEW |
| `<type>_tokenized` | 某类实体被 Token 化，如 `customer_tokenized` | ANONYMIZE |
| `<type>_preserved_by_policy` | 某类实体按策略原样保留，如 `metric_preserved_by_policy` | — |
| `routing_preserved` | 收件人等路由字段未改写 | — |
| `token_like_input` | 输入中已有 `[TYPE_001]` 形状的字符串（可能是二次外发，也可能是 Token 注入），仅作标记 | — |

### 4.5 实体类型与默认处理（目的地 trust = `external_untrusted`）

| 类型 | 默认动作 | 替换为 |
|---|---|---|
| `SECRET` | BLOCK | `[BLOCKED_SECRET]`（不可恢复，不写 Vault） |
| `INTERNAL_INFO` | REVIEW | `[REDACTED_CONFIDENTIAL]`（不可恢复） |
| `CUSTOMER` `PROJECT` `PLANT` `PERSON` `ORDER_ID` `EMAIL` `CONTACT` `AMOUNT` | Token 化 | `[CUSTOMER_001]` 等，任务内稳定、可恢复 |
| `METRIC` `DEVICE_ID` | 原样保留 | — |
| 路由字段中的邮箱（`to` `cc` `bcc` `guests` `recipients` `attendees` …） | 原样保留并校验收件域 | — |

`AMOUNT` 默认 Token 化，带 `constraints: ["preserve:AMOUNT"]` 时保留。trust = `internal` 的目的地：除 `SECRET` 外全部原样保留。
策略文件：`src/agentnexus_dlp/default_policy.yaml`，可用 `policy_file` 覆盖。

### 4.6 Token 格式

`[<TYPE>_<NNN>]`，例如 `[CUSTOMER_001]`。编号在 `(tenant, task, type)` 范围内从 001 起，按首次出现的顺序递增。正则：`\[([A-Z][A-Z_]*?)_(\d{3,})\]`。

## 5. `egress_restore`

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `task_id` | string | 是 | 只恢复该任务创建的 Token |
| `text` | string | 是 | 含 Token 的文本，例如外部系统的返回 |
| `context` | string | 否 | 默认 `task_postprocess`。必须在策略的 `allowed_restore_contexts` 中（默认 `local_ui`、`task_postprocess`） |

返回（未开启时）：`{"status":"error","code":"restore_disabled","message":"…"}`

返回（开启时）：

```json
{"status": "success", "text": "供应商回复：青川重工 的 WO-559120 已处理，[CUSTOMER_009] 待定",
 "restored": 2, "unknown_tokens": ["[CUSTOMER_009]"], "refused_reason": null}
```

- 只有同租户、同任务、同 actor、上下文允许、未过期且未撤销的 Token 才会被恢复。
- 伪造或越界的 Token 原样保留，并列入 `unknown_tokens`。
- `context` 不允许时返回 `refused_reason: "context_not_allowed"`，`restored: 0`。

## 6. `egress_revoke`

参数：`task_id`。返回 `{"status":"success","revoked":<条数>}`。撤销后该任务的密文立即擦除，之后无法再恢复。建议在任务结束、取消或删除时调用。

## 7. 推荐调用流程

```
Agent 要外发
  → egress_evaluate(task_id, channel, destination_id, purpose, text | json_body)
      ready_to_send = true  → 用 sanitized_* 调用真实外发工具 → 外部返回
                               → （可选）egress_restore(task_id, 返回文本)，仅在可信后处理中
      decision = REVIEW     → 停止外发，生成审批单（展示 sanitized_* 预览 + reason_codes）
      decision = BLOCK      → 停止外发，向用户说明 reason_codes
任务结束 → egress_revoke(task_id)
```

## 8. 性能与限制（当前实现）

- 单条评估 P95 约 6 ms（不含 GLiNER，CPU，≤ 200 字文本）。700 条评测的实测数据见 `bench/results/SUMMARY.md`。
- 同一个 MCP 服务实例内串行执行（AgentNexus 对同一服务串行调用）。
- 没有载荷大小上限、没有附件和二进制解析、没有外部响应扫描。
- Vault 为单机 SQLite，不支持多实例共享。

## 9. D1 定稿需要拍板的事项（附建议）

| # | 问题 | 现状 | 建议 |
|---|---|---|---|
| 9.1 | 返回外壳不统一 | evaluate 直接返回结果对象；restore 和 revoke 带 `status` 字段 | 统一为 `{"status":"success\|error","data":{…},"error":{code,message}}`，与 `tools/websearch` 一致 |
| 9.2 | `task_id` 由谁提供 | Agent 在参数里传 | 改为由编排器注入（与 tenant/actor 同级的可信上下文），防止 Agent 借用其他任务的 Token 空间 |
| 9.3 | `channel` 取值 | 自由字符串，不校验 | 定为枚举并校验；不同通道可以有不同的默认策略 |
| 9.4 | `vault_ref` | 实为请求 ID | 改名为 `request_id`，用于审计关联；不暴露 Vault 内部引用 |
| 9.5 | 审批与发送的绑定 | 无 `commit` 接口，evaluate 后内容可能被调包（TOCTOU） | 新增 `egress_commit(request_id, payload_hash, approval_id?)`，由执行侧校验 hash、目的地、有效期和幂等键（设计文档第 7 节） |
| 9.6 | 恢复的调用方 | Agent 可调用（默认关闭） | 恢复只在编排器后处理或本地 UI 中进行，不向 General 暴露 |
| 9.7 | 载荷大小上限 | 无 | 定一个上限（如 256 KB），超出时 BLOCK 或转异步 |
| 9.8 | 接口版本 | 无显式版本字段 | 返回中加 `api_version`，工具名保持不变，按版本做兼容 |
| 9.9 | 结构化返回 | 文本中的 JSON 字符串 | 启用 MCP `structured_output`，提供 output schema，便于 AgentNexus 校验 |

定稿后，我会按结论修改实现与测试，并同步更新本文档。
