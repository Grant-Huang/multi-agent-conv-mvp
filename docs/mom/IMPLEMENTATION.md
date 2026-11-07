# MOM 实施计划与运行说明

用户已授权完整实施 MOM 与 Mock，按以下阶段连续推进。本次新增隔离 `an_runtime::mom`、独立 `an-mom` 服务和独立 SQLite，不修改原 orchestrator/store/registry、AgentAdapter 或现有配置字段。新增 MOM HTTP 接口仅用于本机模拟演示，审批身份为测试身份，不代表企业身份认证。

## 实施顺序

1. 测试先行：S1/S2 端到端、批准阻断、重复事件、过期批准、预算、租约恢复、子任务、SLA 与双视图。
2. 持久公告板：版本化状态、事务事件/outbox、增量游标、恢复与双视图。
3. 领域 MockWorld、类型化工具调用、DAG 依赖与虚拟时间调度。
4. S1 恢复生产、S2 客诉初复/8D及受限调查子任务。
5. 独立服务、人类看板、ScriptedMaster 与可选 LLM 决策接口。
6. 全套验证、运行手册、Mock 台账与实际局限更新。

## 验收边界

目标是设计中的 A 层可运行系统，补上 B 层可配置模型适配入口。真实模型盲测、真实企业接口、工业诊断与现场动作需外部数据/凭证，不能伪称完成。Mock 不因代码已实现就勾选为真实完成。

## 已实现内容与启动

上述 1–6 阶段的 A 层实现已落地。代码入口为 `crates/runtime/src/mom/`、`crates/server/src/mom.rs`、`web/mom/index.html`。没有修改原任务编排器，也没有新增依赖。

在仓库根目录：

```bash
cargo run --bin an-mom -- --db data/mom.db --bind 127.0.0.1:8790
```

通过本机浏览器打开该端口即可使用人类看板。新建 S1/S2 后确定性流程执行只读调查、形成方案，并自动唤醒 ScriptedMaster 启动独立调查。时间只由“推进”按钮/命令推进，后台调查不会消耗实际等待时间。批准和执行分开，创建业务工单与验证完成分开。旧批准失效时可在未下发动作前“更新证据与方案”，最多两次，仍需推进时间和重新审批。

- S1：批准 production → 执行 → 推进 3 分钟 → 核验关闭。可在故障面板注入资源冲突、诊断超时、下发失败和回执丢失。
- S2：批准 initial_response → 执行，模拟消息进入发件箱。8D 默认是进展版。只有显式注入 root_cause_verified 与 effectiveness_verified，再采集模拟现场验证，才生成模拟正式 8D；须重新批准 eight_d、执行并推进时间后才能关闭。真实现场证据未接入。
- 对客发送永远是本地模拟发件箱，真实对客发送不在本次实现内。
- CLI 拒绝非 loopback 监听。本机测试身份不是企业认证，不应通过端口转发对外提供操作权限。
- Ctrl-C 优雅停机；重新启动恢复 SQLite 事件、案例、deadline、动作账本；活动子任务租约失效，旧 token 不能提交。没有真实子进程需要终止。

可选 RelOS 关系上下文（E1-C，拟登记为台账 M24，待确认）：`cargo run --bin an-mom -- --relos-url http://127.0.0.1:8000`（或环境变量 `AN_MOM_RELOS_URL`）。创建 S1 前，服务层在加锁之外异步调用 RelOS `POST /v1/context/compile`（以 CNC-XX 为中心、Line-B 与批次为锚点，预算 1000），5 秒超时、不重试；结果作为观测写入 `world.relos`（WorldChanged 事件留痕），维护/质量诊断只读这条记录，引擎仍可确定性重放。RelOS 不可用、超时或无候选时，诊断为 UNKNOWN 并带 `relos_status`，案例进入人工决策，不会自动下发。没有子进程；HTTP 请求随请求 future 取消。RelOS 中的知识目前为仿真数据，候选根因只是假设。也可通过故障注入 `{"command":"fault","key":"relos","value":{"status":"unavailable"}}` 模拟 RelOS 故障。

可选主模型入口：`cargo run --bin an-mom -- --llm-config config.toml`，读取现有 `[llm]` 配置。最多一个 Master 请求/案例、全局最多四个；模型 60 秒外层超时，45 秒客户端超时，一次格式修正，上限 512 输出 token。模型结果通过版本检查和受限 intent 校验。当前只开放 wait/delegation，不宣传为完整 LLM 自主排产；真实模型未配置、未验证。

## 新增模拟 API

以下属于独立服务，不挂到现有 8787 API，也不更改 `config.toml` 字段：

| 方法与路径 | 用途 |
|---|---|
| GET `/api/mom/cases` | 人类案例列表与逻辑时间 |
| POST `/api/mom/scenarios/s1` 或 `/s2` | 新建有独立事件ID的可重放场景 |
| POST `/api/mom/events` | 输入结构化模拟事件，来源限已知夹具系统 |
| GET `/api/mom/cases/{id}` | 人类运营投影 |
| GET `/api/mom/cases/{id}/agent?after=0&limit=20` | 带硬约束、证据索引和增量事件的机器投影 |
| GET `/api/mom/cases/{id}/events?after=0&limit=20` | 持久事件分页 |
| POST `/api/mom/cases/{id}/master` | 有预算与并发上限的主 Agent 唤醒 |
| POST `/api/mom/cases/{id}/commands` | 类型化模拟操作 |

命令示例：

```json
{"command":"approve","scope":"production","plan_revision":1,"actor":"supervisor"}
{"command":"execute","scope":"production"}
{"command":"advance","minutes":3}
{"command":"delegate","kind":"equipment_investigation"}
{"command":"fault","key":"maintenance_timeout","value":true}
{"command":"refresh_plan"}
{"command":"cancel"}
{"command":"close"}
```

`advance` 会推进所有案例的共享时钟。两类视图共享版本和序号；页面证据以 textContent 渲染，外部资料不会成为 HTML 或工具命令。权限隔离当前仅为模拟角色与工具白名单，不是多租户授权系统。

## 验证与报告

```bash
cargo fmt --all -- --check
CARGO_TARGET_DIR=target cargo test
CARGO_TARGET_DIR=target cargo clippy --all-targets -- -D warnings
(cd web && npm run build && npm test)
python3 tools/mom/smoke.py --report .tmp/mom/smoke-report.json
```

最后一条需先启动服务。报告包含运行 ID、HEAD、模板、seed、时区、检查结果和 mock_ids。测试中的临时数据库位于系统临时目录；手动演示数据库保留于指定路径，不自动删除用户数据。

SQLite 初始化通过事务创建独立 v1 表并记录 user_version；未来结构升级必须增加迁移，不支持高版本时拒绝启动。事件和状态事务提交，outbox 可补拉/标记已发布，订阅游标可持久确认；当前网页使用显式刷新和命令回执，不提供后台 SSE。未实现内容与生产替换仅以 MOCK_REGISTER 为准。

最终运行记录与已知差距见 [RESULTS.md](RESULTS.md)。已将 MOM 登记到 `docs/testing/features.json`，真实 Chromium 用例见 `web/e2e/mom.spec.ts`，通过 `cd web && npm run verify` 一并运行。MOM harness 自行构建二进制、使用专用 loopback 18790 与临时数据库，结束后停止自己的服务并保存日志。

## 动态仿真增量计划

先补主界面同源 `/mom/` 入口，再支持已校验的场景配置、seed、订单/产能/质量参数、持续事件队列、受限播放与暂停。仿真仍使用统一共享时钟；暂停只停止自动推进，手动推进仍作用于所有案例。场景配置和待投递事件持久化，重启保留队列但暂停自动播放，防止重启后静默追赶并越过人工审批。

新增模拟 API 的理由：原硬编码 `/scenarios/s1|s2` 无法表达配置和持续扰动，新增 `/simulations`、`/playback`、`/gaps`；旧接口保持兼容。主程序仅挂载隔离 MOM 路由和独立数据库，不改原业务状态机或 config.toml。gap 继续以统一 Markdown 台账为准，API 从该台账生成，不允许界面自行把 Mock 勾为真实完成。

动态仿真现已实现：

- 启动主程序 `agentnexus` 后，左侧点击 **MOM 运营沙盘**，进入同源 `/mom/`，无需额外启动 8790 服务。主服务仅在 loopback 监听时挂载模拟 API；公开监听时入口返回明确的不可用说明，防止测试审批身份暴露。
- 独立 `an-mom` 仍可使用；两种入口界面相同，各自数据库独立。主服务的 MOM 数据库为原业务数据库路径的 `.mom.db` 同级文件。
- 展开 **配置持续仿真**，设置名称、业务场景、订单/产能、seed、事件数和间隔，可增加白名单定时扰动。新建后只有初始事件，后续事件随共享仿真时钟逐个到达，不再一次性塞入全部事件。
- 自动播放为 **1 秒 = 1 个虚拟分钟**，页面默认一次播放30分钟，API最大360分钟；可暂停，且重启默认暂停。手动推进作用于全部案例，不能撤销已经受理的业务动作。
- `/api/mom/simulations` GET 列出配置与队列，POST 新建；`/api/mom/playback` POST 控制播放；`/api/mom/gaps` GET 从统一 Mock 台账生成完整状态。主服务下 API 同样带 `/mom` 前缀。
- 场景与队列使用现有 JSON snapshot 的兼容默认字段保存，无新增 SQL 表；旧数据库保持可读。固定 seed 与相同输入可复现同一组扰动；重放与取消有独立测试。

## 当前实现子集与审查说明

- `patch` 命令体为 `{"command":"patch","node":"quality","dependency":"maintenance"}`，仅支持受限依赖调整，不允许移除安全关卡或修改已下发计划。联锁触发后拒绝该命令。
- `request_tool` 是拟议 Master 协议，当前 Intent 未实现；节点只使用 READY/RUNNING/SUCCEEDED/UNKNOWN/CANCELLED 子集，未实现完整 CLAIMED/WAITING_TOOL/VERIFYING 生命周期。
- 当前 fixture 的 `production` 是合并模拟授权，覆盖 `mro.transfer`、`eam.maintenance`、`mes.reschedule`，不满足真实系统逐动作分别授权要求。该简化登记为 M20，不能作为真实审批链验收证据。
