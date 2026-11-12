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

每次输出日志、测试结果和截图；失败保留 trace。正向通过只证明这些路径，未覆盖功能在地图中明确标注。另用校验器测试注入重复 ID、未知用例、缺文档和缺测试，确认地图失配确实失败。

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
| RT-TPL-10 | templates | 后端 llama_cpp 配置 background_slots=[2,3,4,5]；openai 配置 max_inflight=8、reserved_interactive=1；openai 不限并发 | 读取 background_capacity | 分别为 4、7、不限（None） |

## MOM 可配置持续仿真（实现前定义）

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-MOMDYNAMIC | mom | 主界面与独立沙盘数据库 | 从主界面进入MOM，调整订单数量和seed创建持续仿真，推进、播放/暂停并查看gap | 同源入口可达；配置影响方案；事件逐步进入案例；暂停与运行状态可见；gap显示24项的真实状态而非仅ID |

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

## HTTP Host 防护（实现前定义）

| Case | Feature | Given / When | Then |
|---|---|---|---|
| RT-HOST-01 | http-host | 启动真实 agentnexus/an-mom 二进制，恶意 Host 请求写入口、页面、API、SSE、同源 MOM | 请求在业务副作用前返回403；业务记录不变；转发头不能绕过 |
| RT-HOST-02 | http-host | 本机 IPv4/localhost/IPv6 authority、错误端口、伪装域名、非法/缺失/重复 Host | 合法且端口匹配才允许；其余400或403 |
