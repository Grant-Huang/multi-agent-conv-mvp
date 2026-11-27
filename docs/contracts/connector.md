# 连接器契约 K（草案）

| 项目 | 内容 |
|---|---|
| 层级 | D1（见 [DEV_STANDARD.md](../../DEV_STANDARD.md) 第 2、3 节） |
| 状态 | **草案，待用户批准**。批准前不约束任何改动；本文件即 [2026-11-26-workstream-reallocation](../decisions/2026-11-26-workstream-realignment.md) 决定第 4 条所要求的首个交付物（连接器契约草案，#398） |
| 服务宪章 | CH-G4「外部系统只经标准连接器接入」（用户确认 2026-11-25） |
| 负责线 | 第 5 线定标准与接入；业务系统连接器的实现由第 3 线按 World Adapter 完成；调度使用归第 4 线（[分工决定](../decisions/2026-11-26-workstream-realignment.md) 第 4、7 条） |

## 1. 范围

适用于一切**外部系统接入**：业务系统（RelOS、MOM、PLC 网关等）、模型服务（云端 / 本机 LLM 后端）、其他 agent（ACP / A2A）、外部工具（websearch、DLP）。**不适用于**进程内模块之间的调用（那不是「外部系统」）。

「连接器」= 符合本契约的接入层。业务代码（特性 crate）不得绕过连接器直接调用外部系统（CH-G4）。划分口径：第 5 线管**接进来、可用、可诊断**（发现、配置、准入、握手、连接器标准）；第 4 线管**怎么用**（按任务选用、调度与回退）。

## 2. 现状

| 接入面 | 位置 | 与本契约的差距 |
|---|---|---|
| Agent 目录与准入 | `crates/agents/src/catalog.rs`（CatalogMeta：id / purpose / install / requirements / docs_url）、`admission_api.rs` | 字段子集已符合 §4；缺凭据引用与 health 探测的统一字段 |
| ACP / A2A agent | `crates/agents/src/acp.rs`（resolve_command 实时探测、is_installed）、a2a.rs（出站经 DLP，#360） | 出站守卫已是范本；命令解析依赖宿主 PATH（无注册表统一定位） |
| 模型服务 | `crates/llm/src/backend.rs`（BackendConfig：kind / base_url / priority / 并发） | 配置面符合；缺凭据引用（当前明文 key 进配置文件）与健康字段 |
| 业务系统 | `crates/server/src/mom_relos.rs`（RelosClient，超时 5s、token 预算）、World Adapter（第 3 线） | RelosClient 已具超时 / 降级范式；World Adapter 迁移清单见第 6 节 |
| 外发脱敏 | [DLP D1 定稿](../../tools/dlp-egress/docs/INTERFACE.md) | 已是独立契约，本契约引用不重定义 |

## 3. 已确认的不变量

来自用户已确认的宪章与决定，编号 K 系列（批准后生效）：

| 编号 | 不变量 | 来源 |
|---|---|---|
| K1 | 外部系统只经连接器接入；业务代码不直接持有外部系统的地址、凭据或客户端 | CH-G4（宪章 §5，用户确认 2026-11-25） |
| K2 | 凭据由宿主持有并以**引用**（环境变量名 / 凭据文件路径 / 密钥管理器键名）交给连接器；**不得**以明文写进配置文件、提示词或任务上下文 | CH-P5 与 D-016 的既有口径（助手建议成文，批准时确认） |
| K3 | 每次外发调用经 DLP 外发检查（enforce 下拦截即不发），并携带 `task_id` 关联（D1 定稿；#360 出站守卫为范本） | DLP D1 定稿；用户确认 2026-11-18 |
| K4 | 连接器必须实现健康探测（probe）与诊断输出；不可用 / 超时 / 格式错误时按连接器声明降级，不得无声失败 | 决定 2026-11-25-agent-integration-to-line5「接入侧可诊断」 |
| K5 | 连接器注册进统一目录（id、能力、状态），供发现与准入；未注册的外部系统不得被业务代码调用 | 目录现状（CatalogMeta）+ 准入闸门（RT-AGCAT / admission）口径 |

## 4. 建议补充的不变量（助手建议，K6–K9；用户批准后移入第 3 节）

| 编号 | 建议 | 理由 |
|---|---|---|
| K6 | 连接器描述符最小字段：`id`、`kind`（business / llm / agent / tool）、`transport`（http / stdio / acp / a2a）、`endpoint`、`credentials`（引用）、`capabilities`、`health`（探测方式与周期）、`docs_url`、`owner`（负责线） | 与 CatalogMeta 对齐；一次定义，目录 / 准入 / 诊断三处共用 |
| K7 | 超时与重试必须显式声明（默认：单次 5 秒、不重试），降级行为写进连接器描述（RelosClient 与 DLP 为范本） | 现有两处范本一致；避免各连接器自行发挥 |
| K8 | Token / 预算类参数（如 RelOS 的 `token_budget`）由调用方声明，连接器不得自行放大 | 防止预算失守（与模型上下文预算同口径） |
| K9 | 出站内容的脱敏与拦截不重定义，一律引用 DLP D1 | 单一权威，避免两套口径 |

## 5. 最小连接器描述符（示例，非实现）

```toml
[id]
kind = "business"            # business | llm | agent | tool
transport = "http"           # http | stdio | acp | a2a
endpoint = "https://relos.example.com"
credentials = { env = "RELOS_TOKEN" }   # 引用，不存明文（K2）
capabilities = ["context.compile"]
health = { probe = "POST /v1/health", interval_secs = 300 }
timeout_secs = 5             # K7：显式声明
retry = "none"
owner = "line5"
docs_url = "docs/contracts/connector.md"
```

## 6. 存量迁移清单（CH-G4「存量按清单迁移」）

| 存量 | 差距 | 迁移动作（各开任务卡） |
|---|---|---|
| `crates/llm` 后端配置的明文 key | 违 K2 | 改凭据引用（env / 文件路径），配置文件只存引用 |
| Agent 命令解析依赖宿主 PATH | 违 K5（统一定位） | 目录注册带显式命令路径 / 安装探测（与 CatalogMeta 合并） |
| `mom_relos.rs` 直连 URL 进配置 | 违 K1（业务代码持有地址） | 地址与凭据迁入连接器注册，MOM 经连接器调用 |
| World Adapter | 待对照 | 第 3 线按 §4 字段补描述符与健康探测 |

## 7. 待决定 → 已有推荐口径（2026-11-27 委托按推荐执行，见[决策](../decisions/2026-11-27-connector-k7-decisions.md)；正式批准后关闭本节）

| 待决定项 | 拍板 |
|---|---|
| ① 凭据引用形态 | **env 优先**（keychain 作为后续增强另卡；[llm] `api_key_env` 已随 PR #493 落地） |
| ② 未注册外部系统阻断时机 | **先告警后拒绝**：迁移期 WARN（`connector.unregistered`），期满翻转拒绝（时点随里程碑由用户拍板） |
| ③ 描述符存放 | **统一 `connectors.toml`**（各面现有配置保持不动但引用描述符 id） |

## 8. 出问题先看

- 业务代码直连外部系统：搜 `reqwest::` / 直连 URL 的出现位置（连接器目录外应无新增长）。
- 凭据泄漏：配置文件与日志中搜明文 key（K2 违规）。
- 连接器不可用无声失败：查 K4 的健康探测与降级声明。

## 9. 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-26 | 新建草案（范围 / 现状 / K1–K5 已确认口径成文 / K6–K9 建议 / 描述符示例 / 迁移清单 / 待决定），待用户批准 | #398；分工决定 2026-11-26 第 4、7 条 |
| 2026-11-27 | §7 三个待决定项按推荐口径成文（env 优先 / 先告警后拒绝 / 统一 connectors.toml）；K6–K9 暂列建议 | 用户委托按推荐执行；[决策](../decisions/2026-11-27-connector-k7-decisions.md)；#485/#487 |
| 2026-11-27 | §4 K6–K9 措辞改回「建议」、头部维持草案状态（#518 / R-20261127-M13：无正式批准记录，用户委托按推荐执行 ≠ 契约批准） | R-20261127-M13 / #518 |
