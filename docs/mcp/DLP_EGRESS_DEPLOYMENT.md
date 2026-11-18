# 外发检查：部署与使用说明

对应实现：必经化方案第一、二阶段（`docs/mcp/DLP_EGRESS_ENFORCEMENT_PLAN.md`）。
接口约定见 `tools/dlp-egress/docs/INTERFACE.md`（D1 定稿，api_version 1.0）。

## 1. 它做什么

AgentNexus 宿主（Rust）在两类外发发生之前，先交给 dlp-egress 判定：

| 路径 | 何时检查 | enforce 下的处理 |
|---|---|---|
| 工具调用 | General（含报告流程）调用 `[[egress.tools]]` 中登记的工具前 | 只把脱敏后的参数交给工具；REVIEW / BLOCK / DLP 异常时不调用工具 |
| 云端 LLM | 发往 `[[egress.llm]]` 中登记的后端的每一次请求前 | 只发送脱敏后的消息；模型输出中的 Token 在本机恢复后再交给调用方；不通过时不发出任何请求 |

- DLP 的三个工具由宿主直接调用，**不出现在模型可见的工具清单里**，模型无法调用或绕过。
- `task_id` 来自任务上下文，`channel / destination_id / purpose` 来自配置，均不来自模型输出。
- 三种模式：`off`（默认，不启动 DLP，行为与以前完全相同）、`audit`（检查并记录，从不拦截）、`enforce`（拦截）。

## 2. 部署步骤

### 2.1 安装 dlp-egress 并生成密钥

在仓库根目录：

```bash
cd tools/dlp-egress && uv sync --extra mcp && cd ../..
```

```bash
mkdir -p tools/dlp-egress/.data && python3 -c "import base64,os;print(base64.urlsafe_b64encode(os.urandom(32)).decode())" > tools/dlp-egress/.data/dlp-vault.key && chmod 600 tools/dlp-egress/.data/dlp-vault.key
```

### 2.2 dlp-egress 配置（宿主专用）

复制 `tools/dlp-egress/config/dlp.example.yaml`，按下面修改：

```yaml
tenant_id: <你的租户>
actor_id: agentnexus-host
vault_path: ../.data/dlp-vault.sqlite3
vault_key_file: ../.data/dlp-vault.key
destinations_file: destinations.yaml     # 登记所有目的地（见 2.3）
master_data_file: master_data.yaml       # 客户 / 项目 / 厂区 / 人员名单，强烈建议提供
allow_restore: true                      # 宿主需要恢复云端 LLM 输出中的 Token；模型接触不到这个工具
```

`allow_restore: false` 时，LLM 请求照常脱敏，但模型输出里会保留 `[CUSTOMER_001]` 这样的 Token
（日志 `egress.restore_unavailable`），不影响安全，只影响可读性。

### 2.3 登记目的地

`destinations.yaml` 中登记 `[egress]` 绑定用到的每个 `destination_id`，例如：

```yaml
destinations:
  public-search:  {trust: external_untrusted}   # web_search / web_fetch
  cloud-llm:      {trust: external_untrusted}   # 云端模型
  vendor-api:     {trust: external_untrusted}
```

未登记的目的地一律 BLOCK。

### 2.4 AgentNexus 配置（`config.toml`）

```toml
[egress]
mode = "audit"                 # 先 audit，见第 3 节；代码默认 off
timeout_ms = 2000              # 单次判定上限；enforce 下超时不发出
dlp_command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
dlp_args = ["--config", "tools/dlp-egress/config/dlp.host.yaml"]

[[egress.tools]]               # 内置工具名；MCP 工具写 服务id.工具名
tool = "web_search"
destination_id = "public-search"
channel = "external_api"
purpose = "web_research"

[[egress.tools]]
tool = "web_fetch"
destination_id = "public-search"
channel = "external_api"
purpose = "web_research"

[[egress.llm]]                 # 非本机地址的 LLM 后端；没有 [backends] 时名字是 default
backend = "cloud"
destination_id = "cloud-llm"
purpose = "llm_inference"
```

`channel` 只能是 `email / slack / external_api / file_upload / mcp / agent`。LLM 绑定的 channel 固定为 `external_api`。

### 2.5 启动校验

| 情况 | audit | enforce |
|---|---|---|
| 绑定的工具或后端不存在（拼写错误） | 拒绝启动 | 拒绝启动 |
| `web_search` / `web_fetch`、HTTP 远程 MCP 工具、`external_write` 工具没有绑定 | 警告 `config.egress`（`W_EGRESS_UNBOUND`） | 拒绝启动 |
| LLM 角色实际使用的地址不是本机，且所属后端没有绑定 | 警告 | 拒绝启动 |
| DLP 进程无法启动 | 记录 `egress.guard_unavailable`，继续运行 | 拒绝启动 |

本机地址指 `127.0.0.0/8`、`localhost`、`::1`。stdio 本地 MCP 工具默认视为本机工具，不要求绑定；若它会联网，请手动登记。

## 3. 先用 audit：注意事项

`audit` 用来在不影响业务的前提下观察“如果开启拦截会发生什么”。上线前请逐条确认：

1. **audit 不提供保护。** 所有内容仍按原文外发，只多了判定记录。审计期间不能对外宣称“已脱敏”或“已受控”。
2. **按日志看“本来会被拦截的”。** 每次判定一条 `egress.evaluate`，含 `decision`、`reason_codes`、`request_id`、`path`、`elapsed_ms`。
   audit 下不会出现 `egress.blocked`。统计 `decision` 为 `BLOCK` / `REVIEW` 的记录，就是切换到 enforce 后会被拦下的调用。
3. **逐条复核误拦。** 对 `BLOCK` / `REVIEW` 抽样，按 `request_id` 回到业务场景判断是否误判，结果用于完善主数据和策略。
   AgentNexus 日志不含正文，复核需要在业务侧找到原始内容。
4. **识别能力有限，必须有人复核。** 2026-11-18 用 1000 条模拟测试集评测：凭证 200 个漏了 152 个，应拦截却放行 141 条，机密信息的实体识别几乎为 0（详见 PLAN.md）。
   这意味着 enforce 也**不能**被当作完整保护，规则改进之前不能放松其他管控。
5. **清掉所有 `W_EGRESS_UNBOUND` 警告。** 这些在 enforce 下会导致拒绝启动。
6. **关注延迟。** 每次登记工具的调用、每次云端 LLM 请求都多一次判定（`elapsed_ms`，通常几毫秒到几十毫秒）。DLP 冷启动需要数秒，在 AgentNexus 启动时完成。
7. **关注可用性。** 统计 `egress.guard_unavailable`。audit 下 DLP 故障只记录不影响业务，但到了 enforce，同样的故障会让外发全部失败。
8. **Vault 会增长。** audit 下 DLP 也会为 Token 建立映射（加密存储，TTL 默认 24 小时），请保护好 `vault_key_file`，并留意磁盘占用。
9. **DLP 自身的审计日志。** 由 AgentNexus 拉起时，DLP 的标准错误输出没有保存（沿用 MCP 子进程的处理方式），
   所以 DLP 侧的 `dlp.evaluate` 审计日志看不到。排查以 AgentNexus 的 `egress.*` 日志为准（见第 7 节待办）。

**切换到 enforce 的建议条件**：连续一周无 `W_EGRESS_UNBOUND` 和 `egress.guard_unavailable`；误拦率经复核可接受；
用户知道被拦截时会看到什么（第 4 节）；回滚方式已演练（第 6 节）。

## 4. enforce 下用户会看到什么

| 错误码 | 含义 | 发生了什么 |
|---|---|---|
| `E_EGRESS_BLOCKED` | 内容不允许外发（凭证、未登记目的地等） | 工具没有调用 / 模型请求没有发出。General 会把错误作为工具结果继续推理，或任务失败 |
| `E_EGRESS_REVIEW` | 含机密信息，需要人工审批 | 同上。**目前没有审批流**（第三阶段），需要用户改写内容后重试 |
| `E_EGRESS_UNAVAILABLE` | DLP 超时、崩溃或返回不合规 | 同上（fail closed）。DLP 会在下一次调用时自动重新拉起 |

错误信息中带 `request_id` 和原因码，不带原文。

## 5. 覆盖范围（以及没有覆盖的）

已覆盖：General 的全部工具调用（含报告流程的并行检索）、所有经 `Backends` 创建的 LLM 客户端（前台对话、路由、回复、General、审查）。

**没有覆盖**，需要另外的管控：

| 路径 | 说明 |
|---|---|
| 编码 agent（ACP、Codex、pi 等子进程） | 子进程自己联网，宿主看不到其请求（方案第三、四阶段） |
| stdio 本地 MCP 服务自身的网络访问 | 只检查发给它的参数，不检查它自己再发往哪里 |
| 远程节点 / A2A | 视为企业内部节点，未接入检查 |
| 独立的 `an-mom` 服务 | 不在 AgentNexus 主进程内 |
| 工具返回结果 | 例如搜索结果中回显的 Token 会进入模型上下文（不泄露原文，但模型可能把 Token 写进回答） |
| 网络层 | 应用层守卫拦不住绕过宿主的出网（方案第四阶段：出站代理白名单） |
| 展示与发送的绑定 | D1 不做 `egress_commit`；AG.5 由 Rust 侧一次性凭证保证“展示什么就发什么” |

不在任务中的 LLM 调用（如前台对话）每次调用使用独立的 Token 作用域。

## 6. 回滚

把 `[egress] mode` 改为 `"off"` 并重启，立即恢复原有行为（不启动 DLP，不做任何检查）。不涉及数据迁移。

## 7. 待办

- DLP 审计日志落盘（或转发到 AgentNexus 日志）。
- 第三阶段：REVIEW → 人工审批 → 执行；放开 MCP `external_write`；编码 agent 派发前检查。
- 第四阶段：出站代理白名单与部署检查脚本。

## 附：助手排查（AG.5 第 3、4 层）也使用这个守卫

- 「助手」页签的"排查"里，诊断包（第 3 层）和"请其他助手帮忙"（第 4 层）复用同一个 `[egress]` 守卫，不另起 DLP 进程（docs/DECISIONS.md 第 41 条）。
- `mode = "off"` 时第 4 层不可用；`audit` 或 `enforce` 时可用。**不论哪种模式，排查都只在 DLP 返回 `ready_to_send = true` 时发送**，并且只发脱敏后的文本。
- 需要在 `destinations.yaml` 登记：
  - 第 4 层每个可能当帮手的助手 `agent:<助手 id>`（例如 `agent:claude`），purposes 含 `diagnose`；
  - 第 3 层 `user:clipboard`。渠道 `clipboard` 在 D1 第 9.3 项里已确认，但工具目前还不支持；工具返回 `invalid_channel` 时，诊断包按"未经检查"给出本机处理过的版本。
- 帮手回复用 `egress_restore`（context `local_ui`）还原后只用于显示，因此 DLP 配置需要 `allow_restore: true`。每次排查结束后宿主调用 `egress_revoke` 撤销映射。
