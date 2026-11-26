# 浏览器验收用例（实现前定义）

所有用例使用真实网页与 Rust API，仅替换外部模型。新增验证基础设施，不修改现有业务功能。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-CONV | conversation | 新浏览器上下文与测试会话 | 发送问候、刷新、新建另一会话、切回 | 回答存在且无 think；消息 ID/内容重读一致；另一会话无串入 |
| E2E-FILE | tasks | 可用 General/Reviewer、临时目录 | 要求写报告，在 UI 查看完成并下载 | task COMPLETED、review pass、下载内容与 artifact 一致，磁盘文件实际写入 |
| E2E-APPROVE | approval | 模型将任务判为 high | 发审批请求，检查待人工，再点击批准执行 | 批准前无执行结果/文件；批准后实际执行并交付 |
| E2E-CANCEL | cancellation | 受控模型让 General 挂起 | 等执行中，点击取消任务，再点确认取消 | 第一次不取消；第二次变 CANCELLED，API 状态一致 |
| E2E-DIAG | observability | 已发送一轮消息，开发者模式默认关闭 | 开启开发者模式，查看诊断，再看路由日志原文 | 默认不显示诊断页签；开启后有真实后端，日志含该次用户消息和原文 |
| E2E-MOBILE | navigation | 窄屏与多个测试会话 | 打开会话抽屉、新建/选择会话 | 抽屉可操作、选择后关闭，发送框可用，内容不串会话 |
| E2E-TICKER | tasks | 受控模型让 General 写文件；另一会话让它挂起 | 完成后检查走马灯位置、展开历史；再发挂起任务看运行指示，取消 | 走马灯挂在简短回复下，最终正文在其后；收起显示最新一行，展开从旧到新且无“第 N 步”；背景与正文一致、字体更小；运行中行尾有动画 SVG，结束后消失 |
| E2E-FAILCARD | tasks | 受控模型对该任务返回 500，General 出错 | 发送后等任务 FAILED，展开走马灯历史 | 收起行显示失败原因；历史从旧到新、无编号列表；末尾即失败原因；不再有单独错误段和“失败”状态行 |
| E2E-ACK | conversation | 发送超过 80 字、结尾带标记的任务要求 | 等任务完成，检查第一条简短回复 | 简短回复完整包含用户意图（含结尾标记），不出现省略号 |
| E2E-ASSIGN | assignment | 受控模型让 General 写文件 | 等任务完成，看右侧任务卡标题，再展开 | 标题行含执行者 General Agent；详情“执行者”节显示 General Agent、主力、本机 |
| E2E-APPROVEINFO | approval | 模型将任务判为 high | 发送后、批准前查看对话区的待批准卡片 | 卡片写明将交给 General Agent、预计工具 write_file、触发规则“风险等级 High”，并有“批准执行”按钮 |
| E2E-CODEFLOW | agent-ui / git-evidence | harness 临时 git 仓库作 coding workdir；可接入 Claude（模拟 ACP） | 接入助手 → 发「改代码验证：…」→ 等完成 | 执行者为 claude；证据含 git_diff_stat 或 diff 产物含 hello.txt；review pass；workdir 为 git 仓且用户暂存区无残留 |
| E2E-FEEDBACK | feedback | 已有可展开的完成任务（与用户消息关联） | 任务卡展开 →「派错了」→ 选动作 → 保存 | 显示「已记下」；`POST /api/feedback` 成功 |

每次输出日志、测试结果和截图；失败保留 trace。正向通过只证明这些路径，未覆盖功能在地图中明确标注。另用校验器测试注入重复 ID、未知用例、缺文档和缺测试，确认地图失配确实失败。

## R-20261118-M2 JSON 键名检测

| Case | Given | When | Then |
|---|---|---|---|
| PY-DLP-M2 | external_untrusted；顶层、嵌套数组及路由对象的键名含合成凭证或租户主数据 | evaluate JSON | BLOCK且不可发送，不回显敏感键名、不写Vault；普通键与结构保持 |

## R-20261118-H1 嵌套路由载荷

| Case | Given | When | Then |
|---|---|---|---|
| PY-DLP-H1 | external_untrusted；to/attendees/cc下多层dict/list含合成凭证或客户名 | 调用evaluate；另注入字符串分析缺失 | 凭证及漏分析均BLOCK且不回传原值；客户名Token化，非字符串与结构保持 |

## MOM 独立服务验收

以下浏览器用例为现有 MOM 实现的补充基线，不追称为实现前 Red。运行时测试先行与业务失败修复证据另见 MOM 实施说明。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-MOM | mom | 独立 Mock 服务与新数据库 | 创建S1、切双视图、批准、下发、推进并核验 | 硬约束可见；批准后才下发；实际模拟回执核验后关闭 |
| E2E-COMPLAINT | mom | 同一服务中的S2客诉 | 查看未知根因和8D、批准初复、尝试关闭 | 初复不等根因调查；消息仅进模拟发件箱；缺D4/D6不允许关闭 |

## 运行时用例（Runtime 集成层，实现前定义）

浏览器用例之外，M4.3 固定模板的确定性校验（RT-TPL-*，测试函数前缀 `rt_tpl_NN_`）和 General 写研究报告的流程（RT-REP-*，前缀 `rt_rep_NN_`）没有独立界面入口，按设计在 Runtime 集成层验收。由 `web/verification/check-map.mjs` 检查映射。模板并行执行与汇合回复尚未接入编排器；研究报告的章节并行在 General 内部完成，见 DESIGN 9.5。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-TPL-01 | templates | 设计 9.6 中的 quote 模板原文 | 解析并校验（分支数上限 4） | 解析成功，校验通过 |
| RT-TPL-02 | templates | 分支能力名分别为 writer 和 general / coding / reviewer | 解析模板 | writer 解析失败；三个角色名均解析成功 |
| RT-TPL-03 | templates | 模板的分支中写入 depends_on 依赖字段 | 解析模板 | 解析失败，不存在隐藏的依赖边 |
| RT-TPL-04 | templates | quote 有 3 个分支 | 分别以后台容量 2 与 3 注册 | 容量 2 被拒绝且说明超过槽位；容量 3 注册成功 |
| RT-TPL-05 | templates | 注册 quote；Router 输出的三字段各不相同 | 调用 decide_split | 只有 yes 且模板与交付物都匹配时拆分；no、maybe、空值、未注册模板、交付物不一致均整单执行 |
| RT-TPL-06 | templates | quote 的 damage 分支声明 verdict 与 hours | 校验信封 | 字段齐全通过；缺字段或多字段均失败；branch_id 不一致失败 |
| RT-TPL-07 | templates | 信封 payload 中的值分别为数字、布尔、数组、对象、null 与字符串 | 反序列化信封 | 非字符串值全部解析失败；字符串值解析成功 |
| RT-TPL-08 | templates | parts 分支缺席；history 分支 payload 字段不全 | 汇合 | 两路都记为 missing，缺失原因非空，payload 为空，不用其他路数据填补；complete 为 false |
| RT-TPL-09 | templates | 信封的 branch_id 不在模板中；同一 branch_id 出现两次 | 汇合 | 两种情况都整体报错，不生成汇合输入 |
| RT-REP-01 | reports | General 只注册 write_file，受控模型一次 write_file 写完全文并带 answer | 执行任务 | Succeeded，summary 等于 answer，只调用模型 1 次，文件内容即这次写入 |
| RT-REP-02 | reports | 路由只要求 web_search（未同时要求 write_file，仍走逐步循环）；受控模型第一步就写文件 | 执行任务 | 先写的请求被打回，下一次请求含“先调用 web_search”；证据只有 web_search、write_file 各一次；文件含检索来源，没有凭记忆写的正文 |
| RT-REP-03 | reports | 路由同时要求 web_search 与 write_file；report_max_searches=6；受控模型规划 3 章（每章 1 个查询），再按章写正文，最后写摘要结论 | 执行任务 | Succeeded；模型调用 = 1 规划 + 3 章 + 1 汇合；检索 ≤ 6 且无重复查询；每个章节标题恰好一次；正文链接都来自检索结果；文件无「正文已省略」 |
| RT-REP-04 | reports | 受控模型写三步长文：overwrite、append、final（不要求 web_search） | 执行任务 | 写长文请求的 max_tokens 不小于 8192；第二次请求的历史只有 content_chars，没有“正文已省略”占位，也不回放正文；工具结果提示 mode=append |
| RT-REP-05 | reports | 同 RT-REP-03，report_parallel=2，三章写完顺序被打乱（第2章最快） | 执行任务 | 同时进行的章节模型请求 ≤ 2；落盘文件仍按大纲顺序第1→2→3章 |
| RT-REP-06 | reports | 同 RT-REP-03，但检索命令全部失败 | 执行任务 | Failed，错误含检索失败；不调用章节写作；工作区无报告文件 |
| RT-REP-07 | reports | 三章中第2章两次写作都失败，另两章成功 | 执行任务 | Succeeded（过半成功）；summary 写明缺失第2章；文件有第1、3章，无第2章正文标题 |
| RT-REP-08 | reports | 同 RT-REP-03，章节写作阶段人为挂起 | 执行中 abort 该次 run | abort 后不再发出新的模型请求；报告文件不再增长 |
| RT-REP-09 | reports | 规划与检索只有 [S1]；受控模型在第1章写了不存在的 [S9] | 执行任务 | Succeeded；正文无 `[S9]`；参考资料只有检索到的真实链接 |
| RT-EGR-01 | egress | `[egress] mode = off` | General 调用 web_search（参数含客户名与凭证样式串） | 不启动 DLP；工具收到原参数；DLP 无调用记录 |
| RT-EGR-02 | egress | enforce，web_search 已绑定 public-search，任务 task-42 | 调用 web_search（含客户名） | 工具只收到脱敏参数；DLP 收到的 task_id、channel、destination_id、purpose 来自任务上下文和配置 |
| RT-EGR-03 | egress | enforce | 参数分别含凭证、机密信息 | 分别返回 E_EGRESS_BLOCKED、E_EGRESS_REVIEW；错误信息不含原文；工具未被调用 |
| RT-EGR-04 | egress | enforce，current_time 未绑定 | 调用 current_time | 不经 DLP，工具正常调用 |
| RT-EGR-05 | egress | audit | 调用含凭证的 web_search | DLP 有调用记录；工具仍收到原参数（只记录不拦截） |
| RT-EGR-06 | egress | enforce；DLP 返回不符合 schema 的结构化结果 / 判定超时 | 调用 web_search | 启动失败或返回 E_EGRESS_UNAVAILABLE；工具未被调用；enforce 下守卫不得缺失 |
| RT-EGR-07 | egress | enforce；DLP 进程在第一次判定后退出 | 连续调用 4 次 | 至少 2 次成功（重新拉起 DLP）；工具从未收到未脱敏参数 |
| RT-EGR-08 | egress | enforce，cloud 后端已绑定 cloud-llm | 外发两条消息（含客户名）；再外发含凭证的消息 | 发出的消息已脱敏、角色不变；返回 Token→原值映射，restore 使用 local_ui；含凭证时返回 EgressBlocked |
| RT-EGR-09 | egress | enforce；DLP 未开启恢复 | 外发含客户名的消息 | 仍发送脱敏内容；映射为空（输出中保留 Token，不影响安全） |
| RT-EGR-10 | egress | enforce 配置 | 启动校验：绑定的工具不存在；web_fetch 未绑定；云端后端未绑定；audit 下云端后端未绑定 | 前三种拒绝启动并指明对象；本机后端无需绑定；audit 只给出警告 |
| RT-EGR-11 | egress | Runtime 启动，enforce，只绑定 web_search，web_fetch 已注册 | with_mcp_diagnostics 启动 | 启动失败，错误含 web_fetch |
| RT-EGR-12 | egress | Runtime 启动，enforce，web_search / web_fetch 均已绑定 | 启动后读取能力清单 | 启动成功；能力清单中没有任何 egress_* 工具 |
| RT-EGR-13 | egress | LLM 地址为云端地址 | 分别以 enforce 未绑定、audit 未绑定、enforce 已绑定启动 | enforce 未绑定启动失败（错误含后端名 default）；另两种启动成功 |
| RT-EGR-14 | egress | 未写 `[egress]`；另一配置 mode = audit | 解析配置；用同步构造创建 Runtime | 默认 mode = off；同步构造拒绝启用外发检查 |
| RT-EGR-15 | egress | 模型服务替身流式返回，Token 被切在两个数据块之间 | 带外发检查的客户端 chat_stream / chat | 模型服务收到的请求只含 Token；用户看到的增量与结果都是恢复后的原值 |
| RT-EGR-16 | egress | 外发检查判定 BLOCK | 调用 chat | 返回 EgressBlocked；模型服务没有收到任何请求 |
| RT-TPL-10 | templates | 后端 llama_cpp 配置 background_slots=[2,3,4,5]；openai 配置 max_inflight=8、reserved_interactive=1；openai 不限并发 | 读取 background_capacity | 分别为 4、7、不限（None） |
| RT-TPL-11 | templates | Router 输出 template_id=quote、deliverable=报价单、slots_ok=yes；模板已注册 | 委派消息走编排 | 创建 1 个 parent（fanout=root）+ 3 个 branch；事件含 template_split / 进度提及拆分 |
| RT-TPL-12 | templates | 同 RT-TPL-11；三路分支均返回合法 ok 信封 | 跑完 | join 完成；parent COMPLETED；会话交付含三路字段；汇合 complete |
| RT-TPL-13 | templates | 同 RT-TPL-11；一路返回坏信封 / 缺字段 | 跑完 | 该路 missing；交付写明缺失；parent 仍可 COMPLETED；complete 为假 |
| RT-TPL-14 | templates | slots_ok=no，或 template_id 未注册 | 委派 | 不拆分；仅一条普通任务（无 fanout=root） |
| RT-TPL-15 | templates | 已拆分且分支仍在跑 | 取消 parent | 子 ACTIVE→CANCELLED；不新建 join；parent CANCELLED |
| RT-TPL-16 | templates | 一支审查 needs_human | 未验收前 | 不启动 join；WP2 accept 该支后可继续汇合 |
| RT-TPL-17 | templates | 三支分支并发收尾（假 agent 立即返回） | 循环 400 个父任务 | 每个父任务恰好 1 个 join、1 条交付（H3 汇合只建一次；R-20261124） |
| E2E-TPL | templates | 浏览器；受控模型目标含「报价单模板」 | 观察任务卡并等待完成 | 可见拆分/多路；最终汇合向交付且 parent COMPLETED |

## MOM 可配置持续仿真（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-MOMDYNAMIC | mom | /mom/ 同源挂载与独立沙盘数据库（T-88 移除主界面沙盘入口，挂载不变） | 从 /mom/ 挂载进入MOM，调整订单数量和seed创建持续仿真，推进、播放/暂停并查看gap | /mom/ 同源挂载可达；配置影响方案；事件逐步进入案例；暂停与运行状态可见；gap显示台账全部条目（当前26项）的真实状态而非仅ID |

## 每日审查修复验收（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-MOM-01 | mom | S1 已触发安全联锁 | patch/refresh/approve/execute/delegate | 全部拒绝，状态、计划、动作及预算不变；HTTP 同样拒绝 |
| RT-MOM-02 | mom | CLOSED 或 CANCELLED 案例 | 再次取消或关闭 | 终态不变，不新增终态事件 |
| RT-MOM-03 | mom | 三个不同虚拟时刻的观测 | 查询事件 | ObservationRecorded.at 等于接收时虚拟时钟 |
| RT-MOM-04 | mom | 下发失败进入 PARTIAL | 重放 execute | 返回失败且不重复下发 |
| RT-MOM-05 | mom | 有运行子任务的案例被阻断 | 推进至租约到期 | 子任务 TIMED_OUT，不占用运行名额 |

## 执行者与审批说明（Runtime 集成层，实现前定义）

任务记录里的执行者（agent、执行组中的位置、排队时长）与 Policy 给出的候选执行者。测试函数前缀 `rt_assign_NN_`，在 `crates/runtime/tests/assignment.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ASSIGN-01 | assignment | 只有一个 general agent | 提交任务并等完成 | assignment 为该 agent（id、显示名、接入方式、位置），member_index=0、group_size=1、round=0 |
| RT-ASSIGN-02 | assignment | 执行组 [a, b]，各并发 1 | 同时提交两个任务，等两者都 RUNNING | 进入 RUNNING 时已有 assignment；一个是 a（0/2），一个是 b（1/2） |
| RT-ASSIGN-03 | assignment | 唯一 agent 并发 1，前一个任务占用 300ms | 再提交一个任务并等完成 | 第二个任务 waited_ms ≥ 200；第一个任务 waited_ms < 200 |
| RT-ASSIGN-04 | assignment | 旧库 tasks 表没有 assignment_json，policy_json 没有 candidates | 打开数据库并读取旧任务 | 自动补列且可重复打开；旧任务其他字段不变，assignment 为空，candidates 为空 |
| RT-ASSIGN-05 | approval | 执行组 [a, b]，高风险路由 | Policy 裁决 | 需审批；candidates 依次为 a、b 的“显示名（接入方式，位置）”；旧格式裁决可读出且 candidates 为空 |

## MOM × RelOS 关系上下文接入（E1-C，实现前定义）

an-mom 以 `--relos-url` 启用时，服务层在创建 S1 前异步调用 RelOS `POST /v1/context/compile`（5 秒超时、不重试），
把结果作为观测写入 `world.relos`（有 WorldChanged 事件留痕）；诊断工具只读取已记录的观测，引擎仍可确定性重放。
未启用时行为与原有 Mock 完全相同。RelOS 中的知识目前为仿真数据，不代表真实工厂诊断。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-MOM-06 | mom | S1 案例已记录 RelOS 上下文（status=ok，含候选根因与关系 ID） | 推进到诊断节点执行 | maintenance/quality 节点成功，输出候选根因与关系 ID，cause_status 仍为 HYPOTHESIS；证据来源为 RelOS、mock_ids=M24；未批准前无任何下发 |
| RT-MOM-07 | mom | RelOS 不可用 / 超时 / 返回空 | 推进到诊断节点执行 | 两个诊断节点 UNKNOWN 并带 relos_status；方案排除可疑设备；无下发，未批准的 execute 被拒绝 |
| RT-MOM-08 | mom | 写入 world.relos | 非对象、未知 status、超过 64KB | 拒绝写入；合法上下文可写入 |
| RT-MOM-09 | mom | 未启用 RelOS | 正常运行 S1 | 诊断输出与原有 Mock 一致（来源 EAM、M03） |
| HTTP-MOM-RELOS-01 | mom | an-mom 配置桩 RelOS | POST /api/mom/scenarios/s1 | 请求以 CNC-XX 为中心、含 Line-B 锚点与预算；Agent 视图中诊断节点带桩返回的候选根因与关系 ID |
| HTTP-MOM-RELOS-02 | mom | RelOS 地址无人监听 | POST /api/mom/scenarios/s1 | 10 秒内返回；案例照常创建，诊断 UNKNOWN、relos_status=unavailable，进入人工决策 |

## 评审 R-20261107-M1（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-MOM-10 | mom | 案例为 CANCELLED 或 CLOSED | 对允许的故障键调用 change_world（运行时及 HTTP fault 命令） | 返回错误/4xx；案例、资源版本、事件序号及事件列表全部不变；非终态仍可注入故障 |

## 专家工作流节点绑定与模板晋升（E1-C 方案 B，实现前定义）

把 data-collection 中"专家确认"的工作流图（`fixtures/mom/expert/workflows.json`）编译为带节点绑定的工作流模板：
每个节点按确定性规则 R1–R8 得到执行者（agent / human / system / agent_then_human / structural / unbound）、领域、工具、
技能与闸门；相邻同领域的 agent 节点合并为一段。只做编译、校验和统计，不接入编排器执行。
绑定未经专家确认的模板不晋升。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-BIND-01 | mom | 审批节点（带角色、SLA 时限、超时边） | 绑定 | 执行者为 human；闸门类型 approval，审批人=角色、时限取自 SLA、升级对象取自超时边；永不为 agent/system（R1） |
| RT-BIND-02 | mom | 机制为设备，或标签为物理动作（搬、贴、复测、停线…） | 绑定 | 执行者为 human 工作项（R2） |
| RT-BIND-03 | mom | 机制为系统（ERP/MES…），标签分读操作与写操作 | 绑定 | 执行者为 agent，工具为归一后的系统 ID；写操作同时列入 write_tools 并带 action_approval 闸门（R3） |
| RT-BIND-04 | mom | 判断节点：有明确阈值/关键词，或没有 | 绑定 | 有明确规则时为 system（确定性规则）；否则为 agent_then_human（R4/R5） |
| RT-BIND-05 | mom | 节点带控制文档与执行角色 | 绑定 | 控制文档成为技能；角色映射到领域（检验员→质量、维修工→设备、计划员→计划…）（R6/R7） |
| RT-BIND-06 | mom | 无角色、无链接、看不出动作；或系统不在工具清单 | 绑定 | 执行者为 unbound 并给出原因，不猜测（R8） |
| RT-BIND-07 | mom | 质量 agent→质量 agent→人工→质量 agent 的串行链 | 分段 | 前两个合并为一段（步骤按顺序），人工节点单独成段，之后另起一段；条件边保留为段间路由 |
| RT-BIND-08 | mom | 9 条真实专家工作流 | 全部编译并统计 | 每个节点恰好一张绑定卡、恰好属于一段；审批节点全为 human；所有模板通过校验；未经专家确认的模板一律不晋升；报告合计与逐条一致 |
| RT-BIND-09 | mom | 人为构造的错误模板（段间有环、写工具无闸门、工具不在清单、审批绑成 agent、节点遗漏） | 校验 | 每种错误都被拒绝并给出原因 |
| RT-BIND-10 | mom | 标签以信息动作开头但含物理词（"输出保养记录"）；含"自动"的动作；有信息动作但无角色 | 绑定 | 开头的信息动作优先，判为 agent；系统自动动作判为 system（R4）；无角色的信息工作判为 agent 并要求专家补充领域，而不是 unbound |

## Agent 准入与持续信任（E3 第 1 步：协议核心与持久化，实现前定义）

论文 §5.2 的七步协议（S1 提交 Agent Card → S2 澄清 → S3 能力测试 → S4 加权打分 → S5 契约 → S6 人工审批 → S7 激活与试用期）的确定性核心。
等级、抽样、警告、降级、撤权全部由程序决定；本步只实现账本与持久化，不接入编排器（接入见 E3 第 2 步）。
配置文件里写的 agent 视为运维已信任（预准入为 Trusted）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADM-01 | admission | 外部 agent 提交 Card | 按 S1→S7 推进，或跳步、答非所问 | 只能按顺序推进，跳步返回 E_ADMISSION_STATE；Card 自相矛盾（声明可派生子任务却无最大深度、未声明工作语言等）时生成澄清问题；回答数与问题数不符被拒；非法 agent_id 或未知能力维度被拒 |
| RT-ADM-02 | admission | S3 给出完整度、对话质量、测试三项分数 | S4 打分 | 默认权重 30/20/50 计算 0–100 总分；≥60 推荐 Member、≥80 推荐 Trusted；推荐等级不超过申请等级；权重可配置 |
| RT-ADM-03 | admission | 契约已签 | S6/S7 激活 | Trusted 未经人工审批不能激活；Guest 无需审批；Member 默认无需审批、配置要求时必须审批；激活签发凭证代际 epoch=1 |
| RT-ADM-04 | admission | 已激活 agent 完成大量任务 | 判定是否抽检 | 同一 seed、agent、任务结果确定；1 万个任务的抽中率在 0.17–0.23（默认 20%）；换 seed 抽中集合不同 |
| RT-ADM-05 | admission | 试用期抽检结论回写 | 连续不合格 / 合格穿插 / 累计警告 | 连续 2 次不合格记 1 次警告，合格清零连续计数；累计 3 次警告降为 Guest 并标记待人工复核，epoch 加 1，旧凭证失效，不再允许中风险任务 |
| RT-ADM-06 | admission | 已激活或预准入的 agent | 撤权 | 阶段变为 Revoked，epoch 加 1；旧凭证失效；不再允许任何风险等级；之后回写抽检被拒 |
| RT-ADM-07 | admission | 账本有多条记录与审计事件 | 重新打开同一数据库 | 记录完全恢复；审计事件按发生顺序列出 |
| RT-ADM-08 | admission | 只有旧表的旧数据库 | Store::open | 自动新建准入两张表，旧数据不受影响；重复打开幂等 |
| RT-ADM-17 | admission | 多个 agent，各自的任务编号连续且只在末尾几位不同（如 w-000…w-039） | 判定是否抽检 | 每个 agent 的抽中数都落在二项分布的合理范围内（不会全抽或全不抽）；所有 agent 合计的抽中率接近配置比例（回归：哈希高位混合不足导致抽样聚集） |

## Agent 准入接入编排器（E3 第 2a 步，实现前定义）

开启 `[admission]` 后：registry 只把任务分给准入等级允许该风险的 agent；配置文件里写的 agent（非 `a2a` 类型）没有记录时视为已信任；
编排器在接受结果前核对凭证代际，撤权时中止该 agent 正在执行的任务；外部 agent 完成的任务按试用期规则抽检。未开启时行为不变。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADM-09 | admission | 开启准入；执行组里有未准入、Guest、Member 的外部 agent 与配置 agent | 按风险分配 agent | 未准入的外部 agent 不分配，角色不可用时报 E_ROUTE_NO_AGENT 并说明准入原因；Guest 只领低风险；配置 agent 视为已信任，租约带 epoch=1；未开启准入时行为不变 |
| RT-ADM-10 | admission | 执行组主力与备份都是外部 agent，主力正在执行长任务 | 撤销主力 | 进行中的任务立即变为 FAILED（E_AGENT_REVOKED），不交付；执行名额归还；后续任务由备份完成 |
| RT-ADM-11 | admission | 外部 agent 执行期间因抽检警告被降级（epoch 变化） | 任务结果交回 | 凭证已过期，结果被丢弃，任务 FAILED（E_AGENT_REVOKED）、不交付；凭证有效时结果照常接受 |
| RT-ADM-12 | admission | 撤权与任务完成同时发生（多轮） | 并发执行 | 只有一个终态且不被覆盖；撤权报告中止了该任务 ⇒ FAILED 且无交付；COMPLETED ⇒ 恰好一次交付且不在中止清单中 |
| RT-ADM-13 | admission | 外部 agent 完成多个任务（抽检比例 100%） | 记录试用期并回写抽检 | 完成数累加；抽中的任务进入待检清单；只能对待检清单里的任务回写结论；配置 agent 完成任务不产生准入记录 |
| RT-ADM-16 | admission | 配置文件 | 加载与组装 | 不写 `[admission]` 时默认关闭、规则取默认值；开启后 Runtime 带准入账本；非法参数（如抽检比例 2.0）启动失败 |

## Agent 准入 HTTP 接口与外部 agent 适配器（E3 第 2b 步，实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADM-14 | admission | 开启准入的服务与一个桩外部 agent | 通过 `/api/admission/*` 走完 S1→S7（含审批）、派发任务、撤权、重启 | 每步返回正确结果；顺序错误返回 409、输入非法返回 400、未知 agent 返回 404；激活后外部 agent 出现在 /api/agents（kind=a2a）并完成任务；撤权返回新 epoch；重启后已激活的外部 agent 自动重新注册；未开启准入时这些接口返回 404 |
| RT-ADM-15 | admission | 外部 agent 的 HTTP 服务 | 适配器执行任务 | 请求带 task_id、goal、context 与风险；成功结果映射为 Succeeded 并附"外部 agent 输出未经核实"的证据；HTTP 错误、无效 JSON、超时都返回带错误码的失败；非 http(s) 地址被拒 |


## R-FIRST-05 补充要求路由（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-CONV-01 | conversation | 已有待审批任务 | 补充要求的路由遇到传输失败、格式失败或 Policy 拒绝 | 旧任务完整状态和事件不变，不新增任务 |
| RT-CONV-02 | conversation | 已有待审批任务 | 补充要求路由和 Policy 校验成功 | 只路由一次，取消旧任务，使用合并目标创建待审批新任务 |

## R-20261110-H1 已激活接入地址不可变（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADM-18 | admission | Trusted 已签约且已审批 | Contracted 登记/重登记 binding，然后激活再改 binding | Contracted 操作成功；Active 修改被拒绝，完整记录（含 epoch/binding）及审计事件不变 |
| HTTP-ADM-BIND-01 | admission | Trusted 已激活且 binding=good | 再 POST /activate 提交不同 endpoint/role/type/concurrency | 返回 409，完整记录、事件及注册卡片不变，无新增 admission.bound |

## MOM 三入口独立原型（非生产集成）

以下由 `docs/mom/prototype/check-entries.cjs` 驱动，不计入生产 Web 浏览器覆盖；既有节点和会话断言继续由 `check-nodes.cjs`、`check.cjs` 保护，导航定位随三入口契约调整，业务断言不减少。

| Case | Given / When | Then |
|---|---|---|
| P-MOM-ENTRY | 桌面或手机进入原型 | 仅看板、任务、资源三个主入口；流程、方案、交接在事项内 |
| P-MOM-SCOPE | 切换与我相关、本班组、本车间 | 样例事项分别2、1、3项；范围跨入口保持；只是查看范围，不宣称企业权限已实现 |
| P-MOM-STATE | 从看板批准A、从任务模拟调拨受理、从资源再次进入A | 同一事项ID、批准和受理状态一致；受理不等于刀具到位，不解锁生产 |
| P-MOM-RETURN | 列表筛选后进入事项，再返回 | 原范围、维度、分组和滚动位置保留；不回到笼统首页 |
| P-MOM-VIEWS | 查看任务及资源 | 已完成任务默认折叠，受阻展开；资源支持全部摘要、六维聚焦及缺口／全貌 |
| P-MOM-CONTEXT | 在A输入草稿、切至C、再回A | 不混入A内容，回A恢复草稿；交接仅引用当前事项 |

## Git 证据采集的索引隔离（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-GIT-01 | git-evidence | staged/unstaged/untracked/ignored/intent-to-add，触发兜底采集 | 真实索引字节、cached diff、标记和文件不变，未跟踪文件进入证据，忽略文件不进入 |
| RT-GIT-02 | git-evidence | 空仓库无索引，兜底采集 | 获得新文件证据，不创建真实索引 |
| RT-GIT-03 | git-evidence | 带空格路径的链接 worktree，并发兜底采集及快照 | 两次证据完整，真实索引不变 |
| RT-GIT-04 | git-evidence | Git 索引损坏，采集失败 | 明确报告证据不可用，不改变原索引，不伪报无改动 |
| RT-GIT-05 | git-evidence | 快照的 Git clean filter 正在运行时取消采集 | 有界终止本次进程组、回收私有索引目录；真实索引不变 |

## HTTP Host 防护（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-01 | http-host | 启动真实 agentnexus/an-mom 二进制，恶意 Host 请求写入口、页面、API、SSE、同源 MOM | 请求在业务副作用前返回403；业务记录不变；转发头不能绕过 |
| RT-HOST-02 | http-host | 本机 IPv4/localhost/IPv6 authority、错误端口、伪装域名、非法/缺失/重复 Host | 合法且端口匹配才允许；其余400或403 |

## ACP 自动发现预设（AG.0，实现前定义）

新增 4 个内置预设：`claude`（claude-agent-acp）、`codex-acp`（codex-acp）、`cursor`（cursor-agent acp）、`zcode`（zcode-acp-server），角色都是 coding。用临时目录中的假可执行文件模拟"已安装"，只检查文件、不运行。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-ACPD-01 | acp-discovery | PATH 只含临时目录，里面有 claude-agent-acp、codex-acp、cursor-agent、zcode-acp-server；auto_detect = true，构造 Runtime | 注册表出现 claude、codex-acp、cursor、zcode 四张卡片：kind=acp、role=coding、agent_type 等于 id、enabled=true |
| RT-ACPD-02 | acp-discovery | PATH 里只有一个无关的 `agent` 可执行文件，没有 cursor-agent | 不出现 cursor 卡片 |
| RT-ACPD-03 | acp-discovery | 用户在 [[acp.agents]] 写了 id = "claude"（不同命令、名称、角色），预设命令也已安装 | 只有一张 claude 卡片，名称和角色取用户配置 |
| RT-ACPD-04 | acp-discovery | PATH 为空目录、HOME 为空目录（系统固定目录无法隔离） | 构造 Runtime 不报错；4 个新预设的卡片出现与否和"命令能否被找到"一致：找不到就不出现 |
| RT-ACPD-05 | acp-discovery | 读取预设清单；PATH 同时有 opencode 和 claude-agent-acp | 前三个预设仍是 opencode、hermes、pi，命令与参数不变，新预设排在其后；opencode 与 claude 都被注册 |

## Agent 目录与握手检测（AG.1，实现前定义）

只读：`GET /api/agents/catalog` 列出预设目录及状态；`POST /api/agents/probe {"id"}` 对目录内或已注册的 ACP agent 握手并分类。不写配置。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-AGCAT-01 | agent-catalog | PATH 隔离：claude-agent-acp 已安装且自动发现开启；codex-acp 未安装 | 目录含全部 7 个预设；claude 为 registered 并带连接状态；codex-acp 为 not_installed |
| RT-AGCAT-02 | agent-catalog | 读目录 | 每项都有名称、用途、角色、启动命令和至少一条安装步骤 |
| RT-AGCAT-03 | agent-catalog | 已注册的假 ACP agent 握手正常 | kind = ok |
| RT-AGCAT-04 | agent-catalog | 假 ACP agent 在新建会话时返回 -32000（auth_required） | kind = needs_login，并给出下一步说明 |
| RT-AGCAT-05 | agent-catalog | 脚本向错误输出写 "env: node: No such file or directory" 后以 127 退出 | kind = missing_dependency；detail 含该错误输出 |
| RT-AGCAT-06 | agent-catalog | 脚本记录 pid 后长时间不响应 | 约 10 秒后 kind = timeout；该进程已结束 |
| RT-AGCAT-07 | agent-catalog | 目录内预设未安装 | kind = not_installed，立即返回 |
| RT-AGCAT-08 | agent-catalog | 未知 id、非 ACP agent（general）；请求体附带 command 字段 | 未知和非 ACP 返回 404；带 command 的请求被拒绝，命令未被执行 |
| RT-AGCAT-09 | agent-catalog | 检测已注册 agent 成功 | 健康接口中该 agent 的连接状态变为 connected |
| RT-AGCAT-10 | agent-catalog | 回归（2026-11-18 CI 偶发失败）：脚本向错误输出写 "env: node: No such file or directory" 后立刻以 127 退出，连续检测 50 次；另一个脚本不写任何错误输出就立刻退出 | 50 次全部 kind = missing_dependency（不能因为握手请求写失败报 Broken pipe 而归成 handshake_failed）；不写错误输出的脚本为 handshake_failed，且单次检测 2 秒内结束（补等错误输出有上限） |
| RT-I325-01 | agent-catalog | Issue #325：适配器创建时命令不存在，之后命令出现在磁盘（可完成 ACP 握手的真二进制） | 再次探测 | 首次探测 kind = not_installed；命令出现后再次探测重新解析并 kind = ok（握手成功）；is_installed 与体检口径一致 |
| RT-I325-02 | agent-catalog | 探测 kind = not_installed 但体检能找到命令（in_service_path 命中） | 生成诊断结论 | 规则 installed_need_recheck：标题「已安装，但检测时没找到」，给出重新检测/重启的明确下一步（不再显示"没有命中已知规则"） |

## 前台提速：先写理由 / 规则判风险 / 可疑时再思考（FT.1，实现前定义）

用假模型服务记录每个请求体，按脚本返回结果。"不思考"指请求带 `chat_template_kwargs.enable_thinking = false`；"思考"指不带这个设置（沿用模型默认）。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-FT-01 | front-thinking | 不写任何新配置（基线） | 前台请求不带 enable_thinking；turn_decision 没有 reason 字段；route_decision 的 reason 仍在最后 |
| RT-FT-02 | front-thinking | 不思考 + front_reason_first | turn_decision 与 route_decision 的第一个字段都是 reason，最长 40 字，且为必填 |
| RT-FT-03 | front-thinking | 只开 front_reason_first、不关思考 | schema 与基线相同（A 只在不思考时生效） |
| RT-FT-04 | front-thinking | risk_rules 开；模型给 low 的"删除生产环境数据库旧表"、给 low 的"修改 src/main.rs"、给 high 的"总结这篇文章" | 风险分别为 high、medium、high（只升不降） |
| RT-FT-05 | front-thinking | risk_rules 不开；模型给 low 的"删除生产环境数据库旧表" | 风险保持 low |
| RT-FT-06 | front-thinking | 不思考 + 升级；前台判断第一次输出不是 JSON，第二次合格 | 共 2 次请求：第 1 次不思考，第 2 次思考；结果取第 2 次 |
| RT-FT-07 | front-thinking | 不思考 + 升级；路由第一次给 high，思考后给 medium | 共 2 次路由请求，第 2 次思考；最终风险 medium |
| RT-FT-08 | front-thinking | 不思考 + 升级；路由第一次选了没有可用 agent 的 coding，思考后选 general | 路由成功，角色 general；第 2 次请求思考 |
| RT-FT-09 | front-thinking | 不思考 + 升级；前台判断第一次给 delegate 但 task_goal 为空，思考后合格 | 共 2 次请求，第 2 次思考；结果为 delegate 且目标非空 |
| RT-FT-10 | front-thinking | 不思考 + 升级，max_retries = 1；前台判断始终输出非 JSON | 返回错误；不思考的请求只有 1 次，之后都是思考请求（1 + max_retries 次），不会退回不思考 |
| RT-FT-11 | front-thinking | 不思考但不开升级，或开升级但没关思考；第一次输出非 JSON、第二次合格 | 两次请求的思考设置相同（与现在的重试行为一致） |

### FT.5 类别拆成两个是/否问题（实现前定义，2026-11-18）

开关 `[conversation] route_class_questions`，默认 false。打开后，路由模型不再直接选 L1–L4，而是回答 `needs_data_tools`（需要取数工具吗：精确计算、查当前时间、列出或读取工作目录文件、联网搜索都算；只为保存结果的 write_file 不算）和 `needs_document`（需要生成或修改文档、报告吗），由代码确定性算出类别。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-FT-12 | front-thinking | 不写 route_class_questions（基线） | route_decision 的 schema 有 task_class（枚举 L1～L4），没有 needs_data_tools / needs_document；系统提示词没有这两个问题的说明 |
| RT-FT-13 | front-thinking | route_class_questions = true | schema 没有 task_class；有布尔型的 needs_data_tools、needs_document，且都是必填，排在 execution_role 之前；系统提示词写明两个问题的口径（含"保存结果的 write_file 不算"） |
| RT-FT-14 | front-thinking | route_class_questions = true；模型依次回答 (否,否)、(是,否)、(否,是)、(是,是) | 路由结果的类别依次为 L1、L2、L3、L4 |
| RT-FT-15 | front-thinking | route_class_questions = true，同时不思考 + front_reason_first | schema 第一个字段仍是 reason，其后紧跟 needs_data_tools、needs_document |
| RT-FT-16 | front-thinking | route_class_questions = true，不改思考设置（开思考） | 请求不带 enable_thinking，schema 同样是两个是/否问题（开关与思考无关） |

### FT.5b 类别由工具清单算出（实现前定义，2026-11-19）

开关 `[conversation] route_class_from_tools`，默认 false。打开后，只对执行角色为 general 的路由结果重算类别：要取数 = 工具清单里有 write_file 以外的工具；要文档 = 工具清单里有 write_file，或模型给的类别是 L3/L4。提示词与请求不变。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-FT-17 | front-thinking | 不写 route_class_from_tools（基线）；模型给 general、工具 [read_file]、类别 L1 | 类别保持 L1 |
| RT-FT-18 | front-thinking | route_class_from_tools = true；模型依次给 general 且（工具 [read_file]，L1）、（工具 []，L2）、（工具 [write_file]，L1）、（工具 []，L3）、（工具 [read_file, write_file]，L1） | 类别依次为 L2、L1、L3、L3、L4 |
| RT-FT-19 | front-thinking | route_class_from_tools = true；模型给 coding、工具 []、类别 L4 | 类别保持 L4（只改 general） |
| RT-FT-20 | front-thinking | route_class_from_tools 开与关各路由一次同一任务 | 两次发给模型的请求（schema 与提示词）完全相同 |

## 托管配置、热注册与撤销（AG.2，实现前定义）

接口：`POST /api/agents/proposals`（只校验、不写入，返回提议与一次性确认凭证）、`POST /api/agents/proposals/{id}/apply {"token"}`。提议类型：add_preset、add_custom、set_default、disable、enable、remove、revert。托管文件放在数据库同目录的 `agents.managed.toml`。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-AGCFG-01 | agent-config | 自动发现关闭，claude-agent-acp 已安装；提议接入预设 claude 并确认 | 不重启即出现在注册表；目录状态为 registered；已做过一次握手检测；托管文件含 claude |
| RT-AGCFG-02 | agent-config | 两个 coding agent m1、m2（config.toml 没写执行组）；提议把 coding 默认设为 m2 并确认 | 新的 coding 分配得到 m2 |
| RT-AGCFG-03 | agent-config | 托管 agent 正在执行任务（持有名额）；停用它 | 新分配不再得到它；执行中的名额仍可用并能正常归还 |
| RT-AGCFG-04 | agent-config | 唯一的 coding agent 忙，另一个分配在排队；停用它 | 排队的分配报 E_ROUTE_NO_AGENT，说明里有"已被停用或没有安装"之类的大白话 |
| RT-AGCFG-05 | agent-config | 连续两次修改后撤销两次，再撤销一次 | 两次撤销依次恢复，注册表同步；第三次撤销被拒绝（没有可撤销的版本） |
| RT-AGCFG-06 | agent-config | 托管文件有自定义 agent、默认执行者，以及与已安装预设同 id 的托管 agent；重新启动 | 托管内容生效；同 id 时托管配置优先于自动发现的预设 |
| RT-AGCFG-07 | agent-config | config.toml 手写了 agent "hand" 和 coding 执行组 | 停用 hand、改 coding 默认、托管同 id agent 的提议都被拒绝，并说明要改 config.toml |
| RT-AGCFG-08 | agent-config | 确认时不带凭证、凭证错误、提议已过期 | 分别被拒绝（403、403、410）；托管文件和注册表都不变 |
| RT-AGCFG-09 | agent-config | 启动前托管文件内容损坏 | 启动正常；状态里报 E_AGENT_MANAGED_INVALID；坏文件内容不变；新提议被拒绝并说明原因 |
| RT-AGCFG-10 | agent-config | 自定义 agent 的 env 写了明文值；另一个只写 `${VAR}` 引用 | 写明文的提议被拒绝；写引用的被接受，托管文件里只有引用 |
| RT-AGCFG-11 | agent-config | 把两个不同类型的 agent 设为同一个执行组 | 提议被拒绝；注册表执行组不变 |
| RT-AGCFG-12 | agent-config | 两个不同的修改同时确认 | 两个都生效；托管文件是完整的合法 TOML，包含两处修改 |
| RT-AGCFG-13 | agent-config | 对话中派发写代码任务，但没有任何可用的 coding agent | 回复由前台模型生成，请求里带了原因和"用非专业用户能看懂的话"的要求；模型不可用时的兜底文字不以错误码开头 |
| RT-AGCFG-14 | agent-config | 写代码任务在执行阶段因没有可用 coding agent 失败 | 交付消息由前台模型撰写，提供给它的材料里有原因和恢复办法，并要求用大白话 |

## 助手页签与卡片（AG.3，实现前定义）

测试服务里：`claude-agent-acp` 已安装（包装脚本指向自带的模拟 ACP agent，握手真实）；`zcode-acp-server` 未安装；config.toml 手写了一个 agent `hand`；自动发现关闭；没有内置 coding agent。用例按顺序执行，共享同一个服务。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| E2E-AGFIRST | agent-ui | 新浏览器打开 设置与管理 → 连接与能力（T-88：发现卡自首页迁入） | 出现发现卡，列出可接入的助手；点"知道了"后刷新不再出现 |
| E2E-AGLIST | agent-ui | 打开「助手」页签 | 显示 Claude Code（已安装、未接入）、ZCode（未安装）、hand（在 config.toml 中配置）、内置的通用执行，每项有用途说明 |
| E2E-AGTOML | agent-ui | 查看 hand 一行 | 显示"在 config.toml 中配置"，没有"停用"按钮 |
| E2E-AGGUIDE | agent-ui | ZCode 一行点"看安装步骤"，再点"我装好了，重新检测" | 引导卡列出安装步骤，命令带"复制"按钮；检测结果用大白话说明"还没有安装" |
| E2E-AGCONNECT | agent-ui | Claude Code 一行点"接入"，在确认卡上确认 | 确认卡说明将会发生什么并显示将执行的命令；结果卡显示已接入和检测结果"已连通"；该行变为已接入 |
| E2E-AGUNDO | agent-ui | 在结果卡上点"撤销"并确认 | Claude Code 恢复为已安装、未接入 |
| E2E-AGDISABLE | agent-ui | 再次接入 Claude Code 后停用它，然后在对话里要求改代码 | 确认卡警告"写代码"类任务将没有执行者；之后对话中的回复是大白话，错误码只在末尾括号里 |

## 对话里管理助手（AG.4，实现前定义）

开关 `[conversation] agent_management`。受控模型按脚本给出前台判断；"不思考 / 思考"不涉及。卡片存在消息的 `card_json` 列，送给模型的历史只用 `content`。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-AGCHAT-01 | agent-chat | 开关关闭 | 前台判断请求的 schema 与现在相同（没有 manage_agents 和 agent_* 字段） |
| RT-AGCHAT-02 | agent-chat | 开关打开，claude-agent-acp 已安装；模型判断"接入 claude" | 回复消息带确认卡（提议 id、凭证、说明）；消息正文是大白话，不含凭证 |
| RT-AGCHAT-03 | agent-chat | 模型判断"接入"但没说是哪个，角色 coding | 回复选择卡，候选都是负责写代码、尚未接入的助手；在选择卡上点选（会话内提议接口）后，会话里追加一张确认卡 |
| RT-AGCHAT-04 | agent-chat | 模型判断"停用 hand"（config.toml 里写的） | 大白话说明要改 config.toml，没有卡片 |
| RT-AGCHAT-05 | agent-chat | 生成确认卡之后再发一条消息 | 送给模型的请求里没有凭证，也没有卡片数据，只有卡片消息的大白话正文 |
| RT-AGCHAT-06 | agent-chat | 用卡片里的凭证确认 | 同一会话追加结果卡；原确认卡状态变为 applied；助手已注册 |
| RT-AGCHAT-07 | agent-chat | 提议过期后再确认 | 410；原确认卡状态变为 expired |
| RT-AGCHAT-08 | agent-chat | 旧数据库的 messages 表没有 card_json 列 | 打开时自动补列，可重复打开；旧消息可读，卡片为空 |
| RT-AGCHAT-09 | agent-chat | 模型给的 agent_id 不在候选里 | 按格式不合格重试；第二次合格后正常生成卡片 |
| RT-AGCHAT-10 | agent-chat | 开关打开；模型判"撤销"或"列出"，但多填了助手名（例如 `cursor`）| `decide_turn` 返回的管理助手判断里助手名为空：撤销和列出与具体助手无关，由程序直接清空（AG.4 第三轮评测：und-01/04/05 属于这一类）|
| RT-AGCHAT-11 | agent-chat | 开关打开，发一条消息 | 发给模型的"管理助手"一节写明四条规则：没点名助手时不反问、助手名留空（由选择卡让用户点选）；让助手干一件具体的活是派任务，不追问细节；问"有哪些助手 / 默认是谁"走 list；撤销上一次修改是 undo、不是 enable，接入还没接的助手是 connect，enable 只用于恢复停用的。开关关闭时这一节不出现 |
| RT-AGCHAT-12 | agent-chat | 用户说「管理助手」「查看 / 修改 agent 配置」「接入 Claude」「撤销刚才对助手的修改」等；开关打开；即使模型误判为 delegate（去读配置） | 确定性拦截：走 manage_agents / 确认卡或发现卡，**不**创建交给 General 的任务，回复里也不引导去读 `config.toml` 当操作路径 |
| RT-AGCHAT-13 | agent-chat | 同上类输入，但开关关闭；模型若仍返回 delegate | 不派发任务；回复明确引导去右侧「助手」页签（或打开 `agent_management`），并说明如何指定执行者（页签「设为默认」、`[agents.assign]`、确认卡）；不要求 @worker 私聊 |
| RT-AGCHAT-14 | agent-chat | 用户问「怎么指定谁来做 / 要不要 @ 某个助手」 | 直接回复（不派任务）：说明助手页「设为默认」、对话确认卡、`[agents.assign]`；明确不需要 @worker 私聊 |
| RT-AGMGMT-01 | agent-chat | 任务目标是「管理助手 / 读配置改 agent / 接入助手」类（误从对话委派下来） | Router **拒绝**该目标（不调用模型选 General + 读文件工具）；错误说明应走对话管理助手或「助手」页签，不要派 General 读配置 |
| E2E-AGCHAT | agent-ui | 对话里说"把 Claude Code 接进来" | 对话里出现确认卡 → 确认 → 对话里出现结果卡；刷新后两张卡仍在，确认卡显示已确认 |
| E2E-AGCHOICE | agent-ui | 对话里说"帮我接个写代码的助手" | 对话里出现选择卡 → 点选一个 → 对话里出现确认卡 |

## 助手排查：规则库与本机体检（AG.5 第 1、2 层，实现前定义）

接口 `POST /api/agents/diagnose {"id"}`：先做一次握手检测，再做本机体检（只运行写死的只读检查），按规则给出大白话结论和修复步骤。不外发任何内容。RT-AGDIAG-05（诊断包脱敏）、07～12（外部助手排查）依赖 DLP，D1 定稿后再定义。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-AGDIAG-01 | agent-diagnose | 已安装的 claude-agent-acp 启动时向错误输出写"env: node: No such file or directory"并退出 | 检测分类为缺依赖；命中"找不到 Node.js"规则，给出大白话解释和安装步骤 |
| RT-AGDIAG-02 | agent-diagnose | 服务的 PATH 里找不到 claude-agent-acp，登录 shell 的 PATH 里能找到 | 命中"服务找不到命令、终端能找到"规则，解释里给出终端里找到的完整路径和修复办法 |
| RT-AGDIAG-03 | agent-diagnose | ZCode 要求 Node ≥ 22，本机 node 是 v18；Cursor 没有 Node 要求；node 命令卡住不返回 | ZCode 命中"Node 版本太低"；排查 Cursor 时不运行 node；node 卡住时 5 秒内结束并记为检查失败，不拖住整个排查 |
| RT-AGDIAG-04 | agent-diagnose | ZCode 的凭证文件存在但不可读；另一次凭证文件不存在 | 不可读时检查结果仍为"存在"（只看是否存在，不读内容）；不存在时命中"缺少 ZCode 登录凭证"规则 |
| RT-AGDIAG-06 | agent-diagnose | 自定义 agent 以未知报错退出 | 没有命中任何规则，结果标记为"未识别"；自定义命令不做登录 shell 检查 |
| E2E-AGDIAG | agent-ui | 「助手」页签里未安装的 ZCode 点"排查" | 显示排查结果：大白话结论"还没有安装"、安装步骤（带复制按钮）和体检项 |

## 助手排查：诊断包与请其他助手帮忙（AG.5 第 3、4 层，实现前定义）

用模拟的 DLP 服务（`crates/server/tests/fixtures/dlp_server.py`，按 `tools/dlp-egress/docs/INTERFACE.md` 返回结构化结果，经 `[egress]` 的外发检查守卫调用）和模拟的 ACP 助手 `an-mock-acp`（可记录收到的提示词和工作目录、可指定回复文字）。不依赖真实的 dlp-egress 和真实助手。

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-AGDIAG-05 | agent-diagnose | 出问题的助手错误输出里含家目录完整路径和本机用户名，配置里有带值的环境变量；分别在没有配置 DLP、配置了 DLP 时排查 | 结果里有诊断包：家目录换成 `~`，用户名、主机名、环境变量的值都不出现，环境变量只留名字；没配置 DLP 时注明"未经外发脱敏检查"；配置了 DLP 时诊断包是 DLP 返回的脱敏文本（渠道 `clipboard`、目的地 `user:clipboard`），用完即撤销 |
| RT-AGDIAG-07 | agent-diagnose | 选一个已连通的助手当帮手，生成预览，再用同意凭证发送 | 预览返回完整待发内容、目的地说明和一次性凭证；帮手实际收到的内容与预览一字不差；不带凭证、凭证错误、凭证用过、凭证过期都被拒绝，且帮手没有被启动 |
| RT-AGDIAG-08 | agent-diagnose | 发送后帮手请求写文件权限 | 权限被拒绝（帮手报告"没有获得写文件权限"），帮手的工作目录是临时空目录，结束后已删除 |
| RT-AGDIAG-10 | agent-diagnose | 帮手超过时限不回复 | 返回大白话的超时说明；帮手进程已结束 |
| RT-AGDIAG-11 | agent-diagnose | 一次发送还在进行时，再发第二次 | 第二次被拒绝，提示稍后再试；第一次正常完成 |
| RT-AGDIAG-12 | agent-diagnose | 选出问题的那个助手、未连通的助手、未知助手当帮手 | 都被拒绝（409 / 404），不生成预览 |
| RT-DLP-01 | dlp-egress | 开启了 `[egress]`（2026-11-18 合并 master 后由 `[dlp]` 改为 `[egress]`） | DLP 的三个工具不出现在能力目录（`GET /api/capabilities`）里，也不在 General 的工具清单里 |
| RT-DLP-02 | dlp-egress | DLP 把内容里的客户名替换成 `[CUSTOMER_001]`（ANONYMIZE）；帮手的回复里带着这个占位符 | 预览和发给帮手的内容里只有占位符、没有原文；显示给用户的帮手回复已还原成原文（context 为 `local_ui`） |
| RT-DLP-03 | dlp-egress | 内容里有疑似密钥（BLOCK，`secret_blocked`） | 不生成预览、不发出；大白话说明"有密钥或凭证，已拦下"并建议更换 |
| RT-DLP-04 | dlp-egress | DLP 返回 REVIEW | 不生成预览、不发送；说明需要人工判断 |
| RT-DLP-05 | dlp-egress | 没有开启 `[egress]`；DLP 超时；返回结构不合规；`api_version` 不是 "1.0"（与 INTERFACE.md 定稿一致） | 都不发送，说明原因；不出本机的排查（第 1～3 层）照常可用 |
| RT-DLP-06 | dlp-egress | 一次发送结束（成功或帮手超时） | 用同一个 `task_id` 调用了 `egress_revoke`；日志里没有待发内容的原文 |
| E2E-AGHELP | agent-ui | 检测失败的助手点"排查" → 看到诊断包（可复制）→ 选已连通的助手当帮手 → 看到完整预览和目的地 → 点"同意发送这一次" | 显示帮手的回复（占位符已还原） |

## Host 拒绝事件可观测性（实现前定义，R-20261113-L1）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-03 | http-host | 正式主服务收到恶意长Host的POST（带敏感请求体/认证头/查询串） | 403且无业务副作用；诊断与JSON各有一条WARN http.host_rejected / E_HTTP_HOST；method/path正确，Host最多100字符，不记录其他敏感输入；合法请求不新增拒绝事件 |
| RT-HOST-04 | http-host | 独立an-mom收到相同恶意POST | 403，JSON日志有WARN与统一错误码及截断Host；合法本机请求仍可访问且案例未创建 |

## R-20261113-L1 预审回归（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-05 | http-host | an-mom工作目录的logs是普通文件，启动并发送非法Host | 正常监听，原文件不变；降级提示及Host拒绝均输出控制台，403不变 |
| RT-HOST-06 | http-host | 主服务/独立MOM收到约60KB路径且Host非法 | 403；JSON中path最多200字符、单条记录小于2KB；主服务诊断同样限长 |
| RT-HOST-07 | http-host | 主服务遇到同一日志目录冲突 | 不panic；保留控制台及内存诊断，不覆盖原文件 |

## R-20261113-L1 11-15 预审回归（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-08 | http-host | 主服务配置logs目录但该路径是普通文件，启动并请求诊断 | 保留降级WARN，不宣告log.file，不覆盖原文件 |
| RT-HOST-09 | http-host | 主服务文件日志目录正常，启动并请求诊断（正向基线补录） | 控制台与JSON恰好各有一条log.file及正确目录，没有降级WARN |

## 子进程就绪身份验证（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-10 | http-host | 端口由其他监听者占用，自己的子进程仍活着但没有监听日志 | 不得报告自己的服务已就绪；原RT-HOST-03～09断言保持；host_log_limits连续50次0失败 |
## R-20261116-L1 子进程就绪（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-11 | http-host | 其他监听者占目标端口，自己的子进程活着却无监听日志 | 不得误报自己的服务就绪；RT-HOST-01/02断言不变；host_guard连续100次默认并行执行0失败 |

## MOM 车间 DES 阶段①：内核与场景生成器（开环，实现前定义）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §4、§5、§15。被测接口：`an_runtime::mom::des::run_open_loop`（场景生成与开环运行）与 `Engine::create_des_simulation` / `Engine::des_review_truth`（引擎接入、标准答案隔离）。默认工厂为 Line-B 上的 CNC-XX、CNC-XY、CNC-XZ（节拍 90 秒）；阈值 Cpk 1.33。阶段①只产生 order / vibration / quality 三类事件（D7），开环不含维修，磨损只增不减。测试函数前缀 `rt_des_NN_`，在 `crates/runtime/tests/mom_des.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-01 | mom-des | 默认工厂，并发簇模式，seed 1/7/42 | 同一输入各运行两次；另用不同 seed 运行 | 两次的事件流、标准答案和遥测序列化后逐字节一致；事件非空、ID 唯一、时间在仿真窗口内、只含三类事件且来源为 MES/EAM/QMS；不同 seed 的事件流不同 |
| RT-DES-02 | mom-des | 只开启"主轴磨损→尺寸"机制，与三种机制全关，seed 1–10 | 各运行 12 小时 | 开启时至少一个 seed 出现同一设备先振动报警、后 Cpk 报警，且该 Cpk 报警的标准根因含该设备主轴磨损；全关时没有任何质量报警、Cpk 始终 ≥ 1.33，但磨损轨迹与开启时完全相同、振动报警照常出现；磨损只增不减 |
| RT-DES-03 | mom-des | 只开启"主轴磨损→尺寸"，无维修，seed 1–10 | 运行 24 小时 | 每台设备的振动报警、质量报警各最多一次（报警锁存，持续越限不重复报警）；至少一个 seed 出现振动报警 |
| RT-DES-04 | mom-des | 引擎中创建并发簇模式的 DES 仿真 | 推进到仿真结束 | DES 事件进入案例；标准答案可由审核接口读到且含根因；任何案例的 agent 视图、人类视图、仿真列表中都不出现标准答案字段（root_causes、mechanisms、ground_truth、主轴磨损量等） |
| RT-DES-05 | mom-des | 两个引擎都创建同一个脚本仿真，其中一个再创建 DES 仿真 | 推进到同一时刻 | 脚本仿真案例和它的事件队列完全相同；DES 仿真出现在仿真列表中 |
| RT-DES-16 | mom-des | 默认工厂，seed 1–10 | 分别以固定剧本、并发簇、诱饵、级联、资源竞争、安静对照模式运行；再给出无法满足的约束 | 固定剧本按 start、+6、+14 分钟依次产生插单、振动、质量报警，根因为 CNC-XX 主轴磨损；并发簇在 60 分钟窗口内至少 3 个扰动且至少一对真实关联；诱饵的振动与质量报警在同一设备 60 分钟内出现，但质量根因不含主轴磨损；级联依次出现插单、振动、质量报警，并带"负荷加速磨损"和"主轴磨损共同原因"关联；资源竞争至少一组资源对应两个以上事件；安静对照无事件且 Cpk 始终 ≥ 1.33；约束无法满足时返回含 "constraint" 的错误而不是放宽约束，重复调用得到相同错误 |
| RT-DES-17 | mom-des | 默认工厂与"默认工厂 + Line-C 上的 CNC-YY" | 同一 seed、同一机制各运行 8 小时 | Line-B 三台设备的遥测完全相同；新设备出现在遥测中 |

## MOM 车间 DES 阶段②：工具读取、what-if 与仿真接口（实现前定义）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §6.3、§6.4、§9 及阶段②修订说明。what-if 与预测只用**可观测状态**（设备状态、振动、Cpk、刀具计数、资源配置），不读取主轴磨损等隐状态，也不复跑带随机数的仿真；因此它是计划工具，不是"看见未来"的神谕。默认资源（建模假设，取自 S1 夹具）：CNC-XZ 缺 φ32 刀具，仓库有 1 把备件，调拨 45 分钟；维修 240 分钟，1 个维修班组；"完成当前批次"30 分钟；检验 20 分钟。候选方案：A 继续使用报警设备；B 当前批次完成后停机维修、其余设备分担；C 立即停机维修、其余设备分担。runtime 测试在 `crates/runtime/tests/mom_des.rs`，HTTP 测试在 `crates/server/tests/mom_des.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-06 | mom-des | P0 固定剧本 DES 仿真推进到起点后 20 分钟 | 对 CNC-XX、500 件、16:00 交期做两次 what-if | 两次结果相同；引擎事件序号、仿真列表和全部案例完全不变；返回 A/B/C 三个方案，各含可行性、完成时间、是否按期、风险件数、违反项和所用设备；A 因 CNC-XX 处于报警状态带 `quality_risk:CNC-XX` 且不可行；B 的风险件数少于 A，C 为 0；推荐方案可行；结果标注 provenance=simulation、basis=observable_state，不含标准答案字段 |
| RT-DES-07 | mom-des | 同上；分别把备用刀具设为 0、把 CNC-XZ 设为未认证 | what-if | 默认资源下至少一个方案可行，用到 CNC-XZ 的方案带刀具调拨前置条件；无备刀时 B、C 带 `tool_unavailable:CNC-XZ`，未认证时带 `uncertified:CNC-XZ`，且不再使用 CNC-XZ；因此误期的方案带 `deadline_miss` 且不可行 |
| RT-DES-20 | mom-des | P0 固定剧本 DES 仿真 | 推进到起点后 30 分钟 | DES 案例的维护、质量、资源、排产四个节点成功；证据来源为 DES、provenance=simulation、含台账 M27；维护输出振动 ≥ 2.8 mm/s 且在 C 区，质量输出 Cpk < 1.33，资源输出显示 CNC-XZ 缺刀；排产输出与案例提案含 3 个方案和推荐项；agent 与人类视图不含标准答案字段 |
| RT-DES-21 | mom-des | 默认工厂扩充到 32 台和 33 台设备 | 开环运行与在引擎中创建 | 32 台可以运行；33 台被拒绝，错误说明设备数上限 |
| HTTP-DES-01 | mom-des | an-mom 服务，已用 S1 场景取得案例并创建 P0 DES 仿真 | 推进 30 分钟后读取工厂状态、趋势预测与 what-if | 创建返回仿真编号；工厂状态列出各设备振动、Cpk、刀具与资源，截至时间等于当前虚拟时间；预测按观测线性趋势给出到达阈值的估计分钟数并注明方法；what-if 返回 3 个方案；所有返回 provenance=simulation，且不含标准答案字段与主轴磨损量 |
| HTTP-DES-02 | mom-des | 同上 | what-if 数量为 0、交期超过 48 小时、未知仿真或设备；创建 33 台设备的仿真 | 均返回 400 及说明，2 秒内返回；之后工厂状态接口仍正常响应 |

## MOM 车间 DES 阶段③：方案选择、执行回写与核验（实现前定义）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §6.4～§6.7 与阶段③修订说明。阶段③起 DES 是闭环：执行动作作为"干预"记入仿真，从执行时刻起重新推演，之后的遥测、事件和标准答案随之改变；执行时刻之前的历史不变。被测接口：`Engine::select_des_plan`（从排产方案中选定一个可行方案，选定后需重新批准）、`Engine::des_inject_fault`（注入仿真故障）、执行与核验沿用现有 `approve` / `execute` / `close`；HTTP 新增命令 `select_plan`。runtime 测试在 `crates/runtime/tests/mom_des.rs`，HTTP 测试在 `crates/server/tests/mom_des.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-08 | mom-des | P0 固定剧本 DES 案例进入人工决策 | 选定方案 B、批准、执行，再推进到维修完成后 | 不可行的方案 A 不能被选定；维修动作在"完成当前批次 30 分钟 + 维修 240 分钟"后完成，调拨动作在 45 分钟后完成，而不是固定 2 分钟；维修期间工厂视图显示 CNC-XX 停机，调拨到达后 CNC-XZ 有刀；维修完成后 CNC-XX 恢复运行、振动低于 2.8 mm/s、Cpk 不低于 1.33；全部动作核验后案例可以关闭 |
| RT-DES-09 | mom-des | 同上，已按方案 C 执行 | 同一设备再到一条质量观测 | 观测作为核验证据记录（事件 `VerificationObservationRecorded`），不产生 `PlanInvalidated`，计划版本和阶段不变 |
| RT-DES-10 | mom-des | 同上，已按方案 C 执行 | 同一案例再到一张新插单 | 仍产生 `PlanInvalidated` 并进入人工处理（基线：既有行为，补录为回归保护） |
| RT-DES-11 | mom-des | 同上，选定方案 C 并批准后，仓库备件被注入故障清空 | 执行，再推进 60 分钟 | 刀具调拨动作失败，案例进入 PARTIAL 并产生 `CompensationRequired`；CNC-XZ 仍然无刀 |
| RT-DES-18 | mom-des | 资源竞争模式的 DES 仿真产生多个不同设备的报警案例 | 推进到仿真结束 | 同线、共用维修班组的报警案例之间互相记录 `RelatedCaseLinked`，理由写明共用资源；不与脚本案例关联 |
| HTTP-DES-03 | mom-des | an-mom 服务中的 P0 DES 案例进入人工决策 | 依次发送 select_plan(B)、approve、execute、advance、close 命令 | 每步返回 200；推进后工厂视图显示 CNC-XX 恢复运行且振动低于 2.8 mm/s；案例全部动作核验并成功关闭 |

## MOM 车间 DES 阶段④：审核层与批量实验（实现前定义）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §7、§8、§10 与阶段④修订说明。审核分三步：确定性规则检查 → AI 审核（`Reviewer` 接口，测试用脚本替身）→ 人工确认或推翻。规则：`hitl_before_write`（写操作之前有人工批准）、`root_cause_attribution`（记录的根因假设与标准答案比对）、`plan_safe`（执行的方案可行且不是"继续使用报警设备"）、`effectiveness`（维修完成后设备振动低于 2.8 mm/s 且 Cpk 不低于 1.33）、`timeliness`（各阶段耗时，仅信息）。AI 审核的每条结论必须引用输入中的证据编号（`rule:…`、`event:…`、`truth:…`）。审核模型按 `[models].mom_reviewer` → `[llm].model` 的顺序选择；与 MOM 主 agent 同模型时标注"非独立审核"。批量实验：同一组 seed 分别以 AI-Native 与传统 MOM 策略运行；两者都按推荐方案决策，差别只在决策延迟（传统策略为多次部门间交接的延迟之和，参数为建模假设）。runtime 测试在 `crates/runtime/tests/mom_des.rs`，HTTP 测试在 `crates/server/tests/mom_des.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-12 | mom-des | P0 案例；以及人为构造的案例记录 | 记录错误根因假设、再记录正确假设；构造"先执行后批准"的记录；执行方案 C 后在维修完成前后检查 | 错误假设使 `root_cause_attribution` 失败并注明误判，正确假设通过；先执行后批准使 `hitl_before_write` 失败，先批准后执行则通过；`effectiveness` 在维修完成前为待定，完成且设备恢复后通过 |
| RT-DES-13 | mom-des | P0 案例已执行 | 用脚本审核替身分别返回合规结论、报错、超时、引用不存在的证据 | 合规时报告 AI 状态为完成，每条结论引用的证据都在输入中；其余三种情况 AI 状态为不可用并写明原因（错误、timeout、invalid_output），规则检查结果照常在报告中；审核不改变案例和仿真状态 |
| RT-DES-14 | mom-des | AI 审核已完成 | 人工推翻但不写理由；写理由推翻；再确认同意 | 无理由推翻被拒绝；有理由推翻被记录，并生成一条 AI 与人工的分歧记录；同意不产生新的分歧 |
| RT-DES-15 | mom-des | P0 固定剧本，seed 1–100，AI-Native 与传统两种策略 | 运行两次批量实验并导出 CSV | 两次结果完全相同；每个 seed 两种策略各一行，共 200 行；CSV 有表头和 200 行数据；传统策略的平均决策耗时大于 AI-Native |
| RT-DES-19 | mom-des | 三份配置：配置了不同于 `[llm].model` 的 `mom_reviewer`；未配置；配置为与 `[llm].model` 相同 | 解析配置并选择审核模型 | 第一份使用 `mom_reviewer`、为独立审核；后两份为非独立审核并带标注；配置项被识别，不出现未知字段警告 |
| HTTP-DES-04 | mom-des | an-mom 服务配置了脚本审核替身，P0 DES 案例进入人工决策 | 记录根因假设、读取审核报告、触发 AI 审核、提交人工推翻 | 每步返回 200；报告含规则结果、AI 结论与分歧记录；案例的 agent 视图中不出现审核报告与标准答案字段 |

## DES 审查修复 R-20261117-M1 / L1（实现前定义）

移植自修复 PR #84（M1）与 #85（L1），按阶段③的干预与执行后延伸调整；修复在 DES 开发分支上完成。RT-DES-22/23（含补充的 22b）是 `crates/runtime/src/mom/des/engine_link.rs` 中的单元测试（需要只在单元测试中可见的重放计数器），不计入 runtime 集成测试映射；RT-DES-24 在 `crates/server/tests/mom_des_budget.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-22 | mom-des | 32 台设备、2880 分钟的 P0 DES 仿真，已有关联案例 | 逐分钟推进 2880 分钟 | 重新推演累计至多 1 次；多个时刻的案例快照与直接重放逐值一致，工厂视图一致。补充 22b（阶段③）：执行方案后推进到场景结束后 300 分钟，重新推演至多 2 次，维修完成后设备恢复运行 |
| RT-DES-23 | mom-des | 同上，快照序列化后恢复 | 依次读取工厂状态、预测、what-if，再推进 | 各入口共用至多 1 次重新推演；读取不改变持久状态；状态克隆不丢缓存；遥测和缓存不进入快照 |
| RT-DES-24 | mom-des | an-mom 服务；32 台设备、2880 分钟、并发簇（窗口 1 分钟、至少 1000 个扰动） | 创建该仿真，同时并发读取案例列表 | 创建在 2 秒内返回含 "constraint" 的 400，读取在 2 秒内返回 200；被拒绝的创建没有留下任何记录 |


## R-20261118-H2 协同事项关闭与安全联锁

| Case | Given | When | Then |
|---|---|---|---|
| RT-DES-31 | 资源竞争事项已执行推荐方案，一成员收到同源safety_trip并BLOCKED | 维修完成后关闭事项 | 安全成员保持BLOCKED和联锁事实及预留；发出含原因的CaseCloseSkipped；其他成员正常关闭 |

## MOM 车间 DES 阶段③b：按报警触发的模板与产线协同事项（实现前定义）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §6.8。对外约定：模板名 `DAG-TPL-DES-EQUIPMENT@1`、`DAG-TPL-DES-QUALITY@1`、`DAG-TPL-DES-COMBINED@1`、`DAG-TPL-DES-DIAGNOSIS@1`、`DAG-TPL-DES-LINE-INCIDENT@1`；协同事项是批次为 `DES-n:INCIDENT:…` 的案例，`world.des_incident` 记录产线、成员与报警设备集合，成员案例的 `world.des_member_of` 指向事项。测试在 `crates/runtime/tests/mom_des.rs`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-25 | mom-des | 并发簇、资源竞争、无约束三种模式，seed 1–5 | 推进到仿真结束 | 每个未关闭的 DES 报警案例都有模板：属于协同事项的成员用诊断模板且没有排产节点；其余按报警类型选模板（只有振动 → EQUIPMENT，只有质量 → QUALITY，两类都有 → COMBINED，插单与两类报警齐全 → 原 S1 模板），并有排产节点 |
| RT-DES-26 | mom-des | 并发簇，seed 1–5 | 推进到仿真结束 | 同一产线 60 分钟内有两个及以上 DES 案例时建立协同事项；事项使用 LINE-INCIDENT 模板，至少两个成员且成员都指回事项；任一时刻同一产线最多一个活动事项；事项的排产方案有 3 个、报警设备集合非空且都来自成员；成员加入后不再请求人工决策，加入前已请求的须在加入时撤回（`DecisionWithdrawn`，2026-11-17 用户批准方案 A 后修订） |
| RT-DES-27 | mom-des | 资源竞争模式中，某个协同事项的报警设备集合至少两台、有推荐方案 | 选定推荐方案、批准、执行，推进到全部维修完成，关闭事项 | 集合中每台设备都有维修干预，只有 1 个维修班组时维修时段互不重叠；维修完成后各设备恢复运行、振动低于 2.8 mm/s；关闭事项后成员案例一并关闭 |
| RT-DES-28 | mom-des | P0 固定剧本（基线） | 推进 30 分钟 | 只有一个 DES 案例，使用原 S1 模板，不建立协同事项，也没有成员关系 |
| RT-DES-29 | mom-des | P0 案例已执行方案 C 并关闭（基线，来自 2026-11-17 审查） | 同键再到一条质量观测，再推进 | 观测进入新案例而不是已关闭的案例；已关闭案例的 `world.des` 不再刷新 |

## 审查修复 2026-11-18：R-20261118-H2、M1、L1

问题与验收来自 [docs/review/2026-11-18.md](../review/2026-11-18.md)，决策见 [DECISIONS.md](../DECISIONS.md) DES-25、DES-26。H2 已由 #96 修复（上方 RT-DES-31），这里补充事项侧汇总。测试位置：`crates/runtime/tests/mom_des.rs`（RT-DES-32、33）、`crates/server/tests/mom_des.rs`（HTTP-DES-05）。R-20261118-L2 只改文档与示例配置注释，不涉及行为，没有用例。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DES-33 | mom-des | 资源竞争模式中一个已执行推荐方案的协同事项（同 RT-DES-27）；一个成员收到同源的 `safety_trip=true` 振动事件，进入 BLOCKED | 直接关闭该成员；推进到全部维修完成后关闭事项 | 直接关闭被拒绝；事项可以关闭，该成员仍为 BLOCKED、`safety_trip` 仍为 true，其他成员照常关闭；**事项上**逐个汇总被跳过的成员：一条 `CaseCloseSkipped`，写明成员与原因（含 safety）。H2 的主修复与成员侧记录由 #96 的 RT-DES-31 覆盖，本用例覆盖决策 DES-26 的事项侧汇总（原编号 RT-DES-31，合并 #96 后为避免重号改为 33） |
| RT-DES-32 | mom-des | P0 案例已执行方案 C | 先在没有 AI 审核时"同意"；再在 AI 审核降级（模型报错）后"同意"；然后 AI 审核得结论 A 并"同意"；再运行 AI 审核得结论 B | 前两次"同意"被拒绝。对 A 的"同意"记录 AI 审核版本。得到 B 后，AI 审核版本号增加，报告的 `human` 为空（还没有人对 B 下结论），历史记录中原来的"同意"标明针对的版本、`applies_to_current_ai = false` |
| HTTP-DES-05 | mom-des | 只有一个异步工作线程的服务，配置了脚本审核替身，P0 DES 案例 | 先在没有 AI 审核时 POST 同意；再让另一线程持引擎锁 2 秒，同时 POST AI 审核，并在请求期间每 50 ms 打一次心跳 | 同意返回 400。心跳的最大间隔小于 1 秒（处理函数不在异步工作线程上同步等锁）；AI 审核最终返回 200，状态 completed |

## 审查修复 2026-11-19：R-20261119-M2、M13、M14

问题与验收来自审查报告 `docs/review/2026-11-19.md`（分支 `review/2026-11-19`）。决策见 [DECISIONS.md](../DECISIONS.md) DES-29（由本线修）、DES-30（修法）。已转绿，证据见 [review-2026-11-19-des.md](evidence/review-2026-11-19-des.md)。

测试位置：
- `crates/des/tests/factory.rs`：RT-DESF-09、10；
- `crates/des-mcp/tests/stdio.rs`：RT-DESM-04；
- `crates/runtime/tests/mom_des.rs`：RT-DES-34；
- `crates/server/tests/mom_des.rs`：RT-DES-35。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DESF-09 | des-factory | 模拟工厂服务中已有一个 P0 仿真（M2） | 用 `start_minute = u64::MAX-10`、`horizon_minutes = u64::MAX`、维修耗时 `u64::MAX` 的工厂分别创建仿真；用 `start_minute = u64::MAX` 下维修单 | 都作为业务错误返回，不 panic；已有仿真的全部观测不变；下一个新仿真编号仍是 `DES-2`（被拒绝的创建不占编号） |
| RT-DESM-04 | des-factory | MCP 服务（stdio）已创建 DES-1（M2） | 用 `start_minute = u64::MAX-10` 创建仿真；再读 DES-1 的设备状态 | 创建返回工具错误（`isError`），不是 -32603 内部错误；DES-1 的设备状态与之前相同（服务没有因锁中毒而永久不可用） |
| RT-DESF-10 | des-factory | 默认工厂（1 个维修班组，维修 240 分钟）的 P0 仿真（M13） | 在当前时刻依次给 CNC-XX、CNC-XY 下维修单；再给 CNC-XX 下一张；再给 CNC-XZ 下一张；重发 CNC-XY 的单 | 第一张 [497, 737)；第二张排到 737 开始、977 完成；CNC-XX 维修未结束时的第二张被拒绝；CNC-XZ 排到 977 开始；重发返回原回执 |
| RT-DES-34 | mom-des | P0 案例已执行，AI 审核先后得到结论 A（版本 a）和 B（版本 b）（M14） | 按版本 a 提交"同意"；再按版本 b 提交"同意" | 按 a 提交被拒绝，错误含 `E_REVIEW_STALE`，报告的 `human` 仍为空；按 b 提交成功，记录的 `ai_version` 为 b |
| RT-DES-35 | mom-des | an-mom 服务配置了脚本审核替身，P0 DES 案例运行了两次 AI 审核（版本 a、b）（M14） | POST 人工结论：不带 `ai_version`；带 a；带 b | 不带时返回 4xx；带 a 返回 409；带 b 返回 200，报告中 `human.ai_version` 为 b |

## 模拟工厂与 MCP 挂载（阶段⑤a-1）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §11a。对外约定：
- 模拟工厂服务 `an_des::FactoryService`：调用入口是 `call(工具名, JSON 参数)`。
- MCP 服务 `an-des-mcp`：经 stdio 提供同一套工具。成功时返回 `structuredContent`，业务错误作为工具错误返回（`isError`）；管理口令取自环境变量 `AN_DES_ADMIN_TOKEN`。
- 工具名只用小写字母、数字和下划线，长度 ≤ 64，原因见 §11a 修订。
- 13 个工具分四类：
  - 观测：`equipment_status`、`equipment_trend`、`quality_capability`、`inventory_tooling`、`events_poll`、`order_status`；
  - 执行：`maintenance_order_create`、`tooling_transfer_create`、`production_schedule_apply`；
  - 控制：`simulation_create`、`simulation_advance`、`simulation_fault_inject`；
  - 管理：`simulation_ground_truth`。

测试位置：`crates/des/tests/factory.rs`（RT-DESF）、`crates/des-mcp/tests/stdio.rs`（RT-DESM）、`crates/runtime/tests/mom_des.rs`（RT-DES-30）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DESF-01 | des-factory | 工具目录 | 读取 `tool_catalog()` | 恰好是上述 13 个工具，类别与设计表一致。工具名唯一、匹配 `^[a-z][a-z0-9_]{0,63}$`，不含 AgentNexus 专有概念（agentnexus、case、mom）。每个工具有说明，输入 schema 是 object 且拒绝未知字段。执行类工具的必填字段包含 `idempotency_key` 与 `approval_ref` |
| RT-DESF-02 | des-factory | 两个服务实例，并发簇 seed 7 | 用相同输入创建仿真并推进到结束 | 事件逐字节相同，可观测状态相同。按游标分页拉取：遵守 limit；拼接结果等于一次拉取；同一游标重复拉取结果相同；不返回未来事件，没有重复；事件编号带仿真编号前缀；继续推进后，已投递的前缀不变 |
| RT-DESF-03 | des-factory | 两个服务实例，P0 | 一个在推进过程中反复调用全部观测工具，另一个不调用 | 观测可重复，且两个服务的最终世界相同（读取不改变世界）。状态标注 `provenance: simulation`，含振动、分区、Cpk、运行状态、刀具在位等可观测字段 |
| RT-DESF-04 | des-factory | P0 推进到报警后（CNC-XX 处于 C/D 区） | 下维修单：先缺审批引用、缺幂等键或审批引用为空白，再正常提交，然后同键重试，再同键改参数 | 缺字段的请求被拒绝且不改变世界。正常提交返回回执（单号、完成时刻）。同键同参数返回原回执、只生效一次（与只下一次单的世界相同）；同键不同参数被拒绝。`order_status` 从 accepted 变为 completed，维修后振动低于 2.8 mm/s。`production_schedule_apply` 返回 accepted 回执 |
| RT-DESF-05 | des-factory | P0 推进到报警后 | 先调拨刀具，再注入"备用刀具丢失"，再调拨一次 | 第一次回执 accepted，完成时刻等于当前时刻加调拨时间（取自 `inventory_tooling`）。注入后的调拨回执为 failed，原因来自仿真（提到 spares） |
| RT-DESF-06 | des-factory | 已配置管理口令的服务；未配置口令的服务 | 分别不带口令、带错误口令、带正确口令读取标准答案；读取全部观测工具 | 只有正确口令能读到标准答案；未配置口令时管理工具一律拒绝。观测输出不含 root_causes、mechanisms、spindle_wear、relations、contentions |
| RT-DESF-07 | des-factory | P0 推进 30 分钟 | 调用未知工具、未知仿真、带未知字段、时钟倒退或超上限、limit 为 0 或超过 1000、未知设备、维修开始时刻早于当前、未知单号、未知故障、时长为 0 的仿真 | 每个请求都在 2 秒内报错，世界不变 |
| RT-DESF-08 | des-factory | 一个服务 | 连续创建仿真 | 仿真编号形如 `DES-n` 且互不相同；达到上限时报错并说明 capacity |
| RT-DESM-01 | des-factory | 经 stdio 启动 `an-des-mcp` | 列出工具 | 与 `tool_catalog()` 一致（数量、名称、说明、schema）。注解如下：观测类 `readOnlyHint = true`；执行类 `readOnlyHint = false`、`idempotentHint = true`；控制类不是只读；管理类是只读 |
| RT-DESM-02 | des-factory | 一个 MCP 服务与一个进程内服务 | 依次执行同一组调用：创建、推进、拉取事件、四个观测工具、下维修单、同键重试、调拨、推进、再拉取、读取标准答案 | 每一步 MCP 的 `structuredContent` 与进程内结果逐字节相同 |
| RT-DESM-03 | des-factory | MCP 服务已创建一个仿真 | 调用未知仿真、错误口令、缺审批引用的写操作；然后关闭 stdin | 业务错误作为工具错误返回，不带数据，服务继续可用。未知工具可以是协议错误。关闭 stdin 后进程在 5 秒内正常退出 |
| RT-DES-30 | des-factory | P0 seed 1 与并发簇 seed 7 | 同一配置分别在 MOM 引擎（进程内）与模拟工厂服务中创建，在相同时刻推进 | 仿真编号相同，各时刻的可观测设备状态相同。服务事件等于内核事件加仿真编号前缀（种类、来源、设备、客户、时刻、载荷都相同）。服务的标准答案与审核层读到的相同 |

## 模拟工厂连接器（阶段⑤a-2）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §11a"⑤a-2 设计"，决策见 [DECISIONS.md](../DECISIONS.md) DES-27。

对外约定：
- 服务层连接器 `an_server::mom_des_connector::DesConnector`（经 stdio 启动 `an-des-mcp`）；
- 引擎的远程仿真模式 `an_runtime::mom::des::remote`（`create_remote_des_simulation`、`des_ingest_remote`、`des_pending_writes`、`des_record_write_receipt`、`des_set_remote_truth`）；
- 错误码 `E_DES_UNAVAILABLE`。

测试在 `crates/server/tests/mom_des_connector.rs`。测试会找 `an-des-mcp` 可执行文件：优先用环境变量 `AN_DES_MCP_BIN`，否则在同一 target 目录中找，找不到时构建一次。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DESC-01 | des-factory | 同一配置（P0 seed 1、并发簇 seed 7，240 分钟）分别在进程内引擎与"连接器 + 模拟工厂"中创建 | 两边逐分钟推进；每当出现等待决策且有推荐方案的案例，两边做同样的选定、批准、执行（MCP 模式随后由连接器发出写操作） | 仿真编号相同；结束时两边每个案例的批次、阶段、模板、事件种类序列、`world.des` 设备快照、排产方案（编号、可行性、完成时刻）、动作（工具、状态、完成时刻）完全一致 |
| RT-DESC-02 | des-factory | MCP 模式 P0，案例等待决策 | 先在未审批时执行；再审批并执行；把待发写操作发出一次但不记录回执，再正式发出一次 | 审批前没有任何写操作；未审批的执行被拒绝且不产生写操作。审批执行后的写操作包含维修单，幂等键以案例编号开头、等于参数中的 `idempotency_key`，并带上指向该案例的审批引用。重发返回与第一次相同的单号（同键只生效一次）；回执记录后不再有待发写操作；维修动作的完成时刻与回执一致，动作回执含单号 |
| RT-DESC-03 | des-factory | MCP 模式 P0 | 读取开放给 agent 的工具；以 agent 身份调用观测工具，再调用执行、控制、管理工具 | 开放的工具恰好是 6 个观测类工具。观测调用成功；其余三类被拒绝（错误含 not allowed），且没有发给模拟工厂（被拒绝的推进没有改变时钟） |
| RT-DESC-04 | des-factory | 命令不存在；或模拟工厂运行中进程被杀 | 启动连接器；推进 | 启动在超时内报 `E_DES_UNAVAILABLE`；进程退出后推进报 `E_DES_UNAVAILABLE`，引擎时钟与案例数不变（不静默推进、不写假观测），引擎中也没有进程内仿真可以回退（`des_factory_view` 报错） |
| RT-DESC-05 | des-factory | 同一配置分别在进程内与 MCP 模式创建 | 读取审核用标准答案；两边对同一案例记录同一根因假设并取审核报告 | MCP 模式的标准答案与进程内相同（来自模拟工厂管理入口）；根因规则结果相同且为 pass |

### 阶段⑤a-3：an-mom 接入模拟工厂连接器（2026-11-19 用户确认）

设计见 [DES_CLOSED_LOOP.md](../mom/DES_CLOSED_LOOP.md) §11a"⑤a-3 设计"，决策见 [DECISIONS.md](../DECISIONS.md) DES-28。

对外约定（新增，均为增量）：
- 启动参数 `an-mom --des-mcp <命令>`（或环境变量 `AN_MOM_DES_MCP`）：启动时经 stdio 拉起模拟工厂；启动失败则 an-mom 退出并报 `E_DES_UNAVAILABLE`，不回退到进程内仿真；
- `AppState::with_des(连接器)`；`mom::clock_step(&AppState)`：共享时钟走一步（后台时钟每秒调用一次）；
- `POST /api/mom/des/simulations` 返回新增字段 `source`（`mcp` 或 `in_process`）；
- `GET /api/mom/des/tools`、`POST /api/mom/des/tools/{tool}`：桌面与会话经连接器调用观测工具；
- 模拟工厂不在线时 HTTP 返回 503，正文 `code` 为 `E_DES_UNAVAILABLE`。
- 模拟工厂工具 `simulation_create` 增加可选参数 `simulation_id`（`DES-正整数`，已存在则拒绝），连接器传入引擎的下一个编号；
- an-mom 启动时打印实际监听地址（`--bind 127.0.0.1:0` 时为系统分配的端口）。

测试在 `crates/server/tests/mom_des_connector.rs`（RT-DESC-12）与 `crates/server/tests/mom_des_http_mcp.rs`（RT-DESC-06～11）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-DESC-06 | des-factory | 已构建的 `an-mom` 与 `an-des-mcp` | 运行 `an-mom --help`；用不存在的命令启动 `an-mom --des-mcp`；用真实的 `an-des-mcp` 启动并经 HTTP 创建 P0 仿真、读取工具清单 | 帮助中有 `--des-mcp`；命令不存在时 15 秒内以非零状态退出，错误输出含 `E_DES_UNAVAILABLE`；真实启动后创建返回 200、`source` 为 `mcp`，工具清单恰好是 6 个观测工具 |
| RT-DESC-07 | des-factory | 两个 an-mom 服务：一个进程内，一个接了模拟工厂 | 两边各用 S1 场景取得案例，再创建 P0（seed 1，240 分钟）仿真，用 `advance` 命令推进 120 分钟 | 两边仿真编号相同；`source` 分别为 `in_process` 与 `mcp`；所有 DES 案例（批次以仿真编号开头）的批次、阶段、事件种类序列完全一致，且至少有一个 DES 案例 |
| RT-DESC-08 | des-factory | 接了模拟工厂的 an-mom，P0 案例等待决策 | 经 HTTP 先执行（未审批），再选定推荐方案、审批、执行，然后再推进 30 分钟 | 未审批的执行返回 400，且没有发出写操作；审批后的执行返回 200，返回时写操作已经发出：至少一张维修单，审批引用指向该案例；执行返回的案例中维修动作的回执含单号；之后推进不重发（已发写操作数不变） |
| RT-DESC-09 | des-factory | 接了模拟工厂的 an-mom，已有 S1 案例（共享时钟已过仿真起点），已创建 P0 仿真，播放 5 分钟 | 调用 `clock_step` 5 次；再调用 1 次（播放已结束）；重新播放后杀掉模拟工厂进程再调用 1 次 | 前 5 次后引擎时钟前进 5 分钟，模拟工厂 `equipment_status` 的观测时刻等于引擎时钟；第 6 次时钟不动；杀掉后 `clock_step` 报 `E_DES_UNAVAILABLE`，引擎时钟不变，播放被暂停 |
| RT-DESC-10 | des-factory | 接了模拟工厂的 an-mom，已有 S1 案例与一个 P0 仿真，然后杀掉模拟工厂进程 | 创建仿真；对 S1 案例发 `advance` 命令；经工具接口读取设备状态 | 三者都返回 503，`code` 为 `E_DES_UNAVAILABLE`，10 秒内返回；引擎中仿真数与时钟不变（不回退到进程内仿真、不静默推进） |
| RT-DESC-11 | des-factory | 一个进程内 an-mom；一个接了模拟工厂、已创建 P0 仿真并推进 30 分钟的 an-mom | 读工具清单；调用 `equipment_status`、`simulation_ground_truth`、`simulation_advance`；读工厂视图、what-if、趋势预测 | 进程内服务的工具清单返回 400（没有接模拟工厂）。接了模拟工厂的：工具清单恰好 6 个观测工具；`equipment_status` 返回 200 且 `provenance` 为 `simulation`；管理与控制工具返回 403（含 not allowed），被拒绝的推进没有改变模拟工厂时钟；工厂视图与模拟工厂 `equipment_status` 的结果相同、截至时刻等于引擎时钟；趋势预测与 `equipment_trend` 的结果相同；what-if 返回 3 个方案；三者都不含标准答案字段 |
| RT-DESC-12 | des-factory | 连接器 A 创建 P0 仿真，案例审批执行后留下待发写操作（未发出）；随后换成新的连接器 B（模拟工厂新进程，相当于 an-mom 重启） | B 发出待发写操作；B 创建新仿真并推进；另外直接调用模拟工厂服务，用已存在的编号或格式不对的编号创建仿真 | B 发出 0 个写操作且不报错，A 的仿真的待发写操作原样保留；B 的新仿真编号为 `DES-2`（与旧仿真不冲突，模拟工厂中也是这个编号），照常创建和推进（旧仿真不会卡住新仿真）；重复编号与格式不对的编号都被拒绝 |

## R-20261118-M3 常见凭证与同形字

| Case | Given | When | Then |
|---|---|---|---|
| PY-DLP-M3 | 中文密码/口令、token=、DB_PASS=、OpenAI项目key、Google key、同形字AWS/GitHub，共8种合成输入 | evaluate到external_untrusted | 全部BLOCK、不可发送且无原值；其余原文（含℃）不变；正常Unicode和术语不被改写 |


## AI Desktop A1（壳层与导航；断言冻结）

设计见 `docs/ai-desktop/DESIGN.md`，追溯 `docs/ai-desktop/TRACEABILITY.md`，决定 61。A 阶段 SMT-03 允许纯 Mock（`TASK-2026-018`）。浏览器用例：`web/e2e/desktop.spec.ts`（默认 harness 18787）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-ADNAV | ai-desktop | 干净浏览器；Mock 首页种子含 SMT-03 | 打开 `/`；从首页卡打开任务；返回桌面；再从侧栏「最近」打开同一任务；查看运营入口 | 见「我的 AI 桌面」与三区；工作区 Task ID 为 `TASK-2026-018`；返回后仍在桌面首页；再次打开仍为同一 ID；`[mom]` 未开时无「运营桌面」页签且可见「运营模块未启用」说明 |
| E2E-ADLAYOUT | ai-desktop | 已从首页打开 SMT-03 工作区 | S2→S3→S2；刷新页面 | 仍在同一 Task；布局偏好恢复为离开前的 S2 或 S3；执行区展开状态可观察 |
| E2E-ADLAYOUT-CHU7 | ai-desktop | 首页快照夹具；本地模拟模型 | CH-U7（#321，INTENT-2026-020）：360/768/1024/1440 四宽度下测页面滚动；首页与工作区把内容区滚到底再滚到顶（展开执行构造溢出）；1024 宽会话页发消息后滚消息区 | 页面 scrollHeight ≤ 视口（无页面级滚动）；首页 S1 输入框与工作区输入框滚动前后位置不变且在视口内；会话页输入框位置不随消息区滚动变化 |
| E2E-UX01 | ai-desktop | 首页快照夹具含 SMT-03（标“模拟”） | 体验地图 UX-01：卡片进入工作区 → 写草稿 → 打开再关闭证据 → S3 再回 S2 → 打开设置后 Esc → 返回桌面再从卡片进入 | 每一步对话输入可用；草稿与任务恢复；关闭证据回到原工作区；全程无整页跳转；探针记录步数与整页跳转（基线补录，见 docs/experience-map/journeys/UX-01.md）；前置清理已随 UXFIX-D 移除 |
| E2E-UX01-I7 | ai-desktop | 路由拦截注入服务端旧草稿（另一会话，PUT 一律 409 应答） | UXFIX-D（INTENT-2026-008）：打开 SMT-03 工作区 → 远端快照返回前输入新内容 → 返回桌面再从卡片进入 | 本会话新输入保留、不被旧草稿覆盖；出现含“其他设备”的冲突提示；可一键改用其他设备草稿（单元级并发序列另见 draftStore.test.ts） |
| E2E-UX01-I8 | ai-desktop | 首页快照夹具含 SMT-03 | UXFIX-D（INTENT-2026-011）：工作区发送消息 → 出现“已受理（模拟）”回执 → 返回桌面再从卡片进入 | 发送记录含已发送正文与受理回执，离开再返回后仍在原位 |
| E2E-UX02 | ai-desktop | 首页快照夹具含 SMT-03 | 体验地图 UX-02：进入 SMT-03 工作区 → 写任务草稿 → 切“独立话题” → 输入并发送临时问题 → 切回“继续当前任务” | 全程在同一任务工作区；作用域按钮状态正确切换并恢复；回到任务后原任务草稿仍在（UXFIX-D，INTENT-2026-013）；无整页跳转（基线补录，见 docs/experience-map/journeys/UX-02.md） |
| E2E-UX01-I5 | ai-desktop | 首页快照夹具；本地模拟模型 | UXFIX-A（INTENT-2026-001）：首页 S1 输入问题 → 点“问答” → 浮层出现助手回答 → 再问一次（会话接口拦截为不可达） | 回答来自真实会话（不含“问答（模拟）”文本）；全程留在桌面、首页卡片仍在、无整页跳转；接口不可达时浮层显示“不可用”说明而非模拟文本 |
| E2E-UX03 | ai-desktop | 首页快照夹具含 SMT-03；本地模拟模型 | 体验地图 UX-03：首页输入一句话目标 → 点“委派任务” → 打开 SMT-03 工作区 → 返回桌面 → 等待完成 → 从“我的工作”卡片打开该任务再返回 | 委派后仍在桌面，受理状态可见，首页输入清空可用；可进出其他任务；委派的任务出现在“我的工作”并显示“已完成”；从卡片打开后工作区显示该任务自己的目标、“已完成”与产物名，不出现夹具模拟内容（UXFIX-E，INTENT-2026-009/014，卡片与工作区状态词一致）；无整页跳转（基线补录，见 docs/experience-map/journeys/UX-03.md） |

| E2E-ADTESTCTRL | ai-desktop | 自管服务器实例（无 [desktop] 段，test_fixtures=false）；同仓 dist | UXFIX-B（INTENT-003）：读快照 test_fixtures 标志 → 打开「设置与管理」→ 助理面板 | 快照 test_fixtures=false；设置面板无「注入冲突（测试）」。工作区模拟按钮与证据面板身份切换/空列表夹具的门控由组件级测试覆盖（workspaceView.test.tsx：关闭时不渲染、开启时渲染，harness 下现有 E2E 不变） |
| E2E-UX02-I4 | ai-desktop | 首页快照夹具 | UXFIX-G（INTENT-2026-012）：工作区写草稿 → 点左栏「我的助理」→ 面板对话输入输入文字 → 「调整助理」进设置 → Esc 关闭设置与面板 → 回到工作区 | 助理面板有可编辑的对话输入与最近消息区；面板与设置表单不出现内部术语 Soul/Taste（设置用「沟通方式」「判断标准」，DESIGN §7 展示名同步）；关闭面板后任务 ID 与草稿不变；无整页跳转 |
| E2E-UX03-I5 | ai-desktop | 首页快照夹具；本地模拟模型 | UXFIX-H（INTENT-2026-015）：首页委派 → 受理提示“后台处理中” → 打开其他任务再返回 → 等任务终结 | 受理提示不再显示“后台处理中”（同步为完成状态）；助理消息区出现该任务的完成通知（普通优先级、按任务去重）含“查看结果”；点“查看结果”直达该任务工作区 |
| E2E-ADSETGROUPS | ai-desktop | 首页快照夹具 | UXFIX-F（INTENT-2026-010）：打开「设置与管理」→ 检查分组列表 | 不出现「（占位）」字样；未实现分组（工作与桌面/通知与主动性/设备与语音/隐私与权限）显示「即将提供」且按钮禁用；「我的 AI 助理」「连接与能力」可用 |
| E2E-UX03-I7 | ai-desktop | 首页快照夹具 | UXFIX-I（INTENT-2026-016）：S1 输入为空 → 「委派任务」状态与点击结果 → 填写目标后恢复 | 输入为空时按钮禁用并提示「先说说要做什么」；点击不产生受理提示/任务 chip；填写后按钮恢复可用 |
| E2E-UX04 | ai-desktop | 本地模拟模型 | 体验地图 UX-04：“＋ 新会话”发任务 → 等完成 → 在对话输入写追问草稿 → 点右侧任务条目展开详情 | 任务详情展开；对话输入仍可编辑、草稿不变；无整页跳转（基线补录，见 docs/experience-map/journeys/UX-04.md） |
| E2E-UX05 | ai-desktop | 首页快照夹具含 SMT-03；提醒经测试 API 注入（`desktop.test_fixtures`） | 体验地图 UX-05：S1 输入框写字并聚焦 → 注入关联 SMT-03 的普通提醒 → 重新打开桌面 → 点“为你关注”中的提醒 → 返回桌面 | 提醒浮层出现且不抢焦点、不清空输入；“为你关注”中的提醒直达 SMT-03 工作区；返回后提醒仍在；处理过程无整页跳转（基线补录，见 docs/experience-map/journeys/UX-05.md） |
| E2E-UX05-I3I4 | ai-desktop | 首页快照夹具；提醒经测试 API 注入（不刷新页面） | UXFIX-N（INTENT-2026-021/023）：桌面打开时注入提醒 → 浮层与“为你关注”出现 → 点浮层“稍后” → 再注入第二条 → 点浮层“打开” → 返回桌面 → 点“忽略” | 不刷新时“为你关注”实时出现提醒；浮层展示原因与来源（普通建议 · 关联任务 TASK-2026-018）；稍后浮层收起且“为你关注”仍在；打开直达关联任务；忽略时明示会移除，浮层与“为你关注”一致移除 |
| E2E-UX06 | ai-desktop | 首页快照夹具含 SMT-03 | 体验地图 UX-06：SMT-03 工作区写草稿 → “调整助理” → 改助理名称并保存 → 查看记忆 → 返回助理分类 → 关闭设置 → 返回桌面（末尾恢复原名称） | 捷径直达“我的 AI 助理”分类；记忆子层返回原分类；关闭后回到同一任务且草稿不变；首页显示新名称；无整页跳转（基线补录，见 docs/experience-map/journeys/UX-06.md） |
| E2E-ADDRAFT | ai-desktop | 首页可打开 SMT-03 与产品改进两卡 | 两任务分别写草稿并来回切换；SMT-03 点发送（模拟延迟）后立刻改草稿并切到另一任务，再返回 | 各任务草稿隔离恢复；发送期间追加的输入保留；迟到回执不覆盖当前输入框正文 |
| E2E-ADSCOPE | ai-desktop | 已打开 SMT-03 工作区 | 选「独立话题」；再选「继续当前任务」；点「纠正关联」；再「转为任务」 | 独立话题文案可见；继续时显示固定 Task ID；纠正后有撤回记录；转为任务后有建立关联记录且可继续固定任务 |
| E2E-ADEVID | ai-desktop | 首页 SMT-03 | 点「查看证据」；打开正常证据→来源→逐层返回；打开失败/过期项；切空列表夹具 | 进入工作区且侧栏证据面板；预览含对象/时间/版本/任务 ID；来源可见；失败与过期有明确提示；空列表有「暂无证据」；关闭证据后可回首页 |
| E2E-ADHISTORY | ai-desktop | 干净浏览器 | 打开「历史」；按标题筛选；固定一项后排序变化；打开 SMT-03；关闭后再开历史筛选仍在 | 推荐理由可见；固定后该项仍在列表前列；打开任务 ID 正确；筛选值恢复 |
| E2E-ADPROF | ai-desktop | 打开 SMT-03 并写草稿 | 工作区点「调整助理」；改名称并保存；查看记忆再返回；注入冲突后再保存 | 侧栏/首页助理名同步；记忆面包屑回到助理分类；草稿仍在；冲突提示且输入框草稿未被存档覆盖 |
| E2E-ADMEM | ai-desktop | 打开 SMT-03 工作区 | 调整助理 → 查看记忆；列表可见服务端条目；打开 mem-1 看正文；返回列表再回助理 | 列表标题含「质量报告偏好摘要格式」；详情正文与来源/有效期来自 HTTP；返回路径不丢草稿 |
| E2E-ADMEMW | ai-desktop | 打开 SMT-03；夹具已 reset | 打开 mem-1 纠错正文并保存；再次打开 mem-1 撤回；回列表 | mem-1 正文已更新；撤回后列表不再含 mem-1；工作区草稿仍在 |
| E2E-ADXR | ai-desktop | 干净浏览器；夹具 reset；双上下文同服务 | 端 A 见未读≥2 并标记 asst-msg-1 已读；端 B 刷新后 asst-msg-1 已读且未读减 1；端 B 再标其余；端 A 刷新未读为 0；重复提交已读 | 服务权威跨端一致；重复提交幂等；未读徽标来自 HTTP |
| E2E-ADINPUT | ai-desktop | 干净浏览器 | 在 360/768/1024/1440 宽检查关键控件可达；设置中 Escape 关闭；会话输入 composition 期间 Enter 不发请求；`prefers-reduced-motion` 时根节点标注 | 各宽度可见「设置与管理」或抽屉入口；Esc 关掉设置；composition 中无 POST /messages；`html[data-reduced-motion]` 为 1 |
| E2E-ADASK | ai-desktop | 干净浏览器 · 桌面首页 | S1 先问答；再委派复杂任务；后台芯片出现后继续问答；手动打开任务 | 问答后仍在桌面；委派先「提交中」再「已受理」且不强制进工作区；芯片为真实 task id；打开进入发起会话（聊天）并可看见该任务 |
| E2E-HOMEACCEPT | ai-desktop | 干净浏览器 · 桌面首页 | S1「委派任务」走 B1 可靠受理（POST 后按丢回执只信 GET）；同 `client_operation_id` 再 POST；打开任务；刷新后再进该会话 | 202/回读 status=accepted；dispatch_count=1；同 op 幂等；芯片 task id 可查；重连后仍在原会话 |
| E2E-HOMEREV | ai-desktop / approval | 桌面首页；受控模型「人工验收」→ needs_human+result | S1 委派 → 打开原会话 →「判定通过」 | 任务 COMPLETED；`GET .../messages` 有 `task_id` 绑定的助理 Composer 交付；任务 `result` 含产物或证据；刷新后再进发起会话仍可断言（GS3 通过；T1/S7） |
| E2E-HOMEREJ | ai-desktop / approval | 同上；默认 `max_review_rounds=1` | S1 委派 →「判定不通过」→ 返工；再待验收 → 再「判定不通过」 | 首次 reject→QUEUED/返工且同 `conversation_id`；二次 reject→FAILED；会话 durable 写入失败交付（`task_id` 绑定）；刷新后仍可断言（GS3 拒绝出口；对齐 RT-REV-02/03） |
| E2E-ADATTN | ai-desktop | 干净浏览器 · 会话输入聚焦 | API 注入普通建议（toast 轮询出现）；静音后再 API 注入；API 注入紧急 | 输入框保持焦点；普通建议可见但不抢焦点；静音后普通建议不新增；紧急仍可见；生产界面无注入控件（T-88） |
| E2E-ADHOME | ai-desktop | 干净浏览器打开首页（fixture 已播种） | 检查首页与全局：无测试注入控件、无配置类链接行与横幅、无旧顶栏页签与右侧重复面板；侧栏含 助理/新会话/任务/历史/运营(权限)/设置 | 首页仅保留 S1 输入与三区快照卡（UI16 无重复面板）；生产界面无测试控制（MOCK_REGISTRY 规则 2） |
| E2E-ADAPPR | ai-desktop | 打开 SMT-03 工作区 | 批准 → 改参数升版 → 用旧版意图执行失败；再批准并执行两次；拒绝后不可执行；未知回执可查证 | 升版后提示原审批失效；执行次数为 1；拒绝后错误可见；未知回执经查证变为 applied |
| E2E-ADRESUME | ai-desktop | 打开 SMT-03 工作区 | 种子双任务 Run；对 A 断线重放含重复事件；对 B 补充输入继续 | A/B 状态不串；A 事件去重后进度更新；B 从 waiting_input 变为 running |
| E2E-ADVOICE | ai-desktop | 打开 SMT-03 工作区 | 拒绝麦克风后改文字；开启后注入部分转写再确认最终并关联；播报中插话；断线后改文字 | 部分转写不关联任务；确认后关联 SMT-03；插话停止播报并保留最终转写；拒麦/断线均可文字继续 |
| E2E-ADMIC-01 | ai-desktop | media=real；干净上下文；getUserMedia 拒绝 | 点「开启麦克风」 | permission=denied；可见改文字入口；无 Task 关联 |
| E2E-ADMIC-02 | ai-desktop | media=real；Playwright fake device 可授权 | 授权开麦 → ASR 注入部分 → 确认最终并关联 | partial 非空且未关联；finalize 后 final 非空且关联当前 Task |
| E2E-ADMIC-03 | ai-desktop | media=real；已有最终转写并可播报 | 开始播报后插话 | playing=false；phase=listening；final 保留 |
| E2E-ADMIC-04 | ai-desktop | media=real；听写后 offline | 改用文字继续 | final_text 与服务一致；可继续 |
| E2E-ADASR-01 | ai-desktop | media=real；asr=remote-stub | 开麦后 stub 自动 partial；确认最终并关联 | asr=remote-stub；partial 含「远端」且未关联；finalize 关联 Task；不上送音频 |
| E2E-ADASR-02 | ai-desktop | media=real；asr=remote；无 URL/密钥 | 开麦 | 回退 webspeech/harness；可见「真钥另验」提示；仍可 ASR 注入并关联 |
| E2E-ADHAND | ai-desktop | 打开 SMT-03 工作区 | 手机与桌面各写不同草稿；选手机稿解决冲突；默认未共享转写，授权后再关闭 | 同一 Task/Run/阅读锚点；冲突可选后两端一致；未授权显示不共享 |
| E2E-ADD01-01 | ai-desktop | 双 browser context；同服务；fixture 语音 | 端 A（手机）语音发起→部分不关联→确认关联 SMT-03；端 B（桌面）开 handoff 刷新 | 两端同 Task/Run/锚点；默认 voice_shared=false；端 B 对端转写为「未授权不共享」；第二物理机另验 |
| E2E-ADD01-02 | ai-desktop | 接 ADD01-01；端 A 已关联 | 端 A 授权 handoff 转写共享；端 B 刷新；端 B 续写桌面草稿 | 端 B 可见最终转写与关联 Task；可续写且不重置语音会话；第二物理机另验 |
| E2E-ADD01-03 | ai-desktop | 双 context；端 A 先 finalize 不关联 | 端 A 纠正关联到当前 Task；端 B 在共享后刷新 | linked_task_id 纠正为 SMT-03；端 B 共享后可见；不改 store 表结构 |
| E2E-ADACCEPT | ai-desktop | 打开 SMT-03 工作区 | 现有模式丢回执→须回读；可靠模式丢回执后原操作 ID 重试；同 ID 改正文 | 待确认可见；派发次数保持 1；改正文得 409 冲突且不二次派发 |

### AI Desktop B1 可靠受理（运行时 / HTTP）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADACCEPT-01 | ai-desktop | 已有会话；测试库 | `POST /api/desktop/v1/conversations/{id}/requests` 带 `client_operation_id` 与正文 | **202**；含 `request_id`/`message_id`；用户消息落库；`dispatch_count=1` |
| RT-ADACCEPT-02 | ai-desktop | 已成功受理一次 | 同 `client_operation_id` + 同正文再 POST | **202**；`request_id`/`message_id` 不变；`dispatch_count` 仍为 1；用户消息仍一条 |
| RT-ADACCEPT-03 | ai-desktop | 已成功受理一次 | 同 `client_operation_id`、改正文再 POST | **409** `operation_conflict`；原消息与 `dispatch_count` 不变 |
| RT-ADACCEPT-04 | ai-desktop | POST 已 202（模拟丢回执） | `GET /api/desktop/v1/requests/{request_id}` | **200**；可读 `status=accepted` 与关联 `message_id` |
| RT-ADACCEPT-05 | ai-desktop | 已受理完整载荷 | 单独改变会话、scope、task_id 或正文 | Conflict；原记录不变，另一会话无消息 |
| RT-ADACCEPT-06 | ai-desktop | 两会话、8 个同步并发线程 | 同操作键提交相同/不同载荷 | 仅一个 Created；同载荷 Replay、异载荷 Conflict；总用户消息一条 |
| RT-ADACCEPT-07 | ai-desktop | 同库关闭后重开 | 相同载荷及跨会话重试 | 从库重放全部字段；跨会话 Conflict；保留正文 trim 约定 |
| RT-ADACCEPT-08 | ai-desktop | 真实 HTTP 已受理 | 改会话/scope/task_id，再同载荷重试 | 异载荷 409 且不泄露正文；同载荷 202 重放；不重复写消息 |
| RT-ADREPLAY-01 | ai-desktop | 会话已有落库事件 | `GET /api/desktop/v1/events?conversation_id=&after=` 再发 live | **200** SSE：先回放缺口，再续接 live；含 `event_id`/`cursor` |
| RT-ADREPLAY-02 | ai-desktop | 任务权威状态已 COMPLETED；流含较早 status 事件 | 订阅全量回放；再投递迟到 Running 事件 | 流内 `event_id` 去重；`GET` 任务快照 status/`updated_at` 不回退 |
| RT-ADREPLAY-03 | ai-desktop | 会话存在 | `after` 指向不存在/已淘汰游标 | **410** `cursor_expired`；不静默回传全量 |
| RT-ADREPLAY-04 | ai-desktop | 写入事件后关闭 Runtime | 同库新进程订阅 `after` | 仍能补齐落库事件，与 store 会话游标一致 |
| RT-ADREPLAY-05 | ai-desktop | 两会话各有事件 | 只订会话 A | 不得出现会话 B 的 `event_id` |
| RT-ADLINK-01 | ai-desktop | 已有会话与任务 | `POST /api/desktop/v1/tasks/{id}/links` 带操作 ID | **201**；落库 link；同操作 ID 重放同一 `link_id` |
| RT-ADLINK-02 | ai-desktop | 已成功建链 | 同操作 ID、改 `conversation_id` 再 POST | **409** `operation_conflict`；原 link 不变 |
| RT-ADLINK-03 | ai-desktop | 已有显式关联 | `DELETE .../links/{link_id}` 两次 | **200**；`revoked_at` 置位且幂等；默认列表不含已撤回 |
| RT-ADLINK-04 | ai-desktop | 任务仅有归属 `conversation_id` | 列举 links；重启同库再列 | 可见 `projected` 初始关联且会话一致 |
| RT-ADPREF-01 | ai-desktop | 空库 | `GET /api/desktop/v1/preferences` | **200**；默认字段与 `version=1` |
| RT-ADPREF-02 | ai-desktop | 默认配置 | `PATCH` 带 `expected_version=1` | **200**；`version=2`；持久化 |
| RT-ADPREF-03 | ai-desktop | 已 PATCH 成功 | 再用过期 `expected_version` PATCH | **409** `version_conflict`；不覆盖已存配置 |
| RT-ADMEM-01 | ai-desktop | harness | `GET /api/desktop/v1/memories` | **200**；`items` ≥1；含 `mem-1` 及 title/source/body/expires；不含设置来源条目（#354） |
| RT-ADMEM-02 | ai-desktop | harness | `GET /api/desktop/v1/memories/mem-1` | **200**；正文与列表一致 |
| RT-ADMEM-03 | ai-desktop | harness | `GET /api/desktop/v1/memories/no-such` | **404** `not_found` |
| RT-ADMEM-04 | ai-desktop | reset 后 | `PATCH /memories/mem-1` 改 `body` | **200**；GET/列表正文已更新 |
| RT-ADMEM-05 | ai-desktop | reset 后 | `DELETE /memories/mem-1` | **200**；列表不含 mem-1；再 DELETE 幂等 |
| RT-ADMEM-06 | ai-desktop | harness | `PATCH /memories/no-such` | **404** `not_found` |
| RT-ADMEM-07 | ai-desktop | 同库路径；PATCH mem-1 后关闭 Runtime | 新进程/新 Runtime 打开同库再 `GET /memories/mem-1` | **200**；正文仍为纠错后内容（Store 权威，非进程夹具） |
| RT-ADMEM-08 | ai-desktop | 两个全新库各起一个服务；库一纠错 mem-1 | 读库二的 `mem-1` | 库二保持种子态：mem-1 为种子正文（进程状态不跨库泄漏） |
| RT-ADMEM-09 | ai-desktop | 种子（mem-1）撤回后关闭 Runtime | 同库重开服务，`GET /memories` 与 `GET /memories/mem-1` | 列表为空、mem-1 仍 **410** `revoked`（软删行使 ensure 播种不触发，撤回不得被重播种覆盖） |
| RT-ADMEM-10 | ai-desktop | reset 后（INTENT-032 / #354） | `GET /api/desktop/v1/memories` | **200**；列表不含任何「设置」来源条目；免打扰时段等设置项不作为记忆存储 |
| RT-ADMEM-11 | ai-desktop | reset 后（INTENT-032 / #354） | `GET /api/desktop/v1/memories` 逐条核对 | 来源含「用户确认」的条目 `confirmed` 为真；种子条目来源为系统生成的「用户保存 · 日期」 |
| RT-ADXR-01 | ai-desktop | harness reset | `GET /api/desktop/v1/assistant/messages` | **200**；`items` ≥2 含 `asst-msg-1`/`asst-msg-2`；均 `read=false`；`unread_count`≥2 |
| RT-ADXR-02 | ai-desktop | reset 后 | `POST .../assistant/messages/read` 带 `["asst-msg-1"]` | **200**；`newly_marked=1`；GET 列表该条 `read=true`；`unread_count` 减 1 |
| RT-ADXR-03 | ai-desktop | 已标 asst-msg-1 | 同 ID 再 POST read | **200**；`newly_marked=0`；`read_ids` 不变 |
| RT-ADXR-04 | ai-desktop | harness | `POST .../read` 空 `message_ids` | **400** `invalid_request` |
| RT-ADXR-05 | ai-desktop | 标记 asst-msg-1 已读后，同库重开服务（新进程） | `GET /assistant/messages` 与 `GET .../read` | 已读状态保留（T-96：服务端权威，重启不丢） |
| RT-ADXR-06 | ai-desktop | 库 A 标记 asst-msg-1 已读；库 B（全新库） | `GET` 库 B 列表与未读数 | 库 B 保持种子态：asst-msg-1 未读、未读数不变（进程级状态不得跨库泄漏） |
| RT-ADXR-07 | ai-desktop | alice（`x-desktop-principal` 头）标记 asst-msg-2 已读 | alice 与 bob 各自 `GET /assistant/messages`、`GET .../read`；另无头 `GET` | alice 见 `read=true`；bob 见 `read=false` 且 `read_ids` 不含该条（M4 跨用户状态泄漏修复）；无头回退 `local` 与 alice/bob 互不可见 |
| RT-ADXR-08 | ai-desktop | 旧库（`desktop_assistant_read_ids` 无 `user_id` 列，含 1 条已读） | `Store::open` 两次 | 自动迁移为 `(user_id, id)` 主键；旧行并入 `local`、`alice` 不可见；local 列表该条 `read=true`；重复打开幂等 |

### AI Desktop T-99 关联审计上云（运行时 / HTTP）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-ADLAUD-01 | ai-desktop | 任务 X | `POST /tasks/{id}/link-audits` 带 `{source:"workspace", action:"link"}`；再 POST `{action:"revoke"}`；`GET` 列表 | 两次均 **200** 且返回服务端生成的 `audit.id`；GET 按 created_at 升序返回 2 条，字段（task_id/source/action）一致 |
| RT-ADLAUD-02 | ai-desktop | 任务 A 追加 link；任务 B 追加 revoke | 分别 `GET` 两任务的列表 | 各自只含自己的审计（task 过滤互不可见） |
| RT-ADLAUD-03 | ai-desktop | harness | `POST` 带 `action:"其他值"` | **400** `invalid_request`（action 仅 link/revoke） |
| RT-ADLAUD-04 | ai-desktop | 库 A 审计后同库重开服务；库 B（全新库） | 读库 A 列表（重启持久）；读库 B 列表 | 重启后审计保留；全新库为空（用户事件不播种、跨库不泄漏——H4 同族探针） |
### AI Desktop T-82 首页快照（运行时 / HTTP）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADSNAP-01 | ai-desktop | fixture 已 reset | `GET /api/desktop/v1/snapshot` | **200**；`cards` 含 `card-smt-03`（zone=mine、entity_id=TASK-2026-018、card_type=task、simulated=true、source_refs 为数组）；agent 区含 TASK-2026-021；watch 区可为空（真实读模型）；`counts.tasks_active`/`counts.attention_unread` 为数值；`cursor` 非空 |
| RT-ADSNAP-02 | ai-desktop | fixture reset 后经 B1 受理建真任务 | 再 `GET /snapshot` | 真实任务以卡片出现在快照且 `simulated=false`；fixture 卡保持 `simulated=true`（真实/模拟可辨） |
| RT-ADSNAP-03 | ai-desktop | harness | `POST .../snapshot/fixture/reset` 连续两次，各自再 `GET /snapshot` | 每次均含 `card-smt-03`（种子可复现、reset 幂等） |
| RT-ADSNAP-04 | ai-desktop | 全新库未播种 | `GET /api/desktop/v1/snapshot` | **200**；`cards` 为数组（可为空）、`counts` 结构有效、`cursor` 非空（空库不报错、不静默回退静态卡） |

### AI Desktop T-86 草稿上云（运行时 / HTTP）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-ADDRAFTS-01 | ai-desktop | 空库 | `PUT /api/desktop/v1/drafts/task/TASK-X` 带 `{text,scope,continue_task_id,version}`；再 `GET` 同路径 | **200**；GET 返回同正文/scope/continue_task_id 与 PUT 的 version |
| RT-ADDRAFTS-02 | ai-desktop | 已存 version=2 的草稿 | 用 `expected_version=1` 再 PUT | **409** `version_conflict`；body 含当前草稿；原内容不被覆盖 |
| RT-ADDRAFTS-03 | ai-desktop | 已存 version=2、text=草稿A | `DELETE ...?sent_version=2`；再用 `sent_version=2` 重复 DELETE | 第一次：text 清空、version=3、scope 保留；第二次：幂等不变化（仍是清空后状态） |
| RT-ADDRAFTS-04 | ai-desktop | 草稿含 scope=continue 与 continue_task_id；关闭 Runtime 后同库重开 | `GET` 同路径；另对 kind=other PUT | 重启后草稿仍在（持久）；kind 非法 **400** `invalid_request` |

### AI Desktop T-95 夹具门控与事件去重（运行时 / HTTP，FIX-C/H4）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-FIXC-01 | ai-desktop | 连续两次 `POST /resume/fixture/seed` | 两次各产生独立会话；`after_cursor`/事件编号每次生成不重复；各自会话 2 条事件全部落库可回放（H4 主断言）；第二会话用自己的 after_cursor 订阅 **200** |
| RT-FIXC-02 | ai-desktop | `desktop.test_fixtures=false`（生产缺省） | `/resume/fixture/seed`、`/replay-a`、`/continue-b` 全部 **404**（夹具接口不可达） |
| RT-ADACL-01 | ai-desktop | 种子证据；alice 有授权 | `GET .../evidence/{id}?view=summary|source` + 头 | **200**；可读摘要/原文 |
| RT-ADACL-02 | ai-desktop | bob 无授权 | 同上带 bob | **403**；body 不泄露原文/摘要 |
| RT-ADACL-03 | ai-desktop | 撤权 alice | 读各视图 + 列表 + 搜索 | 全拒或列表/搜索不含该条；撤权幂等 |
| RT-ADACL-04 | ai-desktop | 已撤权 alice | carol 读原文 | **200**；互不影响 |
| RT-ADACL-05 | ai-desktop | 证据 status=expired/revoked | 有权主体读 summary/source | **410** `source_unavailable`；不回正文 |
| E2E-ADSOURCES | ai-desktop | 打开证据面板（真实 ACL） | 切换 bob/alice 撤权/carol；搜索 | 无授权与撤权后列表与搜索拒绝；carol 仍可读原文 |
| RT-ADAPPR-01 | ai-desktop | 任务审批夹具 pending | decide approve + 匹配 revision | **200**；status=approved；decided_by 非空 |
| RT-ADAPPR-02 | ai-desktop | 已批准 revision=1 | bump 升版后再用旧 revision execute | **409** `revision_conflict`；execute_count 仍为 0 |
| RT-ADAPPR-03 | ai-desktop | reset 后 reject | execute | 拒绝（4xx）；execute_count=0 |
| RT-ADAPPR-04 | ai-desktop | 已批准 | execute 两次（同 revision） | 两次均成功语义；execute_count=1；last_receipt=applied |
| RT-ADAPPR-05 | ai-desktop | 已批准 | execute force_unknown → ack-unknown | receipt 先 unknown 再 applied；execute_count=1 |
| RT-ADATTN-01 | ai-desktop | mute_ordinary=true | 投递 ordinary 与 urgent | ordinary 不入队；urgent 入队 |
| RT-ADATTN-02 | ai-desktop | 已有同 title ordinary | 再投递同 title ordinary | 队列中该 title ordinary 仍仅 1 条 |
| RT-ADVOICE-01 | ai-desktop | reset 后 permission=denied | as-text | **200**；final_text 为文字继续；可无麦克风 |
| RT-ADVOICE-02 | ai-desktop | start 后 partial | finalize link_task | partial 时 linked_task_id 为空；finalize 后关联任务且 partial 清空 |
| RT-ADVOICE-03 | ai-desktop | finalize 后 play | barge | playing=false；phase=listening；final_text 保留 |
| RT-ADVOICE-04 | ai-desktop | finalize 后 offline | as-text 同文 | phase=offline；final_text 保留 |
| RT-ADHAND-01 | ai-desktop | handoff reset | GET/响应 | 同 task/run/read_message 锚点；voice_shared=false |
| RT-ADHAND-02 | ai-desktop | 双端不同草稿 | resolve phone | conflict 先 true 后 false；两端正文=手机稿 |
| RT-ADHAND-03 | ai-desktop | reset | share-voice true→false | voice_shared 随请求切换 |

## 运营桌面与运营频道 D0/D1（用例草案，实现前定义）

设计见 `docs/mom/OPS_DESK.md`，决策见 DECISIONS 第 56–58 条。以下为**草案**：待用户确认断言后，才写成可执行测试并登记到 `features.json`（因此暂不写成表格行，避免被 `check-map` 当作缺测试的已登记用例）。数据全部来自 DES 事件（先进程内），仅替换外部模型。

浏览器用例（`web/e2e/ops.spec.ts`，**断言已于 2026-11-18 经用户确认并冻结**；P2 一项按下方说明改写）：开关打开的实例每个用例单独启动，配置 `[mom] chat_bridge=true`、`ops_merge_secs=1`、`ops_hourly_cap=10`；E2E-OPSOFF 用默认 harness。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-OPSDESK | ops-desk | 开关打开；一个待批准案例、一个受阻案例（安全联锁）、一个已完成案例 | 打开“运营桌面”页签 | 厂况条“待批准 1”“受阻 1”；待批准置顶、受阻其次；已完成默认折叠且保留总数；每行有“模拟”标识且只有一个按钮；桌面没有“批准”入口；选“待我批准”筛选后刷新仍保持 |
| E2E-OPSDRAWER | ops-desk | 开关打开；一个待批准案例 | 点击事项行 | 抽屉依次有“发生了什么与影响、责任、方案与前提、缺什么、进展与下次反馈、凭什么算解决”；原始数据默认折叠、展开后可见；抽屉无批准按钮；Esc 关闭后焦点回到触发的事项行 |
| E2E-OPSCHANNEL | ops-desk | 开关打开；注入一个待批准案例 | 查看页签未读数，打开运营频道，再批准该案例 | 未读数为 1，打开后清零；频道里该案例只有一张带“模拟”的卡；批准后同一张卡更新（含批准人）、频道消息条数不变；频道没有模型回复，也没有任务 |
| E2E-OPSNOISE | ops-desk | 开关打开，每小时上限 10；一个已有卡片的案例，再来 12 个新案例 | 对已有卡片的案例重复上报 10 次观测；再注入 12 个新案例 | 重复观测不新增频道消息；卡片总数为 10，另有一张“另有 3 条”汇总卡；桌面仍有全部 13 个事项 |
| E2E-OPSAPPROVE | ops-desk | 开启桥的实例，有待批准案例 | 桌面点「在频道中处理」→ 卡上点「批准」 | 卡显示 supervisor；桌面无「批准」按钮；引擎 decision_inbox 含批准人 |
| E2E-OPSAUTOEXEC | ops-desk | 开启桥的实例，有待批准案例（INTENT-027，决定 2026-11-26-ops-approve-auto-execute） | 桌面点「在频道中处理」→ 卡上点「批准」；全程不调用执行 API | 卡变「核验中」并显示执行回执；引擎 stage=VERIFYING 且 actions 非空；桌面「待批准 0」，该事项行显示「核验中」 |
| E2E-OPSEXPIRE | ops-desk | 开启桥的实例，待批准案例，已注入 `resources_available=false`（INTENT-027） | 卡上点「批准」；用 `advance` 推进引擎时间越过批准有效期；恢复资源并 `refresh_plan` 后在卡上再点「批准」 | 受阻时卡显示「已批准」「执行受阻」且引擎无 actions、批准保留；过期后卡显示「批准已过期」「需重新批准」并有重新批准按钮；刷新方案重评后重新批准即执行成功（VERIFYING），新批准有效期刷新（expires_at 变大） |

| E2E-OPSRECEIPT | ops-desk | 开启桥的实例；两个 s1 场景案例 | UXFIX-P（INTENT-2026-026，UX-07-I5）：案例 A 批准不执行 → 卡上「查看执行」；案例 B 等卡发布后批准、经 API 执行并推进关闭 → 「查看执行」与「查看核验证据」 | A 显示「尚未执行」且不出现「已提交」；B 回执分层来自引擎（已提交 → 源系统已受理，含回执编号 `mock-op:*` 与时间，标「模拟」），不出现写死文本「受理 / 已提交」；核验证据来自引擎证据表（`mock-field-verification:*` 等），不出现写死「现场照片与点检记录」 |
| E2E-OPSOFF | ops-desk | 默认 harness，未配置 `[mom]`（基线：现状本来就通过，用于防止回归） | 注入 MOM 案例，打开页面，发一句问候 | 没有“运营桌面”页签、没有运营频道、`/api/mom/desk` 为 404；原有会话回复正常 |
| E2E-ADREAD | ai-desktop | 开关打开；频道内 ≥6 张运营卡；窄视口 | 滚到中间卡使其位于列表顶部；切到桌面再回运营频道；工作区收起执行后离开再打开 | 锚点=滚动停止时首条可见消息及其偏移（几何钳制时为钳制位置的可见消息）；返回后该锚点消息仍为首可见、偏移恢复（阅读位置精确还原）；工作区执行区保持收起 |

说明：原草案里 E2E-OPSNOISE 的“大量 P2 读数不进频道”改写为“同一案例重复观测只更新原卡”。原因：进程内 MOM 目前没有纯过程读数（每次观测都会让案例的计划失效），纯 P2 要等 DES 事件接入后才有；P2 的分级由 RT-OPSDESK-01 覆盖。这是实现前的改写，不是为迎合实现改断言。

Rust 运行时用例（`crates/runtime/tests/ops_desk.rs`，函数前缀 `rt_opsdesk_NN_`）。D0a（后端）的 01–05、08 已写成可执行测试；D0b 的 09 已绿；D1-1 的 06、07 已写成可执行测试并转绿，见 `docs/testing/evidence/ops-desk-d1-1-red.md`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-OPSDESK-01 | ops-desk | 一组事件种类与案例阶段的组合 | 调用分级函数 | 安全联锁、超期和“等待人决定”为 P0；纯观测和过程事件为 P2（不因案例阶段变化）；其余状态变化为 P1；同输入恒得同输出，不调用模型 |
| RT-OPSDESK-02 | ops-desk | 文件库里的引擎与发布器，已发出一张卡 | 重启后再 tick；随后批准案例再 tick | 重启后没有新事件时不重复发；重启期间的新事件不漏，更新原卡（带批准人），频道仍只有一条消息 |
| RT-OPSDESK-03 | ops-desk | 一个待批准案例 | 发布器 tick | 运营频道会话存在且 id 固定；消息正文含案例号和“模拟”；卡片是来源为 simulation 的 P0 案例卡；不关联任务，频道下没有任务 |
| RT-OPSDESK-04 | ops-desk | 不同的 `[mom]` 配置文本 | 解析配置 | 未配置时开关关闭；打开开关时合并窗口默认 60 秒、每小时上限默认 10；显式配置值被采用；无未知字段 |
| RT-OPSDESK-05 | ops-desk | 合并窗口 60 秒；另一组每小时上限 2 张、5 个待批准案例 | 窗口内更新案例；一次 tick 处理 5 个案例；另测配额用尽后更新已汇总案例并加入新案例 | 窗口内不更新，并记录原因 `merge_window`，窗口后才更新原卡；上限内发 2 张卡，其余 3 个并入唯一一张汇总卡，记录原因 `hourly_cap`，没有案例被静默丢弃；同一案例在汇总卡中只计一次，新案例依次追加 |
| RT-OPSDESK-08 | ops-desk | 主库 | 用固定 id 创建会话两次，中间写入消息 | id 为固定值；第二次不改标题和创建时间，不清空消息，会话只有一条 |
| RT-OPSDESK-06 | ops-desk | 一个待批准案例，发布器未 tick | 对同一案例调用 `open_case` 两次，再 tick；对不存在的案例调用 | 两次返回同一非空消息 id；频道里该案例只有一张卡；tick 后消息条数不变；不存在的案例错误以 `E_CASE_NOT_FOUND` 开头 |
| RT-OPSDESK-07 | ops-desk | 零案例引擎；另造待批准、受阻、已关闭各一 | 调用 `desk_view` | 零案例 `state=no_data`；有案例时 `state=ok`，`rows[0]` 待批准、`rows[1]` 受阻、已完成最后；`strip` 计数一致；每行 `provenance=simulation` 且 case_id 都在引擎里；结果确定 |
| RT-OPSDESK-09 | ops-desk | 发布器（文件库），先后发出几张卡 | 查看未读、批准已发卡的案例、再发新卡、标记已读、重启 | 无消息时未读为 0；每张新卡、汇总卡各加 1；就地更新原卡不增加；标记已读后为 0；已读位置重启后仍在 |
| RT-OPSDESK-11 | ops-desk | 服务端挂桌面路由，已发卡的待批准案例 | approve 两次；错误 plan_revision；不存在案例 | 首次 already=false；再次 already=true；卡 approver=supervisor；错误版本 409；不存在 404 |
| RT-OPSDESK-10 | ops-desk | 服务端同时挂 MOM 与运营桌面路由，已创建 s1 案例 | `GET /api/mom/desk`；`GET …/cases/{id}`；对同一案例 `POST …/open-in-channel` 两次；对不存在案例 GET/POST | desk 200 且 rows 案例集合与 `/mom/api/mom/cases` 相同；详情含 `projection_seq`；两次 open 返回同一 `message_id`；不存在案例 404 |
| RT-OPSDESK-12 | ops-desk | 文件库引擎，一个待批准案例（s1）；另一组先注入 `resources_available=false` | 调用 `approve_and_execute`：正常案例；受阻案例；受阻案例到期前再次调用；错误方案版本 | 正常：返回已执行，stage=VERIFYING、actions 非空、无 execution_block；受阻：批准保留、无 actions、execution_block 记录原因、事件 `ExecutionBlocked`；到期前重试：仍受阻、批准不重复登记；错误版本：整体失败，无批准无事件 |
| RT-OPSDESK-13 | ops-desk | 文件库引擎，受阻批准（`approve_and_execute` 已 Blocked） | `advance` 越过批准有效期后再 `advance`；随后重新 `approve_and_execute`；发布器 tick 后看卡片 | 首次越过有效期发一次 `ApprovalExpired`，再推进不重复发；重新批准后有效期刷新、过期标记清零；卡片摘要含「批准已过期，需重新批准」（过期后）与「已批准」（重新批准后） |
| RT-OPSDESK-14 | ops-desk | 同一引擎两个待批准案例；发布器未 tick 前其中一个已被批准并执行（stage 已到 VERIFYING）（#384） | 发布器一次 tick；查 skip_log | 两个案例都出卡；被执行案例没有 `p1_no_card` 跳过记录；`DecisionRequested` 事件按种类固定 P0，与消费时刻的阶段无关 |

说明：RT-OPSDESK-06/07 属 D1-1；RT-OPSDESK-10/11 属 D1-2/D2（`crates/server/tests/mom_ops.rs`）。证据见 `ops-desk-d1-*.md`、`ops-desk-d2.md`。 RT-OPSDESK-12/13 属 INTENT-027（批准即执行，决定 2026-11-26-ops-approve-auto-execute），见 `docs/testing/evidence/ops-approve-execute.md`。

说明：原草案 RT-OPSDESK-03 的“不进入记忆抽取”本期无法验证——仓库里还没有个人记忆抽取模块（见 `docs/memory/` 的缺口 G-9）。这里改为断言消息带结构化案例卡，日后的抽取器按卡片类型跳过即可；等记忆模块出现时再补断言。原 RT-OPSDESK-04 的“无桌面接口和发布器”属于服务层，放到 D0b 的服务端测试。

D2（卡片批准，决定 60）的 E2E-OPSAPPROVE、RT-OPSDESK-11 已冻结并转绿；证据见 `docs/testing/evidence/ops-desk-d2.md`。

## R-20261118-M5 JSON 路径不碰撞

| Case | Given | When | Then |
|---|---|---|---|
| PY-DLP-M5 | 字面点号/方括号/空键与嵌套路径共存，含客户名及普通值；另注入重复分析路径 | evaluate | 每个字段分别正确脱敏且结构/原输入保持；重复路径在写Vault前BLOCK；普通field_path仍为字符串 |


## 业务内省 INT（S2：采纳模式统计与校验）

设计见 `docs/memory/INTROSPECTION_PLAN.md`，可执行说明见 `docs/memory/INTROSPECTION_PLAYBOOK.md`。状态约定：动作成功为 `ACCEPTED`/`VERIFIED`，失败为 `FAILED`；成功并验证需 `verification` 非空。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-03 | introspection | 某模板下 13 个带审批 Case：9 个成功并验证、2 个动作失败、2 个审批过期未执行 | 运行采纳模式内省 | 统计与回算一致（审批 13、成功并验证 9、失败 2、过期未用 2）；耗时为 complete_at−accepted_at；样本不足的分组只写「样本不足」；结论不含源数据没有的字段 |
| RT-INT-06 | introspection | 任意内省分析路径 | 监视引擎状态 | 对 MOM 无写调用（序号、时间、阶段、动作数不变） |
| RT-INT-07 | introspection | 样本低于门槛或证据不足 | 校验 | 验真为 inconclusive；不进入可推送集合 |
| RT-INT-08 | introspection | 一条 Finding 的证据引用指向不存在的记录，或数字被篡改 | 校验 | 该条 Drop 并记原因；完好条目仍可 Accept |


## 业务内省 INT（S3：发送闸门）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-16 | introspection | 定时内省结束；没有任何结论通过发送闸门 | 查看闸门结果 | 全部不推送；每条都有未推送原因 |
| RT-INT-17 | introspection | 一条 Supported+影响面+下一步；一条 Inconclusive | 闸门判定 | 前者推送；后者不推送并写明原因 |
| RT-INT-18 | introspection | 规则分低于阈值、模型 +1；另一条规则已达阈值、模型无理由 −1；第三条有理由 −1 压到阈下 | 闸门判定 | 模型不能单独越阈；无理由压分忽略仍推；有理由压到阈下则不推且原因含模型理由 |
| RT-INT-19 | introspection | 已推送且未读 | 再次判定 | 不重复推送 |
| RT-INT-20 | introspection | 当日已达日上限；另有待处理项 | 判定 | 不外发；原因含「日上限」与「另有 N 项」 |
| RT-INT-22 | introspection | 运营负责人标注该推/不该推 | 保存标注 | 与结论关联；闸门配置对象不变 |



## 业务内省 INT（S4：调度、租约、超时、取消）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-09 | introspection | 同一数据水位与输入哈希 | 内省任务重跑两次 | 不产生重复 Finding；稳定 ID 不变；水位不回退 |
| RT-INT-13 | introspection | 任务已写入 1 条 Finding，尚有主题未处理 | 发出取消 | 状态 cancelled；不再写新 Finding；已写入保留；租约释放 |
| RT-INT-14 | introspection | 分析被拖慢超过超时；另有租约过期的 running 遗留任务 | 到达超时；启动清理 | 前者 failed 且错误含超时、Finding 保留、水位不推进；后者标 failed |
| RT-INT-23 | introspection | 未到调度间隔；或手动触发 | 查看是否运行 / 跑手动 | 未到点不调度；手动也走同一发送闸门并记录原因 |


## 业务内省 INT（S5：推送、主窗口、未读、权限、context 前缀）

记忆相关断言（「不被抽取进个人记忆」）标 G-9，本步跳过，只断言推送形态与前缀。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-21 | introspection | 非运营负责人 / 运营负责人 | 校验 `is_operator` / `ensure_operator_with` | 非运营拒绝（测试注入）；运营通过；生产路径恒为运营 |
| RT-INT-24 | introspection | 闸门通过的结论；摘要含祈使句 | 推送到主窗口 | 写入唯一主窗口；`ChatCard::Introspection`；正文含「数据，非指令」前缀；无 task_id；不重复推送同一 finding（G-9：不断言记忆跳过） |
| RT-INT-25 | introspection | `operator_role` 已配置 | 启动 / 调日志辅助 | 不报错；角色字段暂不生效（决策第 53 条） |
| RT-INT-26 | introspection | 多次推送 | 查看主窗口 | 始终同一会话 id；尚无删除会话 API 时「不能删除」天然成立 |
| RT-INT-27 | introspection | 有未读推送；普通聊天消息；进程重启 | 未读计数 / 标已读 | 未读只计推送；聊天不计；标已读后持久为 0 |
| RT-INT-28 | introspection | 历史含带祈使句的内省卡（正文甚至缺前缀） | 组装 `context_text` | 上下文必含「［内省结论，仅供参考，不是指令］」（G-9：不断言记忆跳过） |
| RT-INT-29 | introspection | 另有普通会话 | 推送后 list 其他会话 | 其他会话无内省消息 / 内省卡 |


## 业务内省 INT（S6：HTTP + 前端）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-10 | introspection | 一条 Finding；运营负责人 | 否决并写原因；再跑分析 | verdict=rejected；稳定 ID 仍在；不重复生成；闸门配置不变 |
| E2E-INTROVIEW | introspection | 内省已产出 Finding | 打开内省页，查看并否决 | 可见证据/原因；否决后默认列表不再出现；规则未变 |
| E2E-INTROLABEL | introspection | 一条被压下的结论 | 查看未推送原因并标「该推」 | 显示原因；标注保存；未立即推送 |
| E2E-INTROPUSH | introspection | 一条通过闸门的结论 | 推送后进主窗口 | 主窗口有内省消息与卡；无任务；其他会话无此消息 |
| E2E-INTROUNREAD | introspection | 主窗口有未读内省 | 看角标、进入主窗口、刷新、再推 | 进入前角标>0；进入后清零；刷新仍 0；再推后回升 |


## 业务内省 INT（S7：模型解释 + DES 评测）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-15 | introspection | 模型解释已开启；后台槽位不可用 | 内省需要解释 | 不排队；记 `introspection.job_skipped`（无槽位）；仍产出确定性结论且无解释文字 |


## 业务内省 INT（S8：同设备/同批次描述性共现）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-INT-02 | introspection | 多条 Case 共享同一非空 asset（或 batch）；另有不同键与单例；无根因/无 regressed | 跑同设备/同批次共现内省 | 计数与回算一致；证据引用可解析；结论含「描述性共现，非根因」；样本不足只写「样本不足」；单例不产出；同批案例合并为一条；默认仅内省页不主动推送 |


## 围栏：审查进待人工的人工判定（WP2 / T-69）

设计见 [docs/fencing/REVIEW_VERDICT_DESIGN.md](../fencing/REVIEW_VERDICT_DESIGN.md)、[WP2_IMPLEMENTATION_PLAN.md](../fencing/WP2_IMPLEMENTATION_PLAN.md)。用例冻结；实现只改业务代码。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-REV-01 | approval | 审查进入 NEEDS_HUMAN 且有 result | `POST .../review-verdict` accept | COMPLETED；事件含人工 note |
| RT-REV-02 | approval | 同上，仍有返工轮次 | reject | 回到执行或 VERIFYING，带问题清单 |
| RT-REV-03 | approval | 返工用尽 | reject | FAILED |
| RT-REV-04 | approval | 已是 COMPLETED | accept 或 reject | 409，状态不变 |
| E2E-REV | approval | 浏览器任务卡为有 result 的待人工 | 点「判定通过」 | 状态与交付文案为已完成 |

### 重启语义（WP7 / G08 / GS6 重启分支；用例冻结；实现只改业务代码）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-RESTART-01 | restart | 库中有 RUNNING/VERIFYING 任务 | 关闭 Runtime 后同库重启 | 两类任务均 FAILED；每任务新增含「重启中断」原因的 StatusChanged 事件 |
| RT-RESTART-02 | restart | 库中有中断任务 | 同库重启 | 各会话收到绑定 `task_id` 的助理失败交付，正文含「任务因服务重启中断，可重新委派」 |
| RT-RESTART-03 | approval / restart | 待验收（NEEDS_HUMAN 有 result）跨重启 | 重启后 `POST .../review-verdict` accept | **200**；COMPLETED；事件含人工 note；交付照常 |
| RT-RESTART-04 | approval / restart | 待验收跨重启；`max_review_rounds=1` | 重启后 reject → 返工 → 再待验收 → 再 reject | 首次 reject 回执行侧且同 `conversation_id`；二次 reject FAILED（返工额度按库扣减） |
| RT-RESTART-05 | approval / restart | 待批准（NEEDS_HUMAN 无 result）跨重启 | 重启后 `POST .../approve` | **200**；按路由继续执行至终态 |
| RT-RESTART-06 | restart | 库中有 COMPLETED/FAILED/CANCELLED 任务 | 同库重启 | 状态与 `updated_at` 不变；无新增重启事件 |
| RT-RESTART-07 | restart | 单任务（非模板家族）QUEUED 跨重启；开关默认开 | 同库重启 | 任务重新排队并执行至终态；事件含「重新排队」（WP7 方案 C） |
| RT-RESTART-08 | restart | 单任务 CREATED 跨重启 | 同库重启 | 同 RT-RESTART-07 |
| RT-RESTART-09 | restart | RUNNING/VERIFYING 与 CREATED/QUEUED 并存 | 同库重启（默认开） | 前者 FAILED + 原因；后者不被启动恢复标记失败（由重排处理） |
| RT-RESTART-10 | restart | `resume_queued_on_restart=false` | QUEUED 任务同库重启 | 仍 FAILED 且含「重启中断」原因；无「重新排队」事件（回滚开关） |
| RT-RESTART-11 | restart | 模板家族 root/branch/join 处于 QUEUED | 同库重启（默认开） | 均不被重驱：FAILED + 「重启中断」原因；无「重新排队」事件 |
| E2E-RESTART | ai-desktop | 干净浏览器 · 桌面首页；harness 同库重启 | S1 委派任务 A；A 在执行中重启服务；重连回原会话；再委派 B 走到待验收，重启后「判定通过」 | A FAILED 且会话见含重启原因的失败交付；B 重启后判定通过仍 COMPLETED；重连后仍在原会话（GS6 重启分支） |


## World Adapter / DES 实验与运营（M0；DES-UI 已冻结）

设计见 [WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md)、[DES_EXPERIMENT_OPS_SPLIT.md](../mom/DES_EXPERIMENT_OPS_SPLIT.md)、[WORLD_ADAPTER_PLAN.md](../mom/WORLD_ADAPTER_PLAN.md) §0；执行卡 [WA_M0_TASKS.md](../mom/WA_M0_TASKS.md)。  
浏览器用例（`web/e2e/des-experiment.spec.ts`）：**断言已冻结（M0-0）**；M0-3 起六条在 Mock 独立实例上 Green。命名：`DES-UI-01～06` → `E2E-DES*`（`check-map` 允许的 `E2E-[A-Z]+`）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-DESTRIAL | world-adapter | 实验员可进「DES 仿真实验」；有一份可保存配置 | 保存配置（含一次非法参数）并点「试跑」 | 非法参数有明确提示；预览有试跑 ID；运营频道新增卡为零 |
| E2E-DESSTART | world-adapter | 已有合法配置版本与目标运营频道 | 核对后「启动正式运行」并重试同键请求；运营员打开频道 | 仍只有一个正式运行；试跑与正式 ID 不同；同一事项只发一张正确标注的卡 |
| E2E-DESOPS | world-adapter | 正式运行已向运营频道投递异常卡 | 运营员打开卡 →「查看方案」→「批准」→「查看执行」 | 全程在 AI 桌面/运营频道完成；审批匹配方案、事项与世界；执行证据更新原卡；桌面无「批准」 |
| E2E-DESPAUSE | world-adapter | 正式运行中；已有下发等待仿真推进的动作 | 实验员「暂停」→ 运营员见等待 →「单步推进」→ 运营员重连 | 等待状态区别于失败；推进后回执正确；重复及迟到事件不复制或回退卡片 |
| E2E-DESISO | world-adapter | 运行 A 与运行 B 并存；运营员身份 | 运营员尝试进入实验控制；用运行 A 的审批提交到 B | 服务端拒绝；列表/详情无跨运行泄露；启动或业务动作计数不变 |
| E2E-DESLOOP | world-adapter | Mock 双角色就绪（实验员 + 运营员） | 实验员启动正式运行 → 运营员批准维修 → 实验员推进 → 运营员看核验证据 | Mock 下业务路径闭环；两端关联同一执行；标准答案仅审核可见（物理断言随 M2） |

浏览器用例（`web/e2e/mom-workbench.spec.ts`）：M0-4 冻结 `E2E-WA-01` → `E2E-WABIND`。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-WABIND | world-adapter | 桌面可进「MOM 工作台」；有模拟世界与 DES 世界 | 「选择世界」→「看板」「任务」「资源」分别打开同一事项；返回后再进；再切另一世界 | 同一 world/case；返回保持世界与入口筛选；跨世界列表不泄露上一世界事项 |
| E2E-WACHANNEL | world-adapter | Mock 实例上工作台有事项 | 打开事项 →「前往运营频道」 | 离开工作台进入运营频道会话；频道入口可见 |

M0-5/M0-6 集成套件：`./scripts/wa-m0-suite.sh`（RT-WA-01～09 + `des-experiment` + `mom-workbench` + `wa-integration`）。`E2E-DESLOOP` 使用双浏览器上下文（实验员 / 运营员）。H1～H4 准出与覆盖矩阵见 `docs/testing/evidence/wa-m0-6.md`、`docs/mom/WA_MOCK_REGISTRY.md`。Feature Map 中未登记的 `E2E-WA-09/10`（客诉移交 / 生产确认）属 M3/M4，Mock 路径保留。

### M0-1 服务端 Mock HTTP 合同（`crates/server/tests/mom_world_mock.rs`）

协议见 [WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §4；实验控制见 [DES_EXPERIMENT_OPS_SPLIT.md](../mom/DES_EXPERIMENT_OPS_SPLIT.md)。路由前缀 `/api/world/mock`。身份头 `X-WA-Role`：`experimenter` / `operator` / `auditor`。故障注入仅 `allow_test_controls` 开启时可用。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-WA-01 | world-adapter | 实验员；合法配置 | 保存配置（非法时长应失败）并试跑 | 非法参数 `INVALID_ARGUMENT`；试跑返回 `trial_run_id`；该运行 `delivery_count=0`、`mode=trial` |
| RT-WA-02 | world-adapter | 已有配置版本与 `start_key` | 两次「启动正式运行」同键同载荷；另有一次试跑 | 两次返回同一 `official_run_id`；试跑 ID ≠ 正式 ID；正式运行 `delivery_count=1` 且有 `case_id` |
| RT-WA-03 | world-adapter | 正式运行世界 | Observe → Subscribe；测试注入游标过期后再 Subscribe | 首读有 `cursor`/`events`；过期后 `CURSOR_EXPIRED`（HTTP 409） |
| RT-WA-04 | world-adapter | 正式运行；运营员 | Propose → Execute（含 approval_ref 与幂等键）→ 同键再 Execute → Confirm | 返回同一 `execution_id`；Confirm `status` 为 `accepted` 或更高；`protocol_version=1` |
| RT-WA-05 | world-adapter | 正式运行 A 与 B；运营员 | 运营员调实验控制；用 A 的提案对 B 的 world 执行 | 实验控制 `ACCESS_DENIED`；跨运行 Execute `ACCESS_DENIED`；A/B 的 `action_count` 不变 |
| RT-WA-06 | world-adapter | 正式运行；测试控件开 | Execute 后注入 `lose_receipt` 再 Confirm；另注入 `partial_fail` | 先 `unknown`；再 `failed` 且含部分成功子动作证据 |
| RT-WA-07 | world-adapter | 正式运行已暂停 | 实验员 pause → step 1 分钟 | `run_status=paused` 时事项投影为等待而非失败；step 后 `simulation_minute` +1 且执行可 Confirm 前进 |
| RT-WA-08 | world-adapter | 审核员与运营员 | 审核员读 `ground_truth`；运营员读同一接口 | 审核员可见；运营员 `ACCESS_DENIED` 或字段为空 |
| RT-WA-09 | world-adapter | 默认 `AppConfig`；未 merge Mock 路由的服务与已挂载 Mock | 读默认 `mom.world_mock`；对未挂载与已挂载分别 POST configs | 默认 `world_mock=false`；未挂载 → 404；挂载 → `protocol_version=1` 且有 `config_version` |

### M1 远程 DES 读取（断言已冻结；M1-1 Red）

执行卡：[WA_M1_TASKS.md](../mom/WA_M1_TASKS.md)。浏览器用例：`web/e2e/wa-des-remote.spec.ts`。合同：`crates/server/tests/mom_world_des.rs`。路由前缀 `/api/world/des`（相对 M0 `/api/world/mock`）；写路径仍 Mock。证据：`wa-m1-0`～`wa-m1-5.md`（准出见 `wa-m1-5.md`）；T-50 真连接器：`wa-des-connector.md`。套件：`./scripts/wa-m1-suite.sh`。M0 套件不得弱化。T-50：`[mom] des_mcp` / `with_des`；合同另含 live bind/tick 与半提交拒用（同文件）。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-WADES | world-adapter | 主应用可挂远程 DES；工作台可选 DES 世界 | 绑定远程世界 → 看板/任务/资源打开同一事项 → 再切另一世界 | 同一 world/case；跨世界不泄露；`data-provenance` 或来源标签为 DES |
| E2E-WAREAD | world-adapter | 远程正式运行已有投递游标 | 断线或游标过期后点「重新读取」/重连 | 同事件 ID 只应用一次；过期与水位单独显示（非正常空态） |
| E2E-WAOBS | world-adapter | 远程正式运行已推进 | 打开观测或事项进展 | 可见正式投递的设备/质量观测；界面不含审核标准答案字段 |

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-WADES-01 | world-adapter | 可启动 `DesConnector` 的测试进程；另测未连接 | 创建远程运行并绑定世界；未连接时再绑 | 绑定返回 `world_id` 且 `provenance=des`、`protocol_version=1`；未连接明确错误（非 200） |
| RT-WADES-02 | world-adapter | 已绑定远程世界 | tick 完成 MOM 投递前后分别 Observe | 投递前无新业务事件；投递后可见；中断后仍服务最近完整边界 |
| RT-WADES-03 | world-adapter | 已 Observe 取得游标 | Subscribe 续读；再注入过期游标 | 无漏、无重复事件 ID；过期 → `CURSOR_EXPIRED`（HTTP 409） |
| RT-WADES-04 | world-adapter | 远程实验控制可用 | 试跑一次；正式启动一次 | 试跑 `delivery_count=0`；正式 `delivery_count≥1` 且有 `case_id`；试跑 ID ≠ 正式 ID |
| RT-WADES-05 | world-adapter | 已有完整投递边界；测试控件开 | abort-before-publish → step → Observe；再 commit-publish → Observe | 中断期间 `publish_seq`/事件不变；提交后前进且快照对齐 |
| RT-WADES-06 | world-adapter | 已 step 发布一次 | Observe；空 pending 再 commit | `as_of`/`objects.publish_seq` 与事件水位一致；无重复 `event_id`；空 commit 不抬水位 |
| RT-WADES-07 | world-adapter | 已挂运营投影的远程 DES 适配 | 实验员试跑一次 | `delivery_count=0`；运营频道消息数为 0（不投递事项卡） |
| RT-WADES-08 | world-adapter | 同上；已有可用 `config_version` | 正式启动一次 | `delivery_count≥1` 且有 `case_id`；运营频道出现 `OpsCase`，`provenance=des` |

### M2 远程 DES 动作（断言已冻结；M2-1 Red）

执行卡：[WA_M2_TASKS.md](../mom/WA_M2_TASKS.md)；Green 方案：[WA_M2_1_PLAN.md](../mom/WA_M2_1_PLAN.md)。设计验收：WA-03、04、05、08（[WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §9）；里程碑：[WORLD_ADAPTER_PLAN.md](../mom/WORLD_ADAPTER_PLAN.md) M2。协议：Propose / Execute / Confirm（§4.3～4.5）与远程写恢复（§6.2～6.3）。浏览器用例：`web/e2e/wa-des-write.spec.ts`。合同：`crates/server/tests/mom_world_des_write.rs`（**Green 卡入库**；本卡仅本地 Red 证据，见 `docs/testing/evidence/wa-m2-1-red.md`）。路由前缀 `/api/world/des`。M0 Mock 写（`RT-WA-04/05/06`）与 M1 读（`RT-WADES-01～08`）不得弱化。生产写属 M4。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-WAEXEC | world-adapter | 远程正式运行；ops 双角色；已有可批方案 | 方案/版本变化后仍用旧批准 Execute；同审批并发提交；取消后再冲刷待发；跨世界 Execute | 过期/变化拒绝执行；同审批只一次业务执行；取消后待发不发、迟到回执只记事实且案例终态不变；跨世界/跨运行拒绝 |
| E2E-WARECV | world-adapter | 同存活远程工厂；已 Execute | 模拟丢响应后按原幂等键/关联键 Confirm；部分回执补齐；工厂进程重启后再提交旧运行 | 找回原工单；部分回执可补齐；旧运行失败可见且不误投新实例 |
| E2E-WADYN | world-adapter | 远程动态 DES；已发生若干事件 | 批准维修等干预 → 再推进 → 观测 | 已发生事件前缀保持；未来事件改变；界面不含审核标准答案；与 RT-DES-08～11 / RT-DESC 相关覆盖对齐 |
| E2E-WALOOP | world-adapter | 远程 DES fixture；实验员 + 运营员 | 实验员启动正式运行 → 运营员方案/批准/查看执行 → 实验员推进 → 运营员核验证据 | 两端关联同一 `execution_id`；运营频道/工作台同一执行证据；请求走 `/api/world/des` 而非 Mock |

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-WADES-09 | world-adapter | `with_des`；远程 case 与 `plan_revision` | Propose；再在修订/前置变化后用旧批准 Execute | Propose 绑定 case/`plan_revision`；过期或变化 → `APPROVAL_INVALID` / `PRECONDITION_FAILED` |
| RT-WADES-10 | world-adapter | 已有 proposal；同 `(world_id, idempotency_key)` | 并发 Execute；另测键同载荷异 | 只登记一次业务执行；键同载荷异 → `IDEMPOTENCY_CONFLICT`，可找回原 `execution_id` |
| RT-WADES-11 | world-adapter | 案例取消且仍有未发缓冲；另有已发子动作 | 取消后冲刷；迟到回执到达 | 未发不发送；迟到回执只更新证据，案例终态保留（非重新打开执行） |
| RT-WADES-12 | world-adapter | 已 Execute；测试控件丢回执 / 部分失败 | Confirm | 先 `unknown` 或按原关联键回原工单；部分失败 → `failed` 且含部分成功子动作证据 |
| RT-WADES-13 | world-adapter | 旧运行绑定的工厂实例已变更 | 对旧运行再 Propose/Execute | 失败可见（非静默成功）；新实例不接收旧运行误投 |
| RT-WADES-14 | world-adapter | 远程动态场景；干预前后可 Observe | 干预（如批准维修）后 Observe；审核员与运营员分别读 | 已发生前缀事件集合不变、未来事件集合改变；审核答案与干预一致；运营员/普通会话 Confirm/Observe 无标准答案字段 |
| RT-WADES-15 | world-adapter | 已 Execute 且有 `correlation_key`；测试控件丢回执 | 仅用 `correlation_key`（无 execution_id）Confirm；恢复回执后再 Confirm | 找回同一 `execution_id`；先 `unknown` 再 `applied`/`confirmed` |
| RT-WADES-16 | world-adapter | 已 Execute；部分失败 | `complete-partial` 补齐剩余子动作回执后再 Confirm | 补齐后状态可推进；子动作证据无残留 `failed`/`unknown` |
| RT-WADES-17 | world-adapter | 已 Confirm 的执行；工厂实例已变更 | Confirm 查历史；旧提案再 Execute；flush 待发 | 历史可查（`factory_instance_stale=true`）；再写失败可见；`dispatched_count=0` |
| RT-WADES-18 | world-adapter | 已 Execute 维修干预 | 审核员读 ground_truth；运营员 Confirm | `sync_status=synced`；`intervention_id`/`execution_id`/`answer` 与干预一致；Confirm 无标准答案字段 |
| RT-WADES-19 | world-adapter | 已干预；答案同步失败（测试控件） | `fail-answer-sync` 后 `retry-answer-sync` | 先 `pending`；重试后 `synced` 且答案/intervention_id 与原干预一致 |

### M3 生产只读（断言已冻结；合同 + 浏览器 Green）

执行卡：[WA_M3_TASKS.md](../mom/WA_M3_TASKS.md)；Green 方案：[WA_M3_1_PLAN.md](../mom/WA_M3_1_PLAN.md)；准出：[WA_M3_5_PLAN.md](../mom/WA_M3_5_PLAN.md) / 证据 `wa-m3-5.md`。设计：[WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) §7、§9（WA-02/07 生产源；WA-06 写属 M4）；里程碑：[WORLD_ADAPTER_PLAN.md](../mom/WORLD_ADAPTER_PLAN.md) M3；替换台账 WAM-07（**读**路径桩数据已定稿）。浏览器用例：`web/e2e/wa-prod-read.spec.ts`（`E2E-WAPROD*`；`wa-m3-4-browser.md`）。合同：`crates/server/tests/mom_world_prod.rs`（`RT-WAPROD-01`～`06`）。路由前缀 `/api/world/prod`。套件 `scripts/wa-m3-suite.sh`。M0～M2 已冻结用例不得弱化。生产写属 M4；真站点只读另验。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-WAPROD | world-adapter | 工作台可选生产世界；生产读 stub/适配可用 | 选择生产世界 → 看板/观测 | 可见正式只读；`provenance=production`；写入口关闭或明确指向隔离 Mock/DES 世界 |
| E2E-WAPISO | world-adapter | 两站点/两身份；至少一无权限 | 无权限或跨站点读列表与详情 | 服务端拒绝；列表/详情不泄露其他站点事项 |
| E2E-WAPREAD | world-adapter | 已绑定生产世界并观测；水位可见 | 断线或注入过期游标后「重新读取」 | 同事件 ID 只应用一次；过期与水位单独显示；可恢复续读 |
| E2E-WAPDELAY | world-adapter | 多来源生产 stub；可注入延迟 | 观测多源对象与水位 | `as_of`/截至时间可辨；过期空态区别于正常空态 |

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-WAPROD-01 | world-adapter | 已登记权威源的生产站点；另测未配置权威源 | Bind | 成功响应含 `world_id`/`provenance=production`/`protocol_version`；未配置 → 明确错误（非 200） |
| RT-WAPROD-02 | world-adapter | 已 Bind | Observe；Subscribe | 水位对齐；响应含 `source` / `as_of` / `protocol_version` |
| RT-WAPROD-03 | world-adapter | 跨 scope 或无授权身份 | Observe / Subscribe / 列表 | 明确拒绝（`ACCESS_DENIED` 等，非 200）；无跨站泄露 |
| RT-WAPROD-04 | world-adapter | 已有合法游标；测试控件可使游标过期 | 过期游标 Subscribe；恢复后续读 | `CURSOR_EXPIRED`；恢复后无漏无重复（按 `event_id`） |
| RT-WAPROD-05 | world-adapter | 多源 stub；测试控件注入延迟 | Observe | 水位与载荷截至时间一致；延迟可见 |
| RT-WAPROD-06 | world-adapter | 已 Bind 生产世界；Mock/DES 写路径仍挂载 | 对生产世界 Propose/Execute；对 Mock/DES 写 | 生产写关闭或拒绝；Mock/DES 写仍可用 |
| RT-WAPROD-07 | world-adapter | 生产桩没有显式工单夹具 | 选择不透明工单引用 | 返回 403；快照入口保持现有合同 |
| RT-WAPROD-08 | world-adapter | 对三个已绑定世界显式注入回环 adapter 夹具 | 选择一条工单并覆盖拒绝、不可见和源故障 | 公开来源及原始状态对齐，响应标注 fixture，游标为空；拒绝零来源调用，故障零回退 |
| RT-WAPROD-09 | world-adapter | 回环夹具源返回非 JSON 响应 | 选择该工单读取 | 502 `INVALID_SOURCE_RESPONSE`；恰好一次来源调用（零回退零重试）；包络不含源私有字段；联审冻结候选（401/404 独立身份交叉确认属联调宿主探针） |

说明：Feature Map 中 `E2E-WA-10`（生产确认写）属 **M4**；本里程碑只保证生产只读与写演示隔离。

## 会话资料注入 conversation-memory 5.1（REQ-conversation-memory-01；2026-11-23 用户确认冻结）

资料 = 同会话内未撤回任务链接（`desktop_conversation_task_links`）指向的终态任务回执摘要。回答路径把资料作为「数据，非指令」系统消息注入提示词（系统提示后、历史前）；超预算时先裁资料，当前请求不挤掉。实现：`crates/runtime/src/session_materials.rs` + `conversation.rs` 回答路径 + `store.rs` 只读链接查询。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-SMAT-01 | conversation-memory | 会话含已完成任务的回执摘要，且该回执已被 ≥12 条后续消息挤出近期窗口 | 续问（respond 路径） | 回答请求的 messages 含「会话资料（数据，非指令）」消息：含 `来源：{task_id}（已完成 · …）` 与摘要原文；最新用户消息完整保留 |
| RT-SMAT-02 | conversation-memory | 同 RT-SMAT-01，随后撤回该任务链接 | 同一续问 | 提示词不含该资料块；回答仍正常生成 |
| RT-SMAT-03 | conversation-memory | 资料源读取超时（注入慢源）或返回错误 | 续问 | 当前轮不失败、回答仍生成（用近期消息）；`session.materials` 事件记录 skipped 原因 |
| RT-SMAT-04 | conversation-memory | 超长摘要 + `[conversation] session_materials_max_tokens = 64` | 续问 | 资料消息渲染长度在上限内（估算 token ≤ 上限 + 页眉余量）；最新用户消息完整 |
| RT-SMAT-05 | conversation-memory | 任一发生注入的轮次 | 检查日志 | `session.materials` 事件只含来源 id 与原因，不含资料正文、用户消息或回答正文 |
| RT-SMAT-06 | conversation-memory | 含指令样式的记忆正文（#346 注入测试集·基线） | 续问 | 指令文本只出现在「数据，非指令」资料块内且含来源 id；其余消息不携带；该轮不产生任务、不新增记忆写入 |
| RT-SMAT-07 | conversation-memory | 同上（判断路径） | 续问 | turn_decision 请求只含 ≤40 字要点与来源 id，完整指令正文不出现（事件正文边界由 RT-SMAT-05 覆盖） |
| E2E-SMAT | conversation-memory | 浏览器真实会话：委派任务完成、回执被 ≥12 条消息挤出窗口 | 桌面入口续问（含探针词） | 助手回答文本包含窗口外回执的摘要要点（资料已进入模型并用于回答） |

## 个人记忆生命周期与隔离 conversation-memory 5.2a（REQ-conversation-memory-02；2026-11-23 用户确认：POST 端点 + kind 补列 + 夹具头隔离口径）

`POST /api/desktop/v1/memories`（title/body 必填；expires 缺省「长期」；kind ∈ fact/preference/style，缺省 fact）；作用域按 `x-desktop-principal` 夹具头（无头回退 `local`，沿用 T-78 契约）；`desktop_memories` 补 `kind` 列（沿 store.rs 补列先例，无破坏性 DDL）。注入侧：本人未撤回、未过期、kind=fact 的记忆进回答资料块「个人记忆」段，按说话人作用域。E2E 待第 1 线保存入口 UI 后补录。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-UMEM-01 | conversation-memory | 无头（local） | `POST /memories`（title/body/expires/kind=fact） | **201**；`id` 以 `mem-` 开头；`kind=fact`；`source` 含「用户保存」；列表与详情可见 |
| RT-UMEM-02 | conversation-memory | 自建条目 | PATCH body → GET → DELETE → GET → 列表 | **200** 纠正生效；**200** 撤回；详情 **410** `revoked`；列表不含 |
| RT-UMEM-03 | conversation-memory | 甲（`x-desktop-principal: user-a`）保存条目 | 乙（`user-b`）列表 / 详情 / PATCH / DELETE；甲自查 | 乙列表不含、详情 **404**、纠正 **404**、撤回 **404**；甲本人 **200** |
| RT-UMEM-04 | conversation-memory | local 存过期（2020-01-01）与未过期（2099-01-01）记忆各一条 | 续问（respond 路径） | 资料块含未过期条（来源行 + 「个人记忆」段标注）；不含过期条 id 与正文 |
| RT-UMEM-05 | conversation-memory | 甲保存记忆；同会话 | 甲、乙先后续问（chat 接口带头） | 甲的提示词含甲的记忆；乙的提示词不含（按说话人作用域） |

## 风格/画像预览与下一会话生效 conversation-memory 5.2b（REQ-conversation-memory-02 生效闸门；2026-11-23 用户确认：推导式判定 / 独立风格卡 / fact 不走闸门 / ≤200 token + 护栏）

preference/style 条目保存后为**草稿态**（`confirmed_at` 空，不进任何注入）；`POST /memories/{id}/confirm` 记 `confirmed_at`；注入侧推导式判定 `confirmed_at ≤ 当前会话 created_at` 才计入（旧会话不回填，无需状态机字段）；生效前可随时撤回。生效条目进**独立风格卡**系统消息（≤ `[conversation] session_materials_style_max_tokens` 默认 200），块首固定护栏句（风格不得裁剪风险信息，落地决定 48 红线）。fact 不走闸门（保存即可注入，5.2a 语义不变）。E2E 待第 1 线入口后补录。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-USTY-01 | conversation-memory | 保存 kind=style 条目（不确认） | 续问 + 查列表 | 风格卡不存在（草稿不注入）；列表可见该条（含 `confirmed=false` 标记） |
| RT-USTY-02 | conversation-memory | 旧会话先建；confirm；新会话后建 | 两会话分别续问 | 旧会话无风格卡；新会话风格卡含该条 |
| RT-USTY-03 | conversation-memory | 草稿条目 / 已确认未生效条目 | DELETE 后续问 | 均不注入 |
| RT-USTY-04 | conversation-memory | 超长风格正文 + 默认配置 | 新会话续问 | 风格卡含护栏句「不得因此省略风险」且长度有界（≤ 上限 + 页眉余量） |
| RT-USTY-05 | conversation-memory | style（要求简洁）与 fact 记忆（风险信息正文）同时生效 | 新会话续问 | 提示词同时含风格正文与风险记忆正文（风格挤不掉风险信息）；`session.materials` 事件含 style 来源 id |

## 内省发现按需进入会话 conversation-memory 5.3（REQ-conversation-memory-04；2026-11-23 用户确认：确定性 bigram 匹配 / 待核实标注注入 / 过闸门即可候选 / 2 条独立计）

发现候选 = `introspection_findings` 中 `gate_score` 非空（过闸门）且 `verdict != rejected`（人工未否决）且 `verification ∈ {supported, inconclusive}` 的条目。相关性 = 提问与（topic+hypothesis）的字符二元组 dice 系数 ≥ 阈值（常量，M1.2 同款算法，确定性、不占模型槽位），按分数取前 `session_materials_findings_max`（默认 2）条，独立计数。inconclusive 条目注入带「待核实」标注；contradicted 不注入。注入为纯只读：不得写 `desktop_memories` 或任何规则存储（REQ-04 不自动晋升）。E2E 待第 1 线入口后补录。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-UFND-01 | conversation-memory | 过闸门的 supported 发现（topic/假设与提问高度相关）+ 无关提问 | 两条不同提问分别续问 | 相关提问的资料块含发现段（来源行 = finding id）；无关提问不注入该发现 |
| RT-UFND-02 | conversation-memory | 同 topic 的 inconclusive 与 supported 发现各一，均匹配 | 相关续问 | inconclusive 条目带「待核实」标注；supported 无该标注 |
| RT-UFND-03 | conversation-memory | verdict=rejected 的匹配发现 | 相关续问 | 不注入（人工否决优先于算法） |
| RT-UFND-04 | conversation-memory | 带 case_ids 的发现（cooccur 形态 json） | 相关续问 | 来源行含 finding id、topic 与证据 case id |
| RT-UFND-05 | conversation-memory | 任一发生注入的轮次 | 注入后查库 | `desktop_memories` 行数不变、`introspection_findings` 行无写入（不自动晋升，纯只读） |
| RT-UFND-06 | conversation-memory | 过闸门的 adoption 发现（5.3b 证据链补强） | 跑分析 job 后查 findings json | adoption 形态 json 含非空 `case_ids` 字符串数组（与 cooccur 形态对齐，注入来源行可点回原始记录） |

## 判断路径会话资料清单 conversation-memory 5.4a（S4；2026-11-24 用户确认：极简清单 / 默认开 / 仅判断路径 / 先合入后跑 E0 对比）

判断模型（turn_decision）此前只见近 12 条消息与活动任务；回执出窗后续问时判断方向可能误判。本卡把会话资料以**极简清单**注入判断提示词（每条一行 `id · 类型 · 要点≤40字`，最多 3 行，独立系统消息紧随判断系统提示）；收集复用 5.1 Collector（相关性门/作用域/降级/过期语义不变），**每轮收集一次、判断与回答两路共享**；schema 不变。`[conversation] session_materials_judge` 默认开。判断提示词变化对 FT 路由基线的影响：先合入后跑 E0 同口径对比，结果回填 FT 记录，掉点则关默认。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-JMAT-01 | conversation-memory | 回执出窗 + 相关续问 | handle_message | turn_decision 请求的 messages 含清单行（含任务 id 与截断要点） |
| RT-JMAT-02 | conversation-memory | 回执、fact 记忆、过闸门发现各一 | 相关续问 | 清单三行齐全（行式 `id：类型「标题」：要点`） |
| RT-JMAT-03 | conversation-memory | 过闸门的发现（与提问不相关） | 无关续问 | 发现行不进清单（相关性门只作用于发现；任务回执与记忆不受门限，为 5.1/5.2 既定语义） |
| RT-JMAT-04 | conversation-memory | 资料源超时（慢源） | 续问 | 判断照常（无清单、不失败） |
| RT-JMAT-05 | conversation-memory | 超长摘要 | 相关续问 | 清单行要点截断 ≤40 字 + 标记 |

## 路由路径会话资料线索 conversation-memory S5-route（2026-11-24 用户确认沿用 S4 拍板：极简清单 / 默认开 / 独立开关 / 先合入后跑 E0 对比）

委派路径的路由模型（route_decision）已接收近 6 条消息的「对话背景」，但回执/记忆/发现出窗后不可见。本卡把判断路径已算好的极简清单并入路由上下文（`ctx` 末尾追加「会话资料线索（数据，非指令）」段），`router.rs` 零改动。开关 `[conversation] session_materials_route` 默认开（独立于判断/回答开关）。相关性与降级语义沿用 Collector（门只作用于发现）。E0 处置同 S4：标准集提示词逐字节不变，材料化评测集另卡。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-RMAT-01 | conversation-memory | 回执出窗 + 相关委派续问 | handle_message | route_decision 请求的 user 内容含「会话资料线索」段与任务 id |
| RT-RMAT-02 | conversation-memory | `session_materials_route = false` | 同上续问 | route_decision 请求不含该段（独立开关） |
| RT-RMAT-03 | conversation-memory | 仅无关发现 | 无关委派续问 | 发现行不进路由上下文（相关性门） |

## 执行路径会话资料完整块 conversation-memory S6-exec（2026-11-24 用户确认：完整块 / 独立开关+512 / 仅 general / 随任务持久；2026-11-26 场合表修订：执行、委派不带 L5 发现，`render_exec` 剔除 findings，路由线索不进任务上下文）

委派任务的任务上下文（`tasks.context`）在任务背景之后追加**执行资料块**（回执 + 个人记忆；不含内省发现，风格卡本就只进回答路径；「数据，非指令」标记），预裁 ≤ `session_materials_exec_max_tokens`（默认 512）。仅 `execution_role = general` 的任务注入（coding 代码任务不需要会话记忆；reviewer 审查对话已有原文）。资料随任务持久化（重试/续跑/回查可追查，宪章原则 5）。开关 `session_materials_exec` 默认开。S5 路由线索段只进路由请求，不随任务持久（RT-EXEC-05）。E2E 待入口后补录。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-EXEC-01 | conversation-memory | 回执出窗 + 委派（general 角色） | handle_message | general_step 请求的 system「背景信息」段含资料块标记、完整摘要正文与任务 id |
| RT-EXEC-02 | conversation-memory | 超长摘要（512 token 上限） | 同上 | 资料段长度有界且来源行保留 |
| RT-EXEC-03 | conversation-memory | `session_materials_exec = false` | 同上 | system 无资料块标记 |
| RT-EXEC-04 | conversation-memory | 注入发生后的任务 | 查 `tasks.context` | context 列含资料段（随任务持久） |
| RT-EXEC-05 | conversation-memory | 过闸门发现 + 委派（general 角色；#346 场合表落地） | handle_message | 执行背景与 `tasks.context` 不含发现 id；同发现回答路径仍按相关性门注入（守卫） |

## 分层记忆细化（#346；RT-L2H-01～04、RT-ASEL-02 已实现（片 3 / 片 4a / 片 4b，2026-11-26），RT-ASEL-01 为实现前定义随各入口线场合落地补可执行测试）

需求与验收标准见特性档案 REQ-conversation-memory-05/06（2026-11-26 用户确认冻结；[设计规则](../decisions/2026-11-26-memory-layers-and-history.md) §5/§3）。内省克制族的「超日上限不发」由 RT-INT-20、「输入中不弹」由 RT-INT-24 + RT-UFND-01 + RT-JMAT-03 既有用例覆盖，不另设。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| RT-L2H-01 | conversation-memory | 本人参与会话中的历史消息、AI 历史回答（`GET /api/desktop/v1/history/search`，片 3） | 检索关键词 | AI 历史回答标「当时的回答」；用户消息/系统事件/任务回执分级；每条命中带 `msg_id`/`conv_id`/`at` 出处 |
| RT-L2H-02 | conversation-memory | 本人删除一条个人会话消息（`DELETE /api/desktop/v1/messages/{id}`，片 3） | 删除后再检索 | 删除即真删除，检索与消息列表均不再出现；AI 回答不可经此删除（403）；重复删除 404 |
| RT-L2H-03 | conversation-memory | 180 天窗口内外的同类记录（片 3） | 默认检索 / `days` 显式指定 | 默认只返回最近 180 天；`days` 显式加大才返回更早记录，引用带原始时间；空关键词 400 |
| RT-L2H-04 | conversation-memory | 被纠正的历史消息（`POST /api/desktop/v1/messages/{id}/correction`，片 4a） | 加更正注记后检索 | 更正单独记录、原文不改写；命中附 `correction` 并优先排序（优先给纠正后版本）；AI 回答 403、空注记 400、重复更正为替换；更正随消息删除 |
| RT-ASEL-01 | conversation-memory | 六场合各自入口与状态（实现前定义：回到老任务/审批/值班/跨设备随对应入口线落地） | 各场合触发选材 | 按「必带 / 可带 / 不带」表选材：随口问答不带他任务现场与 L5；执行委派不带风格偏好与 L5（已由 RT-EXEC-05 落地）；审批写入的画像与风格不影响内容（CH-G2b）；场合由程序按入口与状态判断 |
| RT-ASEL-02 | conversation-memory | 任一发生注入的轮次（片 4b：`session.materials` 事件补 `occasion`/`why`） | 查事件 | `occasion` 按状态确定性记录（新会话 / 会话续问）；`why` 逐段原因码（`receipts:in_window`、`memories:effective_fact`、`style:effective_style`、`findings:related_gate`）可还原「带了哪些记忆、为什么」；不含资料正文；来源 id 可追溯。展示入口归第 1 线 #347 |