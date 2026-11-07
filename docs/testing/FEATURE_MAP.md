# 当前功能地图

基于远端 929301a；此图同时描述用户路径和可观测判据，机器索引见 features.json。浏览器覆盖由实际测试结果证明，不能只凭 manifest 的 coverage 字段宣称通过。

| Feature | 用户目标与真实入口 | 驱动方法/判据 | 测试与边界 |
|---|---|---|---|
| conversation | 左侧“＋ 新会话”；中间输入框“输入消息” | 发问候、刷新重读、切换会话；最终消息 ID/内容一致，无串入和 think 泄漏 | E2E-CONV、E2E-ACK；多问题质量仍需实际模型验收 |
| navigation | 窄屏顶部“☰ 会话”打开左栏抽屉 | 创建/选择会话后抽屉关闭，发送框可用 | E2E-MOBILE；非固定坐标定位 |
| tasks | 对话中的任务状态/产物，右侧“任务”卡片；简短回复下的走马灯（收起显示最新一行，展开从旧到新，运行中行尾动画） | 真实文件工具写入、质检 pass、COMPLETED；下载字节与 artifact、磁盘一致；走马灯位置、顺序、背景与运行指示 | E2E-FILE、E2E-TICKER、E2E-FAILCARD；进程成功不等于验收通过 |
| approval | 对话中“批准执行”，或展开任务卡 | 高风险先 NEEDS_HUMAN、没有 result；点击后才执行 | E2E-APPROVE；不批准真实危险操作 |
| cancellation | 执行中“取消任务”，或卡片“取消” | 先点变“确认取消”，第二次才 CANCELLED，API 状态一致 | E2E-CANCEL；三秒超时恢复由已有组件测试保护 |
| observability | 右侧底部“开发者模式：关”切开，再点击“诊断”“路由日志” | 诊断显示真实后端；路由日志匹配本次消息，可展开模型原文 | E2E-DIAG；复制剪贴板尚待完整浏览器验证 |
| agent-links | 左下角连接状态：绿=已连通可调用（握手成功，或 10 分钟内有成功调用）；灰=已配置未连通；红=不可用；灰红的原因在悬停提示里 | 内置 agent 始终绿；命令不存在为红；能启动但握手失败为灰；健康接口每个 agent 带 link.status | 单元：runtime link.rs 状态规则；HTTP：server/tests/e2e.rs agent_links_are_red_when_missing_and_grey_when_not_connected；组件：web/src/health.test.tsx；真实 ACP 握手与模型调用未在沙箱验证 |
| nodes | “节点”→按地址添加/发现节点→配对；解除也两步确认 | 以测试节点核对授权、执行、解除后的不可用状态 | 现有 server/tests/e2e.rs node_flow/ACP；真实 LAN 未覆盖 |
| feedback | 展开可匹配用户消息的任务→“派错了”→动作/角色/说明→保存 | POST /api/feedback 信封、持久化记录；非法动作/角色被拒；导出仅为草稿 | 现有 misroute 组件及 Rust HTTP 测试；本次无模板纠正字段 |
| templates | 当前无界面入口（M4.3 尚未接入编排器） | 模板校验、Router 拆分决定、分支信封与汇合输入在 Runtime 集成层验证 | RT-TPL-01 至 RT-TPL-10；Rust 集成测试与单元测试；并行执行与 Composer 汇合回复未实现 |
| tools-evals | 聊天委派工具；终端 websearch、an-eval、dream | 计算/文件/CLI证据、JSON错误、逐条评分和失败草稿；不自动改主评测集 | 现有 Rust/Python 测试；真实搜索时效与模型质量另验 |

应用为单页布局，网页没有 /tasks、/settings 或 /templates/run 路径。API 以 crates/server/src/lib.rs 为准；状态和中文文案见 web/src/labels.ts。

当前不是已实现功能：M4.3 的模板并行执行与 Composer 汇合回复（校验部分见 templates 行）、生产级 MOM 连接器、知识库、Benny 自动修复、多用户登录、通用动态 DAG、执行中断续跑。它们在其他设计中出现不代表当前代码存在。

## 操作定位与维护

优先使用 role/name、aria-label、placeholder，并在 main、会话列表、任务与状态区域限定作用域；重复的下载/取消按钮不要全局盲点。开发者模式默认为关，切换状态保存在浏览器 localStorage，测试使用全新 context。

每次变更功能，先更新 CASES.md 与可执行 E2E，再实现；同步 features.json 的源文件和测试引用。浏览器测试不 mock 应用 API；新增功能应先见到业务断言 Red，既有功能基线不可追称测试先行。节点、搜索、模型真实质量不能由本次六条用例推定通过。

## MOM 模拟功能

| Feature | 用户目标与真实入口 | 驱动方法/判据 | 测试与边界 |
|---|---|---|---|
| mom | 独立 an-mom 人类看板与 Agent 机器视图 | S1 审批后下发、核验再关闭；S2 初复独立，缺证据不能关闭8D | E2E-MOM/E2E-COMPLAINT；业务系统、主模型及审批身份仍为替代件，见 MOM Mock 清单 |

每日审查回归：RT-MOM-01～05（`crates/runtime/tests/mom.rs`）覆盖联锁、终态、事件时间、失败重放和阻断案例租约。HTTP 联锁入口由 `crates/server/tests/mom.rs::safety_trip_rejects_http_patch_and_dispatch_without_side_effects` 验证；它不代表浏览器安全路径已验证。执行证据见 `docs/review/2026-11-06-fixes.md`。
