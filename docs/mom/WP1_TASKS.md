# WP1（DES-MOM-会话闭环）执行任务卡

本文件是 [INTEGRATION_PLAN.md](../INTEGRATION_PLAN.md) 第三节的执行版。运营桌面/运营频道（D0–D3a）单独见 [OPS_DESK_TASKS.md](OPS_DESK_TASKS.md)，**通用流程、完成的定义、命令和环境坑都在那个文件的第 2 节，本文件不重复**，每张卡开工前先读它。  
产品壳主轴见 [AI Desktop V1.7](../ai-desktop/DESIGN.md)；与 WP1 的挂靠见 [ai-desktop/GAPS.md](../ai-desktop/GAPS.md) 与 INTEGRATION_PLAN §0。  
已确认的设计决定：[DECISIONS.md](../DECISIONS.md) 第 56 条（DES–MOM–会话闭环取方案 A：会话只读工具 + 案例卡；MOM 数据来自 DES 事件，暂不用 MOM 界面）、57（案例卡只在需要时推送，桌面最小化）、58（专属运营频道）、60（频道卡批准）。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| L0 | `case_id` 关联与日志字段 | 未开始 | — |
| L1-0 | **先出方案**：引擎句柄如何注入会话工具（见 L1） | 未开始，需用户确认 | L0 |
| L1-1 | 只读工具 `mom_cases`、`mom_factory_status` | 未开始 | L1-0 |
| L1-2 | `mom_forecast`、`mom_whatif` | 未开始 | L1-1 |
| L2 | 案例卡 + 在卡片上批准 | **已并入 D2**（OPS_DESK_TASKS §7 / #136）；会话内按需发卡留待 L1 之后 | D1 |
| L3 | 闭环回写与审核报告卡 | 未开始 | D2 |
| L4 | 经 `an-des-mcp` 替换进程内 DES 调用 | 未开始；**与 D3a 解耦，先出方案** | L3 可并行准备 |
| GS1 | 黄金场景 E2E 总装 | 未开始；UI 入口逐步迁 Desktop 运营区 | L1–L3 |

## 1. 开工前必读的现状

- MOM 引擎在 `crates/runtime/src/mom/engine.rs`（**高风险文件，不要改**；只用它的公开 API：`ingest`、`approve`、`execute`、`close`、`advance`、`human_view`、`agent_view`、`case`、`case_ids`、`now`、`pending_events`）。
- 进程装配顺序（`crates/server/src/main.rs`）：**先**第 35 行 `Runtime::with_mcp_diagnostics(...)` 建运行时（工具注册表在 `runtime/src/lib.rs` 的私有 `assemble` 里组装，General 的工具来自 `ToolRegistry::with_builtin` + `mcp_tools`），**后**第 65–77 行才创建 MOM `Engine`（`<db_path>.mom.db`）并挂载 `/mom`。所以现在会话工具拿不到引擎，这是 L1 的核心障碍。
- 引擎状态在 `an_server::mom::AppState{engine: Arc<Mutex<Engine>>}`，属于 server crate；工具要实现的 `an_agents::Tool` trait 在 `agents` crate；`runtime` 依赖 `agents`，`agents` **不能**依赖 `runtime`。所以 MOM 工具要写在 `runtime` crate（`crates/runtime/src/mom/` 下新文件，实现 `an_agents::Tool`），不能写在 `agents`。
- `Tool` trait：`name()`、`description()`、`args_hint()`、可选 `capability()`（`CapabilityCard{effect: Effect::ReadOnly …}`，默认只有名为 `write_file` 的工具是写入，其余只读）、`async fn call(&self, args: &Value) -> Result<ToolOutput>`。`ToolRegistry::register(Arc<dyn Tool>)` 注册。
- 运营频道、案例卡、`ChatCard::OpsCase` 已有（D0）。会话里“需要时推卡”还没有。

## 2. L0：关联与日志

**目标**：任何一条与案例有关的日志、事件、卡片都能用 `case_id` 串起来。

- 先查现状：`rg -n "case_id" crates/runtime/src/mom crates/server/src` 看哪些日志已带 `case_id`。新增埋点只能用 `docs/DESIGN.md` 第 10 节的事件名，并同时带 `task_id`（运营频道用 `task_id = "ops-channel"`，已有先例见 `mom_ops.rs`）。
- 改动限定在 `crates/server/src/mom.rs`、`mom_ops.rs` 的日志语句，**不改 `engine.rs`**。
- 用例 `RT-MOMLOOP-01`：命令接口（`/cases/{id}/commands`）执行后，捕获日志（参照 `crates/observe` 的现有测试捕获方式）里能按 `case_id` 过滤出这次命令。用例先写进 CASES.md 表格并见 Red。
- 若发现必须改引擎才能带上 `case_id` → 停下来，写方案给用户。

## 3. L1：会话可读（只读工具）

### L1-0 先出方案（不写代码）

要用户拍板的事（推荐项在前）：
1. **注入方式**（推荐）：在 `main.rs` 把创建 MOM 引擎的代码提到 `Runtime` 之前；给 `Runtime` 增加一个新构造函数 `with_extra_tools(config, store, diagnostics, extra: ToolRegistry)`，内部和 `with_mcp_diagnostics` 共用 `assemble`，把 `extra` 合并进 `mcp_tools`。不改现有构造函数签名，旧调用方不受影响。备选：给工具一个“延迟绑定”的 `Arc<OnceLock<…>>`，先注册后填充（装配顺序不动，但有“工具已注册、引擎还没填”的空窗，需要测试）。
2. 工具只在 `[mom] chat_bridge = true` 时注册（与桌面同一个开关；关闭时能力清单里不出现，避免模型调用不存在的数据）。
3. 工具返回格式：JSON 文本，必含 `provenance:"simulation"`、`observed_at`（仿真分钟）、`limitations`（字符串数组）；这三项缺一不可（决定 56 的要求）。

方案要写清：改哪些文件、对外接口变化、风险和回滚（回滚 = 开关关闭 + 去掉 `with_extra_tools` 的调用）。**用户批准前不要改 `crates/runtime/src/lib.rs`**。

### L1-1 `mom_cases`、`mom_factory_status`

- 新文件 `crates/runtime/src/mom/chat_tools.rs`；`mod.rs` 里加 `pub mod chat_tools;`。
- `mom_cases`：参数 `{"case_id": "可选"}`。无参数 → 返回案例列表摘要（复用 D1-1 的 `ops_view::desk_view`，不要再写一份遍历逻辑）；有参数 → 返回 `engine.human_view(case_id)` 的摘要。
- `mom_factory_status`：先读现有 DES 只读接口（`rg -n "fn " crates/runtime/src/mom/des` 找“设备状态/世界快照”读取函数）；没有现成只读函数就**只返回案例里 `world` 字段的摘要**，并在 `limitations` 写明“来自案例快照，不是实时设备数据”，不要新增 DES 接口。
- 两个工具的 `capability().effect` 必须是 `Effect::ReadOnly`，并在 `/api/capabilities` 中可见（用例断言）。
- 用例：
  - `RT-MOMCHAT-01`：工具输出含三个必填字段；案例集合与引擎一致。
  - `RT-MOMCHAT-02`：效果分级为只读；工具集合里**不存在**任何 approve/execute/close 类名称。
  - `E2E-MOMLOOP-READ`（`web/e2e/` 新文件 `momloop.spec.ts`，写法照 `ops.spec.ts`：自己起实例、端口用 **18792**，避免与 ops 的 18791 冲突）：造案例后在会话问“现在有哪些待处理案例”，回答内容含案例 id 且含“模拟”。模型用受控替身（看 `web/e2e/harness.mjs` 里 mock 模型的做法），**不要依赖真实模型**。
- check-map 命名坑：E2E 用例 id 必须匹配 `E2E-[A-Z]+`（只允许大写字母，不能有数字和连字符），所以写 `E2E-MOMLOOPREAD` 而不是 `E2E-MOMLOOP-READ`；运行时用例按 `RT-XXX-NN` 表格行 + 同名 `rt_xxx_NN_…` 测试函数。草稿用例先写成 bullet，表格行必须有对应测试。

### L1-2 `mom_forecast`、`mom_whatif`

先读 `docs/DESIGN.md` §9 与 `crates/runtime/src/mom/simulation.rs` 找推演接口。`whatif` 只能调用**不改变案例状态**的试算；如果现有接口会写状态，**只做 `mom_forecast`**，`mom_whatif` 记为待决定写进 DECISIONS（助手建议），不要硬做。

## 4. L2 / L3

- L2：批准在卡片上，见 [OPS_DESK_TASKS.md](OPS_DESK_TASKS.md) §7（D2）。会话里“按需发卡”（用户在普通会话里问到某案例时，`mom_cases` 的结果附一张 `OpsCase` 卡）作为 L1 之后的可选卡片，先与用户确认。
- L3：案例关闭后卡片更新并附审核摘要。**开工前先读**：`rg -n "review\|audit" crates/runtime/src/mom` 与 `docs/mom/` 里审核层设计，确认审核结果在引擎里是否可读；不可读则先出方案。用例沿用 E2E-COMPLAINT 的精神：缺核验证据不能关闭且卡片显示原因；审核报告不含标准答案。

## 5. L4：连接器替换

经 `an-des-mcp` 的 `FactoryService::call` 替换进程内 DES 调用（⑤a-2/⑤a-3）。基线是已有的 `RT-DESM-02`（同种子下进程内与 MCP 结果一致）。动手前先读 `crates/server/tests/mom_des_connector.rs`、`mom_des_http_mcp.rs`。写操作要带幂等键与审批引用；工厂离线时卡片显示明确错误。这张卡涉及对外调用，**先出方案**。

## 6. GS1 总装 E2E

`E2E-MOMLOOPALL`：DES 报警 → 运营频道出卡 → 会话里追问（L1 工具）→ 卡片上批准（D2）→ 执行回写 DES → 复检核验 → 案例关闭、卡片更新（L3）。每一步断言都要跨两个模块（例如：卡片批准后，`/mom/` 页面与 DES 世界状态同时变化）。

## 7. 不做

沿用 INTEGRATION_PLAN §3.7：不做真实设备写入、不做 MOM 界面改造、不改引擎状态机。
