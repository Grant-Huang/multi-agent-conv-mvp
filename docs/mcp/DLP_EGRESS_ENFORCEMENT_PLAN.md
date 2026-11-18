# 方案：把外发检查变成必经步骤（待审批）

> 状态：**方案稿，等待审批，尚未修改任何代码。** 基线代码：master `8fd24e2`。
> 依据：AGENTS.md“先停下来问人”（涉及对外发送、权限策略；第三阶段涉及 `orchestrator.rs` / `store.rs`）。

## 1. 目标与验收标准

**目标**：AgentNexus 中经过它的每一次外发，都必须先经 dlp-egress 判定。Agent 无法跳过检查，也无法自己声明“已授权”。

**首期验收标准**（第一阶段，见第 4 节）：

1. General 调用任何被登记为“外发”的工具时，宿主（Rust）先调用 DLP。只有 `ready_to_send=true` 时，才用**脱敏后的参数**调用真实工具。
2. 检查结果为 REVIEW 或 BLOCK、DLP 超时或不可用时，真实工具**不会被调用**，Agent 收到明确的错误码。
3. DLP 工具本身不出现在 General 的工具清单里，Agent 无法调用 `egress_restore`。
4. `tenant_id`、`actor_id`、`task_id`、`destination_id`、`channel`、`purpose` 全部由宿主根据配置和任务上下文填写，不来自模型输出。
5. 默认配置（`mode = "off"`）下现有行为**完全不变**，现有测试全部通过。
6. 每次判定写一条结构化日志（不含正文），关联 `task_id`。

## 2. 现状：AgentNexus 的外发路径

| 编号 | 路径 | 代码位置 | 发出了什么 | 现状 | 风险 |
|---|---|---|---|---|---|
| E1 | 网页搜索 / 抓取工具 | `crates/agents/src/tools.rs`（`WebSearch`、`WebFetch`，子进程调用 websearch） | 查询词、URL | 无检查 | **中高**：查询词里可能带客户名、工单号 |
| E2 | MCP 工具调用参数 | `crates/agents/src/mcp.rs`（`McpTool::call`） | 工具参数；HTTP 远程服务的参数直接出网 | `external_write` / `destructive` 一律拒绝；`read_only` 不检查参数 | **中**：只读工具的参数同样会外发 |
| E3 | LLM 调用 | `crates/llm/src/backend.rs` | 完整对话上下文 | 默认后端是本机 llama.cpp（127.0.0.1），不算外发；配置云端后端时才外发 | 配置云端时**最高** |
| E4 | 编码 agent（ACP 子进程） | `crates/agents/src/acp.rs`、`coding.rs` | 任务描述、代码，由子进程发往其服务商 | 宿主看不到子进程的网络请求 | **高**，只能在派发前检查或做网络层限制 |
| E5 | 远程节点 / A2A | `crates/agents/src/remote.rs`、`a2a.rs` | 任务内容 | 节点需配对认证 | 低到中：属于企业内部节点时可按 `internal` 处理 |
| E6 | 将来的外部写工具（邮件、Slack 等） | 尚不存在 | — | MCP 中 `external_write` 被拒绝 | 开放之前**必须**先有本方案 |

关键事实：General 的全部工具调用（内置 + MCP）都经过 `ToolRegistry::call(name, args)`（`tools.rs`），这是理想的强制执行点（PEP）。

## 3. 设计原则

- **宿主强制，不靠提示词**：检查发生在 Rust 宿主中，Agent 的输出只决定“想做什么”，不能决定“是否检查”。
- **判定与执行分离**：dlp-egress 只做判定（PDP）。宿主负责执行或不执行（PEP），并且只把**脱敏后的参数**交给真实工具。
- **可信上下文由宿主注入**：身份、任务、目的地、通道、用途都来自配置和任务上下文。
- **Fail closed**：`enforce` 模式下，DLP 超时、崩溃或返回非法 JSON 一律视为 BLOCK。
- **可灰度**：`off`（不检查）→ `audit`（检查并记录，不拦截）→ `enforce`（拦截）。

## 4. 分阶段方案

### 第一阶段（建议首期）：工具调用守卫（E1、E2、E6）

不涉及 AGENTS.md 中的高风险文件，但修改 `config.toml` 字段（公开接口），需要你批准。

**改动：**

1. `crates/agents/src/egress.rs`（新增）
   - `EgressGuard`：持有一个**专用**的 DLP MCP 会话（复用 `mcp.rs` 现有的 stdio 客户端与超时、清理逻辑）。这个会话不注册进 General 的工具表。
   - `EgressBinding { destination_id, channel, purpose }`，来自配置。
   - `EgressCtx { tenant_id, actor_id, task_id }`，由宿主填写。
   - `evaluate(ctx, binding, args) -> Verdict`：`Send(sanitized_args)` / `Review(reason_codes)` / `Block(reason_codes)`。
2. `crates/agents/src/tools.rs`
   - `ToolRegistry` 增加 `guard: Option<Arc<EgressGuard>>` 与 `bindings: BTreeMap<工具名, EgressBinding>`。
   - 新增 `call_with_ctx(ctx, name, args)`：工具有外发绑定时，先 `evaluate`；`Send` 时用脱敏参数调用工具，否则返回 `E_EGRESS_BLOCKED` / `E_EGRESS_REVIEW` / `E_EGRESS_UNAVAILABLE`，不调用工具。
   - `enforce` 模式下，**启动时校验**：所有 effect 为 `external_write` 的工具必须有外发绑定，否则拒绝启动。之后才允许放开 MCP 中对 `external_write` 的拒绝（单独一步，见第三阶段）。
3. `crates/agents/src/general.rs`：工具调用改为 `call_with_ctx`，传入当前 `TaskSpec` 的 `task_id`。
4. `crates/runtime/src/config.rs`：解析新增的 `[egress]` 配置（见第 5 节），并补充配置解析测试。
5. `crates/agents/src/mcp.rs`：把会话管理抽成可供 `EgressGuard` 复用的函数，不改变现有工具的行为。

**REVIEW 在首期的处理**：返回 `E_EGRESS_REVIEW` 给 Agent，Agent 必须如实告诉用户“需要审批”。真正的审批流放在第三阶段，因为它需要改任务状态机。

### 第二阶段：LLM 外发（E3）

只在配置了云端后端时启用。

- `crates/llm/src/backend.rs`：后端配置增加 `egress` 绑定。请求发出前，把 messages 里的文本交给 DLP（`json_body` 形式，保留结构）；BLOCK 或 REVIEW 时这次调用失败，返回 `E_EGRESS_BLOCKED`。
- 模型返回中的 Token 由宿主在本地恢复后再交给 Agent。恢复仅在宿主内部进行，不经过 Agent。
- 风险：流式输出中 Token 可能被切断，需要做缓冲；每次调用增加约 5–10 ms。建议先以 `audit` 模式运行一周，观察误拦截情况。

### 第三阶段：审批流与执行绑定（高风险，届时另附详细方案）

- REVIEW → 任务进入 `NEEDS_HUMAN`，生成审批单。审批通过后由宿主执行，并做 `commit` 校验：载荷哈希、目的地、有效期、幂等键，防止检查之后内容被调包（TOCTOU）。
  涉及 `crates/runtime/src/orchestrator.rs`、`store.rs`（新增审批表及迁移）、HTTP API 和前端。
- 放开 MCP `external_write`：只允许有外发绑定、且在 `enforce` 模式下的工具。
- 编码 agent（E4）：派发前把任务描述交给 DLP 检查（改 `orchestrator.rs` 的派发路径）。

### 第四阶段：网络层兜底（部署侧）

应用层守卫拦不住子进程（ACP agent、stdio MCP 服务）自己联网。生产部署需要：出站代理加白名单，只允许受控连接器的域名出网；运行 AgentNexus 的用户或容器默认禁止直连公网。这一阶段写部署文档和检查脚本，不改业务代码。

## 5. 配置草案（新增字段，需批准）

```toml
[egress]
mode = "off"                     # off / audit / enforce，默认 off，行为不变
tenant_id = "demo-tenant"        # 可信身份，来自部署配置
actor_id = "agentnexus-general"
timeout_ms = 2000                # 超时视为 BLOCK（enforce）或仅记录（audit）
dlp_command = "tools/dlp-egress/.venv/bin/dlp-egress-mcp"
dlp_args = ["--config", "tools/dlp-egress/config/dlp.example.yaml"]

[[egress.bindings]]              # 哪些工具属于外发、发往哪里
tool = "web_search"
destination_id = "public-search"
channel = "external_api"
purpose = "web_research"

[[egress.bindings]]
tool = "vendor.create_ticket"    # MCP 工具名（服务 id.工具名）
destination_id = "vendor-api"
channel = "mcp"
purpose = "vendor_diagnostics"
```

相应地，dlp-egress 的 `destinations` 中需要登记 `public-search` 等目的地。

## 6. 超时、取消与清理

- 每次判定都有 `timeout_ms` 上限。超时后丢弃该 DLP 会话并终止子进程，下次调用时重新拉起，与现有 MCP 的处理方式一致。
- 任务取消：正在等待判定的调用直接返回取消，不调用真实工具。
- DLP 子进程随 AgentNexus 退出而终止，复用 `proc_group.rs` 的进程组清理。
- `enforce` 模式下 DLP 启动失败：AgentNexus 启动失败（不会以不安全的配置运行）。`audit` 模式下只记录 WARN。

## 7. 日志与错误码（需补充到 DESIGN.md 第 10 节）

| 事件 | 级别 | 字段 |
|---|---|---|
| `egress.evaluate` | INFO | `task_id`、`tool`、`destination_id`、`decision`、`reason_codes`、`elapsed_ms`、`mode` |
| `egress.blocked` | WARN | 同上，带 `code` |
| `egress.guard_unavailable` | ERROR | `code=E_EGRESS_UNAVAILABLE`、原因摘要 |

错误码：`E_EGRESS_BLOCKED`、`E_EGRESS_REVIEW`、`E_EGRESS_UNAVAILABLE`。日志中不记录正文和参数值。

## 8. 测试计划（先写测试，Red → Green）

| 层级 | 用例 |
|---|---|
| 单元（`egress.rs`） | 判定结果映射；超时 → BLOCK；DLP 返回非法 JSON → BLOCK；`audit` 模式不拦截 |
| 集成（`an-agents`） | 用本地 mock DLP（复用 MCP 测试 fixture 的写法）：带客户名的 `web_search` 调用，真实工具收到的是脱敏参数；含凭证时真实工具**未被调用**；DLP 进程被杀后调用失败且不外发 |
| 集成（`an-server`） | `/api/capabilities` 中没有 DLP 工具；`enforce` 下 `external_write` 工具缺少绑定时启动失败 |
| 端到端 | 按 `docs/testing/README.md`：受控替身模型让 General 发起一次搜索，断言任务证据中出现 `E_EGRESS_BLOCKED`，真实搜索命令未执行 |
| 回归 | `mode = "off"` 时全部现有测试通过；`cargo clippy` 无警告；`npm run build` 通过 |

## 9. 风险与回滚

| 风险 | 缓解 |
|---|---|
| 误拦截影响正常使用 | 先 `audit` 再 `enforce`；按 `egress.evaluate` 日志统计误拦截 |
| 延迟增加 | 每次工具调用增加约 5–10 ms（不含 GLiNER）；开启 GLiNER 后需重新测量 |
| DLP 故障导致不可用 | `enforce` 模式下这是预期的 fail closed；提供健康检查和重启策略 |
| 配置错误导致漏检 | `enforce` 下启动时校验 `external_write` 工具必须有绑定；未绑定的只读工具在 `audit` 日志中列出，供人工确认 |
| 回滚 | 配置改为 `mode = "off"` 立即恢复原有行为；代码回滚为单个 PR 的 revert，不涉及数据库迁移（第一阶段） |

## 10. 请你决定

1. **首期范围**：只做第一阶段（推荐），还是第一、二阶段一起做？
2. **默认模式**：代码默认 `off`（推荐）；你们的部署先用 `audit` 还是直接 `enforce`？
3. **哪些工具算外发**：建议首期绑定 `web_search`、`web_fetch` 和所有 HTTP 远程 MCP 工具；stdio 本地 MCP 工具逐个确认。
4. **云端 LLM**：如果生产只用本机 llama.cpp，第二阶段可以延后。
5. **D1 接口定稿**的结论（`docs/INTERFACE.md` 第 9 节），尤其是统一返回外壳和 `task_id` 由宿主注入这两项，会直接影响第一阶段的实现。建议先定稿接口，再开始第一阶段。

批准后，我会先提交第一阶段的测试用例（Red），再写实现。
