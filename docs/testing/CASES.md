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

每次输出日志、测试结果和截图；失败保留 trace。正向通过只证明这些路径，未覆盖功能在地图中明确标注。另用校验器测试注入重复 ID、未知用例、缺文档和缺测试，确认地图失配确实失败。

## MOM 独立服务验收

以下浏览器用例为现有 MOM 实现的补充基线，不追称为实现前 Red。运行时测试先行与业务失败修复证据另见 MOM 实施说明。

| Case | Feature | Given | When | Then |
|---|---|---|---|---|
| E2E-MOM | mom | 独立 Mock 服务与新数据库 | 创建S1、切双视图、批准、下发、推进并核验 | 硬约束可见；批准后才下发；实际模拟回执核验后关闭 |
| E2E-COMPLAINT | mom | 同一服务中的S2客诉 | 查看未知根因和8D、批准初复、尝试关闭 | 初复不等根因调查；消息仅进模拟发件箱；缺D4/D6不允许关闭 |

## 运行时用例（Runtime 集成层，实现前定义）

浏览器用例之外，M4.3 固定模板的确定性校验没有界面入口，按设计在 Runtime 集成层验收。每条用例对应一个以 `rt_tpl_NN_` 开头的 Rust 集成测试函数（`crates/*/tests/*.rs`），由 `web/verification/check-map.mjs` 检查映射。模板并行执行与汇合回复尚未接入编排器，不在本组用例内。

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
| RT-TPL-10 | templates | 后端 llama_cpp 配置 background_slots=[2,3,4,5]；openai 配置 max_inflight=8、reserved_interactive=1；openai 不限并发 | 读取 background_capacity | 分别为 4、7、不限（None） |
