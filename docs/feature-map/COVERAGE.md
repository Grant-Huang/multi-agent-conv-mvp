# 功能覆盖与证据状态

本文件是各功能的**覆盖范围与证据状态权威表**（原 `docs/testing/FEATURE_MAP.md`，已并入功能地图目录）。每个功能的入口、驱动方法、判据和坑见 [README](README.md) 索引中对应的功能文件；用户场景说明见 [docs/manual](../manual/README.md)。

基于远端 929301a；此图同时描述用户路径和可观测判据，机器索引见 features.json。浏览器覆盖由实际测试结果证明，不能只凭 manifest 的 coverage 字段宣称通过。

| Feature | 用户目标与真实入口 | 驱动方法/判据 | 测试与边界 |
|---|---|---|---|
| conversation | 左侧“＋ 新会话”；中间输入框“输入消息” | 发问候、刷新重读、切换会话；最终消息 ID/内容一致，无串入和 think 泄漏 | E2E-CONV、E2E-ACK；多问题质量仍需实际模型验收 |
| navigation | 窄屏顶部“☰ 会话”打开左栏抽屉 | 创建/选择会话后抽屉关闭，发送框可用 | E2E-MOBILE；非固定坐标定位 |
| tasks | 对话中的任务状态/产物，右侧“任务”卡片；简短回复下的走马灯（收起显示最新一行，展开从旧到新，运行中行尾动画） | 真实文件工具写入、质检 pass、COMPLETED；下载字节与 artifact、磁盘一致；走马灯位置、顺序、背景与运行指示 | E2E-FILE、E2E-TICKER、E2E-FAILCARD；进程成功不等于验收通过 |
| approval | 对话中“批准执行”，或展开任务卡；批准按钮上方的审批说明 | 高风险先 NEEDS_HUMAN、没有 result；点击后才执行；批准前写明将交给谁（执行组顺序）、预计工具、触发规则 | E2E-APPROVE、E2E-APPROVEINFO、RT-ASSIGN-05；预计工具是 Router 预估，不是硬限制；不批准真实危险操作 |
| assignment | 右侧“任务”卡片标题行的执行者；展开后的“执行者”节 | 任务记录执行的 agent、主力 / 备份（执行组位置）、排队时长、返工轮次；进入 RUNNING 时已写入；旧库自动补列 | E2E-ASSIGN、RT-ASSIGN-01 至 RT-ASSIGN-04；多节点真实执行组未在浏览器覆盖 |
| cancellation | 执行中“取消任务”，或卡片“取消” | 先点变“确认取消”，第二次才 CANCELLED，API 状态一致 | E2E-CANCEL；三秒超时恢复由已有组件测试保护 |
| observability | 右侧底部“开发者模式：关”切开，再点击“诊断”“路由日志” | 诊断显示真实后端；路由日志匹配本次消息，可展开模型原文 | E2E-DIAG；复制剪贴板尚待完整浏览器验证 |
| agent-links | 左下角连接状态：绿=已连通可调用（握手成功，或 10 分钟内有成功调用）；灰=已配置未连通；红=不可用；灰红的原因在悬停提示里 | 内置 agent 始终绿；命令不存在为红；能启动但握手失败为灰；健康接口每个 agent 带 link.status | 单元：runtime link.rs 状态规则；HTTP：server/tests/e2e.rs agent_links_are_red_when_missing_and_grey_when_not_connected；组件：web/src/health.test.tsx；真实 ACP 握手与模型调用未在沙箱验证 |
| acp-discovery | 无单独入口：启动时自动发现，左下角连接状态里出现对应 agent | 预设命令已安装 → 注册 ACP 卡片（coding、类型 = id）；通用名 `agent` 不认作 Cursor；用户同 id 配置优先；未安装不注册、不报错；原有 3 个预设不变 | RT-ACPD-01 至 RT-ACPD-05（`crates/runtime/tests/acp_discovery.rs`，假可执行文件，只检查文件不运行）；真实 claude-agent-acp / codex-acp / cursor-agent / zcode-acp-server 握手未在沙箱验证 |
| agent-catalog | 暂无界面入口（AG.3 做卡片）；HTTP `GET /api/agents/catalog`、`POST /api/agents/probe` | 目录列出全部预设、用途与安装步骤、本机状态和连接状态；握手检测分类 ok / not_installed / needs_login / missing_dependency / handshake_failed / timeout；超时后进程被结束；只接受 id，带 command 的请求被拒绝且不执行；检测成功后连接状态变绿 | RT-AGCAT-01 至 RT-AGCAT-09（`crates/server/tests/agent_catalog.rs`，模拟 ACP agent 与 sh 脚本）；真实 claude / codex / cursor / zcode 的登录与依赖错误文字未在沙箱验证，分类特征可能需要按实测补充 |
| front-thinking | 无界面入口；配置 `[backends.<名>] front_structured_thinking / front_reason_first / front_thinking_escalation` 与 `[conversation] risk_rules / route_class_questions / route_class_from_tools` | 不写新配置时请求不变；不思考时 reason 字段在最前；规则判风险只升不降；格式不合格、风险 high、角色无可用 agent、任务目标为空时开思考重判一次，不退回不思考；打开 route_class_questions 时路由回答两个是/否问题，类别由代码算出；打开 route_class_from_tools 时 general 任务的类别由工具清单算出 | RT-FT-01 至 RT-FT-20（`crates/server/tests/front_thinking.rs`，假模型服务记录请求体）；FT.1 阶段 01/03/05/11 为基线守护，其余为红灯；FT.5 阶段 12 为基线守护，13～16 为红灯；FT.5b 阶段 17、19、20 为基线守护（19 检查非 general 不受影响），18 为红灯；真实模型的准确率与延迟要在用户本机评测（FT.3、FT.5） |
| agent-config | 暂无界面入口（AG.3 做卡片）；HTTP `POST /api/agents/proposals`、`POST /api/agents/proposals/{id}/apply` | 提议只校验不写入；凭证确认后写托管文件并热更新注册表；可连续撤销；config.toml 优先；没有可用 coding agent 时由前台模型用大白话回复 | RT-AGCFG-01 至 RT-AGCFG-14（`crates/server/tests/agent_config.rs`）；真实 agent 的热接入未在沙箱验证 |
| agent-ui | 右侧「助手」页签；首次打开时对话区顶部的发现卡 | 列出状态、来源、用途；安装引导与重新检测；确认卡 → 结果卡；撤销；停用最后一个写代码的助手后对话用大白话回复 | E2E-AGFIRST、E2E-AGLIST、E2E-AGTOML、E2E-AGGUIDE、E2E-AGCONNECT、E2E-AGUNDO、E2E-AGDISABLE（`web/e2e/agents.spec.ts`，模拟 ACP agent 冒充已安装的 claude-agent-acp）；"普通"使用水平未实现 |
| agent-chat | 对话里说"把 Cursor 接进来"等（`[conversation] agent_management` 打开时） | 前台判断 manage_agents（操作 / 助手 / 角色都是候选枚举）；代码生成确认卡、选择卡、引导卡、发现卡；卡片存进消息；送给模型的历史只用正文；确认后追加结果卡；管理助手 / 改配置类意图确定性拦截，避免误派 General 读配置 | RT-AGCHAT-01 至 RT-AGCHAT-14、RT-AGMGMT-01（`crates/server/tests/agent_chat.rs`、`crates/runtime/src/agent_chat.rs`、`crates/runtime/src/router.rs`、`crates/runtime/tests/agent_chat_migration.rs`）；E2E-AGCHAT、E2E-AGCHOICE；真实模型的识别准确率由用户本机评测 |
| agent-diagnose | 「助手」页签每行的"排查"；`POST /api/agents/diagnose` | 握手检测 + 只读本机体检（服务 / 登录 shell 的 PATH、node 版本、凭证文件是否存在）+ 规则给出大白话结论和修复步骤；不外发 | RT-AGDIAG-01～04、06（`crates/server/tests/agent_diagnose.rs`）、RT-AGDIAG-05、07、08、10～12（`crates/server/tests/agent_diagnose_send.rs`）、E2E-AGDIAG、E2E-AGHELP；诊断包（第 3 层）和请其他助手帮忙（第 4 层，每次预览、逐次同意） |
| dlp-egress | `config.toml` 的 `[egress]`（mode 为 audit 或 enforce）；排查第 3、4 层自动调用，复用外发检查守卫 | 外发前经 DLP 评估：只有 `ready_to_send` 才发，且只发脱敏文本；帮手回复还原后只用于显示；结束后撤销映射；任何失败都不发；DLP 工具不进模型可见的清单 | RT-DLP-01～06（`crates/server/tests/agent_diagnose_send.rs`，模拟 DLP 服务）；真实 dlp-egress 待工具一侧按 CONTRACT 第 8 节改完后接入验收 |
| nodes | “节点”→按地址添加/发现节点→配对；解除也两步确认 | 以测试节点核对授权、执行、解除后的不可用状态 | 现有 server/tests/e2e.rs node_flow/ACP；真实 LAN 未覆盖 |
| feedback | 展开可匹配用户消息的任务→“派错了”→动作/角色/说明→保存 | POST /api/feedback 信封、持久化记录；非法动作/角色被拒；导出仅为草稿 | 现有 misroute 组件及 Rust HTTP 测试；本次无模板纠正字段 |
| reports | 对话中要求“写报告并引用公开资料/最新信息”，General 后台执行 | 同时要求 web_search+write_file 时走代码控制流程：规划→检索（≤6、去重）→章节并行写作→汇合；过半章成功才完成；无效引用剔除；取消后不再请求。仅要求 web_search 时仍走逐步循环（先搜后写） | RT-REP-01 至 RT-REP-09（Rust 集成测试，受控模型）；Router 提示词与重复追加为单元测试；真实模型质量与搜索时效需实际 E2E 验收 |
| templates | 当前无界面入口（M4.3 尚未接入编排器） | 模板校验、Router 拆分决定、分支信封与汇合输入在 Runtime 集成层验证 | RT-TPL-01 至 RT-TPL-10；Rust 集成测试与单元测试；并行执行与 Composer 汇合回复未实现 |
| admission | 无界面入口；HTTP `/api/admission/*`（开启 `[admission]` 时才有） | S1→S7 顺序、澄清问题、加权打分与等级、人工审批闸门、确定性抽检、警告与降级、撤权与凭证代际、重启恢复、旧库自动建表；按等级分配、凭证过期结果丢弃、撤权中止进行中任务并由备份接手、外部 agent 试用期记录；HTTP 接口走完准入流程、激活后注册外部 agent（kind=a2a）并完成任务、重启后自动重新注册、未开启时 404；外部 agent HTTP 适配器 | RT-ADM-01 至 RT-ADM-17；实验工具：`cargo run -p an-runtime --example admission_replay`（用正式账本回放 A1/A2/A3）、`cargo run -p an-server --example admission_revoke_bench`（撤权实测）；Rust 集成测试；未接入网页界面，S2/S3 的大模型澄清与评分由实验脚本完成 |
| tools-evals | 聊天委派工具；终端 websearch、an-eval、dream；外发脱敏 MCP（tools/dlp-egress） | 计算/文件/CLI证据、JSON错误、逐条评分和失败草稿；不自动改主评测集 | 现有 Rust/Python 测试；dlp-egress 有 MCP 协议测试与 700 条+留出集评测（GLiNER 未评测）；真实搜索时效与模型质量另验 |
| egress | `[egress]` 外发检查（off / audit / enforce）：工具调用守卫与云端 LLM 检查，调用 dlp-egress | enforce 下只发送脱敏内容；REVIEW/BLOCK/DLP 异常不发送；DLP 工具不暴露给模型；启动校验外发路径登记；LLM 输出 Token 本机恢复 | RT-EGR-01 至 RT-EGR-16（Rust 集成测试，DLP 与模型服务均为替身）；另有与真实 dlp-egress 的联调测试（设置 AN_TEST_DLP_COMMAND 时运行）；真实模型与真实部署需按部署说明验收 |
| ai-desktop | 默认「我的 AI 桌面」；SMT-03 工作区；S1/审批/Run/语音/跨端/受理等面板 | Feature Map AD-*；Mock 与真实入口共用壳 | E2E-ADNAV…ADAPPR/ADATTN…ADACCEPT（`web/e2e/desktop.spec.ts`）；B1–C1 证据 b1–c1；**C2** RT-ADAPPR/ADATTN，证据 [ai-desktop-c2.md](../testing/evidence/ai-desktop-c2.md)；D 仍待 |

应用为单页布局，网页没有 /tasks、/settings 或 /templates/run 路径。API 以 crates/server/src/lib.rs 为准；状态和中文文案见 web/src/labels.ts。

当前不是已实现功能：M4.3 的模板并行执行与 Composer 汇合回复（校验部分见 templates 行）、生产级 MOM 连接器、知识库、Benny 自动修复、多用户登录、通用动态 DAG、执行中断续跑。它们在其他设计中出现不代表当前代码存在。

## 操作定位与维护

优先使用 role/name、aria-label、placeholder，并在 main、会话列表、任务与状态区域限定作用域；重复的下载/取消按钮不要全局盲点。开发者模式默认为关，切换状态保存在浏览器 localStorage，测试使用全新 context。

每次变更功能，先更新 CASES.md 与可执行 E2E，再实现；同步 features.json 的源文件和测试引用。浏览器测试不 mock 应用 API；新增功能应先见到业务断言 Red，既有功能基线不可追称测试先行。节点、搜索、模型真实质量不能由本次六条用例推定通过。

## MOM 模拟功能

| Feature | 用户目标与真实入口 | 驱动方法/判据 | 测试与边界 |
|---|---|---|---|
| world-adapter | Desktop「DES 仿真实验」「运营频道」「MOM 工作台」 | Mock 全流程 + M1 DES 读 + M2 DES 写（准出 `wa-m2-6.md`）；**M3-3 生产读合同 RT-01～06 Green**（`/api/world/prod`；`wa-m3-2-green.md` / `wa-m3-3-green.md`；浏览器待 M3-4） | `E2E-DES*`、`E2E-WABIND`、`E2E-WACHANNEL`、`E2E-WADES`/`WAREAD`/`WAOBS`、`E2E-WAEXEC`/`WARECV`/`WADYN`/`WALOOP`、`E2E-WAPROD`/`WAPISO`/`WAPREAD`/`WAPDELAY`、`RT-WA-01～09`、`RT-WADES-01～19`、`RT-WAPROD-01`～`06`、`RT-DES-08～11`、`RT-DESC-*`；套件 `wa-m0/m1/m2-suite`；台账 [WA_MOCK_REGISTRY.md](../mom/WA_MOCK_REGISTRY.md)；任务卡 [WA_M2_TASKS.md](../mom/WA_M2_TASKS.md)、[WA_M3_TASKS.md](../mom/WA_M3_TASKS.md)；准出 [wa-m0-6.md](../testing/evidence/wa-m0-6.md)、[wa-m1-5.md](../testing/evidence/wa-m1-5.md)、[wa-m2-6.md](../testing/evidence/wa-m2-6.md)、[wa-des-connector.md](../testing/evidence/wa-des-connector.md) |
| mom | 独立 an-mom 人类看板与 Agent 机器视图 | S1 审批后下发、核验再关闭；S2 初复独立，缺证据不能关闭8D | E2E-MOM/E2E-COMPLAINT；业务系统、主模型及审批身份仍为替代件，见 MOM Mock 清单 |

MOM 增量：主界面左侧 **MOM 运营沙盘** → 同源 `/mom/`；配置持续仿真与播放暂停、事件队列和完整 gap 详情由 E2E-MOMDYNAMIC 验证。独立8790服务仍保留。两者均为明确标识的模拟模式。

RelOS 接入（E1-C）：RT-MOM-06～09 覆盖上下文驱动的诊断、RelOS 不可用时的安全降级、上下文写入校验与未启用时的兼容；HTTP-MOM-RELOS-01/02（`crates/server/tests/mom.rs`）用桩 RelOS 与无人监听地址验证 an-mom 的端到端调用。RelOS 知识为仿真数据（台账 M24）。

车间 DES 阶段①（`mom-des`，开环）：RT-DES-01～05、16、17（`crates/runtime/tests/mom_des.rs`）定义可复现、主轴磨损因果、报警锁存、标准答案隔离、脚本模式不变、场景模式约束与随机流独立；实现位于 `crates/runtime/src/mom/des/`，阶段①为开环（事件一次生成后按虚拟时钟投递，处置不影响后续事件），Red→Green 证据见 [mom-des-phase1.md](../testing/evidence/mom-des-phase1.md)。设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md)；DES 为仿真，参数为建模假设。

车间 DES 阶段②（`mom-des`）：RT-DES-06、07、20、21（runtime）与 HTTP-DES-01/02（`crates/server/tests/mom_des.rs`）覆盖 DES 案例的工具读取、基于可观测状态的 what-if 多方案、只读工厂状态与趋势预测接口、设备数上限；实现位于 `crates/runtime/src/mom/des/observe.rs` 与 `crates/server/src/mom_sim.rs`，Red→Green 证据见 [mom-des-phase2.md](../testing/evidence/mom-des-phase2.md)。

车间 DES 阶段③（`mom-des`，闭环）：RT-DES-08～11、18 与 HTTP-DES-03 定义方案选定、执行回写仿真世界（维修停机与复位、刀具调拨到达）、执行后同源观测作为核验证据、仿真故障导致补偿、共用资源的案例关联；实现位于 `des/engine_link.rs`、`des/scenario.rs` 与 `engine.rs` 两处 DES 钩子，Red→Green 证据见 [mom-des-phase3.md](../testing/evidence/mom-des-phase3.md)。

R-20261118-H2：RT-DES-31以真实Runtime验证关闭协同事项时跳过安全联锁成员并保留预留，其他成员照常关闭。CaseCloseSkipped为案例事件，不是新增运行日志。 证据见 [r-20261118-h2.md](../testing/evidence/r-20261118-h2.md)。

车间 DES 阶段④（`mom-des`）：RT-DES-12～15、19 与 HTTP-DES-04 定义审核层（规则检查、AI 审核与降级、人工确认与分歧记录、审核模型选择与独立性标注）和批量实验（两种策略、同一组 seed、CSV）；已实现并转绿（真实审核模型未接入，测试用脚本替身）。

DES 审查修复（R-20261117-M1/L1，移植自 PR #84/#85 并适配阶段③）：RT-DES-22/23（`engine_link.rs` 单元测试）验证遥测缓存——推进 2880 分钟至多重新推演一次、恢复快照后各读取入口共用一次、缓存不持久化，干预后与场景结束后的推进也不逐分钟重放；RT-DES-24（`server/tests/mom_des_budget.rs`，真实 HTTP）验证无法满足的大场景在 2 秒内返回 400 constraint，并发读取在 2 秒内返回。证据见 [r-20261117-m1-l1-des.md](../testing/evidence/r-20261117-m1-l1-des.md)。

车间 DES 阶段③b（`mom-des`）：RT-DES-25～29 定义按报警触发的模板（单设备层）与产线协同事项（协调层：同线 60 分钟内多个案例时统一决策、成员只做诊断、执行覆盖报警设备集合、关闭时级联），以及 P0 不变与关闭后事件的两条基线；已实现并转绿（已请人决策的案例撤回请求后加入事项，见设计文档 §6.8 实现修订）。

车间 DES 审查修复 2026-11-18（`mom-des`）：RT-DES-33（关闭协同事项时在事项上逐个汇总被跳过的成员；H2 主修复见上方 #96 的 RT-DES-31）、RT-DES-32 与 HTTP-DES-05（人工审核结论绑定 AI 审核版本；AI 审核接口不阻塞异步工作线程）；已实现并转绿。

模拟工厂与 MCP 挂载阶段⑤a-1（`des-factory`）：RT-DESF-01～08、RT-DESM-01～03 与 RT-DES-30 定义独立的模拟工厂服务（13 个通用风格工具，分观测、执行、控制、管理四类；写操作需幂等键与审批引用；标准答案需管理口令）、经 stdio 挂载的 MCP 服务与进程内结果逐字节一致、以及与 MOM 进程内仿真一致；已实现并转绿（仿真内核迁入 `crates/des`）。⑤a-2 连接器用例为草案。

模拟工厂连接器阶段⑤a-2（`des-factory`）：RT-DESC-01～05 定义 AgentNexus 侧连接器与引擎远程仿真模式：MCP 模式与进程内一致、写操作只在审批执行后发出且幂等、agent 只开放观测工具、模拟工厂离线时明确报错且不回退、审核标准答案来自管理入口；已实现并转绿（an-mom 接入见下条⑤a-3）。

模拟工厂接入 an-mom 阶段⑤a-3（`des-factory`）：RT-DESC-06～12 定义 `an-mom --des-mcp` 启动、HTTP 创建与推进走连接器且与进程内一致、审批执行返回时写操作已发出、后台时钟经连接器推进、模拟工厂离线时返回 503 不回退、桌面只开放观测工具、重启后旧仿真不卡住新仿真；已实现并转绿。

车间 DES 审查修复 2026-11-19（`des-factory`、`mom-des`）：RT-DESF-09 与 RT-DESM-04（溢出参数作为业务错误返回，服务不因 panic 失效）、RT-DESF-10（维修单按班组排队，同设备重叠拒绝）、RT-DES-34 与 RT-DES-35（人工结论带人看到的 AI 审核版本，过期返回 409）；已实现并转绿，证据见 [review-2026-11-19-des.md](../testing/evidence/review-2026-11-19-des.md)。

专家工作流节点绑定（E1-C 方案 B）：RT-BIND-01～10（`crates/runtime/tests/promoted_templates.rs`）覆盖 R1–R8 绑定规则、分段、模板校验对错误模板的拒绝，以及 9 条真实专家工作流的编译与统计；未经专家确认的模板不晋升。

每日审查回归：RT-MOM-01～05（`crates/runtime/tests/mom.rs`）覆盖联锁、终态、事件时间、失败重放和阻断案例租约。HTTP 联锁入口由 `crates/server/tests/mom.rs::safety_trip_rejects_http_patch_and_dispatch_without_side_effects` 验证；它不代表浏览器安全路径已验证。执行证据见 `docs/review/2026-11-06-fixes.md`。

R-20261107-M1：终态案例拒绝手动故障注入；RT-MOM-10（runtime 集成）与 `server/tests/mom.rs::terminal_cases_reject_http_fault_without_mutating_state`（HTTP）覆盖 CANCELLED/CLOSED，拒绝后世界、资源版本、事件序号和事件列表不变。非终态故障注入保持可用。运行证据见 [修复证据](../testing/evidence/r-20261107-m1.md)；本条不代表浏览器故障路径已验证。

R-FIRST-05：补充要求先完成 Router/Policy 校验，再取消旧任务并创建替代任务。RT-CONV-01/02 位于 `crates/server/tests/supplement_routing.rs`，使用受控模型与真实 Runtime/SQLite；覆盖传输失败、格式失败、Policy 拒绝时旧任务状态/事件不变，以及成功时合并目标后替换。证据见 [R-FIRST-05](../testing/evidence/r-first-05.md)。本组不是浏览器验证；取消与新任务持久化之间尚非原子事务。

R-20261110-H1：binding 仅在 Contracted 阶段可登记/重登记，Active 不允许就地改写接入地址、角色、类型或并发。RT-ADM-18 为 Runtime 集成；`server/tests/admission.rs::active_agent_reactivation_returns_conflict_without_binding_or_event_changes` 为 HTTP 回归，验证重复 activate 返回 409 且记录、审计事件、注册卡片不变。证据见 [R-20261110-H1](../testing/evidence/r-20261110-h1.md)；不代表准入接口已经有身份认证或 endpoint 网络白名单。
R-20261118-H1：PY-DLP-H1以真实evaluate管线验证路由字段下嵌套dict/list凭证BLOCK、客户Token化，以及漏分析字符串的fail closed。 证据见 [r-20261118-h1.md](../testing/evidence/r-20261118-h1.md)。

R-20261118-M2：预检所有层级JSON键名（含路由对象），凭证或租户主数据命中时直接BLOCK，不回显原键、不写Vault。普通键与值处理保持。 PY-DLP-M2与原有Python全量/评测门槛验证；[证据](../testing/evidence/r-20261118-m2.md)。

## MOM 独立设计原型

| Feature | 用户路径 | 验证及边界 |
|---|---|---|
| mom-workbench-prototype | 看板／任务／资源＋范围切换 → 统一事项 → 处理进展、条件、决定、交接 | P-MOM-*；独立浏览器检查check-entries/check/check-nodes，证据见docs/mom/prototype/README.md；模拟状态，不计入生产接口与权限集成覆盖 |

## Git 改动证据

`git-evidence`：ACP/Coding 共用 `util.rs`；快照和兜底均在私有索引采集，不修改用户暂存区。RT-GIT-01～05（`crates/agents/tests/git_evidence.rs`）覆盖暂存边界、空仓库、链接 worktree、并发、失败及取消清理。失败明确返回证据不可用。每条 Git 命令期限30秒，复用进程组终止并回收后释放临时目录；不是浏览器测试。证据见 [R-FIRST-06](../testing/evidence/r-first-06.md)。

## HTTP 请求 Host 防护

`http-host`：正式 agentnexus/an-mom 在组装所有路由与静态 fallback 后统一应用 host_guard::protect，允许精确 localhost/127.0.0.1/[::1] 且端口匹配实际监听端口；不信任转发头。RT-HOST-01/02（`server/tests/host_guard.rs`）启动真实二进制，覆盖恶意 Host、写入无副作用、静态页、API、SSE、同源 MOM、合法 IPv6 authority 与非法/重复 Host。不是浏览器 DNS 重绑定复现，也不是完整身份认证。证据见 [R-20261106-S1](../testing/evidence/r-20261106-s1.md)。

R-20261113-L1：RT-HOST-03/04（`crates/server/tests/host_diagnostics.rs`）用正式二进制验证主服务诊断与JSON、独立MOM JSON的WARN `http.host_rejected` / `E_HTTP_HOST`，Host最多100字符、不记录请求体/其他请求头/查询串，且合法请求不新增拒绝事件。只补观测性、常量与文档，放行条件和403响应保持。证据见 [R-20261113-L1](../testing/evidence/r-20261113-l1.md)。

R-20261113-L1预审补充：RT-HOST-05/07覆盖独立MOM和主服务日志目录冲突时的控制台/内存诊断降级，原logs文件不变；RT-HOST-06覆盖约60KB拒绝路径在JSON和主服务诊断中截断到200字符，403与准入规则保持。共用日志初始化使用可失败builder，不改公开配置；证据见 [11-14预审修复](../testing/evidence/r-20261113-l1-pre-review-20261114.md)。

R-20261113-L1 11-15预审补充：RT-HOST-08验证文件日志降级时不再误报log.file；RT-HOST-09为正常目录成功提示的正向基线，控制台/JSON各一条且目录正确。成功事件由共用init在writer及订阅器安装后发出，不新增公开接口；证据见 [11-15预审修复](../testing/evidence/r-20261113-l1-pre-review-20261115.md)。

R-20261113-L1 11-16预审：RT-HOST-10确定性验证其他监听者不能满足自己的子进程就绪。host_log_limits/host_diagnostics共用support/service_readiness.rs，等待独立捕获的自身监听日志、存活状态与端口连接；只串行化启动交接。原业务断言不变，host_log_limits连续50次默认并行验证。证据见[evidence/r-20261113-l1-pre-review-20261116.md](../testing/evidence/r-20261113-l1-pre-review-20261116.md)。
R-20261116-L1：RT-HOST-11确定性验证外国监听者不能满足自身子进程就绪。host_guard共用support/service_readiness.rs，确认独立捕获的自身监听日志、Child存活及端口；只串行化启动交接，原RT-HOST-01/02断言不变，连续100次默认并行验证。证据见[evidence/r-20261116-l1.md](../testing/evidence/r-20261116-l1.md)。

R-20261118-M3：PY-DLP-M3以真实evaluate管线验证8种凭证全部BLOCK、非凭证Unicode保留，700条与留出集安全门槛照常执行。 证据见 [r-20261118-m3.md](../testing/evidence/r-20261118-m3.md)。

R-20261118-M5：内部使用含字符串键/整数数组索引的元组路径，阻止字面点号、方括号和空键产生串字段；重复分析路径在写Vault前BLOCK。field_path仍为原格式字符串，仅用于展示，特殊键的展示歧义不再参与寻址。 PY-DLP-M5与原有Python全量/评测门槛验证；[证据](../testing/evidence/r-20261118-m5.md)。
