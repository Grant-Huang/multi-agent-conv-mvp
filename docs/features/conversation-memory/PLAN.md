# 第 5 线实施计划

总体范围见[特性档案](README.md)，跨线汇合见[集成计划](../../INTEGRATION_PLAN.md)。

- [x] 纳入既有业务内省基线。见[总计划 INT](../../PLAN.md#业务内省int)及已登记用例；沿用现有默认开关。
- [x] 5.1 会话中按需注入（REQ-conversation-memory-01，2026-11-23 用户确认冻结验收）。单用户受控环境验证同一会话续问、来源可查、失效与超时降级，独立于生产站点和首页可靠受理排期。落地：回答路径注入同会话未撤回任务链接的回执摘要（`crates/runtime/src/session_materials.rs`）；RT-SMAT-01～05、E2E-SMAT；证据 `testing/evidence/convmem-s1-session-materials.md`。判断 / 路由 / 执行路径的选材另卡。
- [ ] 5.2 个人记忆（REQ-conversation-memory-02）。以 [T-71 桌面夹具列表](../../testing/evidence/ai-desktop-t71-memory-list.md)与 [T-72 纠错与撤回](../../testing/evidence/ai-desktop-t72-memory-write.md)为交互基线，优先验证用户主动保存、预览、修订与撤回；第 1 线提供用户入口，第 2 线复验隔离和生效时点。
  - [x] 5.2a 生命周期与隔离（2026-11-23）：`POST /memories` + `kind` 补列 + `x-desktop-principal` 作用域 + 过期不入注入；RT-UMEM-01～05；证据 `testing/evidence/convmem-s2a-personal-memory.md`；决策 `decisions/2026-11-23-convmem-s2a-lifecycle-and-isolation.md`
  - [x] 5.2b 预览与下一会话生效（2026-11-23 用户四项拍板：推导式判定 / 独立风格卡 / fact 不走闸门 / ≤200 token + 护栏）：`confirmed_at` 补列 + `POST /memories/{id}/confirm`；草稿不注入、旧会话不回填；风格卡带决定 48 风险护栏句；RT-USTY-01～05；决策 `decisions/2026-11-23-convmem-s2b-style-gate.md`
- [ ] 5.3 内省与会话联动（REQ-conversation-memory-04）。利用 DES 样本进行独立验收，保留 INT 既有证据要求。5.1 通过后可与 5.2 并行。
- [x] 5.4 团队与组织范围（REQ-conversation-memory-03）——**2026-11-24 用户拍板移交第 4 线**（服务端身份及来源授权可验证后，逐个接入共享来源和组织判断标准，验证权限撤回及版本生效）。第 5 线职责保留：资料选材侧按 principal 作用域已就绪（5.2a），身份落地后接共享来源。

以上为拟议实施顺序。各阶段在任务卡中锁定需求、用例、运行模式和证据后进入开发。

## S 系列追加卡（档案 §3「分别为对话判断、路由和执行设计最小材料」）

- [x] S4 判断路径会话资料极简清单（2026-11-24 用户确认：极简清单 / 默认开 / 仅判断路径 / 先合入后跑 E0 对比）：每轮收集一次两路共享；`session_materials_judge` 默认开；RT-JMAT-01～05；决策 `decisions/2026-11-24-convmem-s4-judge-materials.md`；E0 同口径对比结果回填 FT 记录（标准集提示词不变，材料化评测集另卡）
- [x] S5-route 路由路径会话资料线索（2026-11-24 沿用 S4 拍板）：路由 ctx 追加极简清单段，router.rs 零改动；`session_materials_route` 默认开；RT-RMAT-01～03；决策 `decisions/2026-11-24-convmem-s5-route-materials.md`
- [x] S6-exec 执行路径会话资料完整块（2026-11-24 用户确认：完整块 / 独立开关+512 / 仅 general / 随任务持久）：general 任务上下文追加 render 完整块（512 预裁），coding/reviewer 不注入；RT-EXEC-01～04；决策 `decisions/2026-11-24-convmem-s6-exec-materials.md`。**2026-11-26 修订（#358 / CH-G2e，用户拍板）**：个人记忆撤回即失效——撤回时清扫未终态任务 context 中该记忆资料段（已终态保留原件供审计），批准/重试/重启续跑同源生效；RT-EXEC-06；决策 `decisions/2026-11-26-convmem-revoke-context-invalidation.md`
- [ ] 材料化评测集（E0 之外的相关性评测）

## 分层细化卡（#346，2026-11-26 宪章 v1.6 与[分层设计规则](../../decisions/2026-11-26-memory-layers-and-history.md)派工）

- [x] 片 1 特性档案登记：L0～L5 分层、按场合选材、克制内省、L2 历史检索规则、可编辑范围、注入八道防线写入档案 §2/§3；REQ-05～07 登记（提出）；登记表同步
- [x] 片 2 CASES 与可执行测试（2026-11-26，先 Red）：REQ-05～07 用户确认冻结（会话确认，状态与用例挂接落 features.json）；注入基线 RT-SMAT-06/07（封装隔离 / 判断清单只带要点，既有行为补录）；**场合表落地修正**——S6-exec 执行块用 `render_exec` 剔除 L5 发现、S5 路由线索不再随任务上下文进执行背景（RT-EXEC-05，Red→Green）；内省克制族映射既有 RT-INT-20 / RT-INT-24 / RT-UFND-01 / RT-JMAT-03；L2 检索 RT-L2H-01～03 与场合选材 RT-ASEL-01～02 登记「实现前定义」随片 3/4 补测试；证据 `testing/evidence/convmem-layers-slice2.md`
- [x] 片 3 L2 历史记录检索（2026-11-26，方案经用户批准）：`GET /api/desktop/v1/history/search`（principal 说明见端点文档；默认 180 天窗口、AI 历史回答标「当时的回答」、消息+已完成任务回执两路检索、引用带 `msg_id`/`conv_id`/`at`）+ `DELETE /api/desktop/v1/messages/{id}`（仅本人 user 消息真删除，AI 回答 403，重复 404）；RT-L2H-01～03；store.rs 新增 `search_history`/`delete_message`（只读查询 + 单条删除，无迁移）；证据 `testing/evidence/convmem-layers-slice3.md`
  - [x] 片 4a 更正注记（RT-L2H-04）：`POST /api/desktop/v1/messages/{id}/correction`（仅本人 user 消息，AI 回答 403、空注记 400）；`message_corrections` 新表（更正单独记录、原文不改写、重复更正替换、随消息删除）；检索命中附 `correction` 并优先排序（REQ-05「优先给纠正后版本」）；证据 `testing/evidence/convmem-layers-slice4a.md`
  - [x] 片 4b 「带了哪些记忆、为什么」事件记录（RT-ASEL-02）：`session.materials` 事件补 `occasion`（新会话 / 会话续问，程序按状态判断）与 `why`（`receipts:in_window` / `memories:effective_fact` / `style:effective_style` / `findings:related_gate` 原因码）；DESIGN §10 事件字段同步；展示入口归第 1 线 #347；证据 `testing/evidence/convmem-layers-slice4b.md`
- [x] 片 5 注入防线落地（2026-11-26）：写入面扫描 `flags`（`injection_risk` / `sensitive_info`，读时派生不阻断不改库，RT-UMEM-06）；撤回影响追溯（`materials_injections` 注入留痕表 + `GET /memories/{id}/impact`，含已撤回记忆可追溯，RT-UMEM-07）；外部候选流的架构事实如实登记（唯一写入面为本人 POST，自动抽取引入时须先落候选闸门）；证据 `testing/evidence/convmem-layers-slice5.md`。**本片 `Closes #346`**
- [ ] 片 4 按场合选材（六场合确定性判断）与「带了哪些记忆、为什么」记录
- [ ] 片 5 注入防线落地（写入闸门外部候选、撤回影响追溯、长度预算）与注入测试集

配套：界面入口与「用了哪些记忆」展示归第 1 线（#347）；注入与跨用户隔离复验归第 2 线；会话调用边界归第 4 线。REQ-05～07 验收标准待用户确认后冻结（本档案 §2）。

## 设置风格生效卡（#403 / INTENT-034，2026-11-26 用户拍板方案 a）

- [x] 设置里的沟通原则（Soul）/ 质量偏好（Taste）视作已确认风格进风格卡：保存即确认、全会话生效（不沿用记忆的「确认晚于会话创建不回填」——设置是持续配置）；默认值视作未设置不注入（清空字段即停用）；来源 id `profile:soul` / `profile:taste`（CH-G1 可说明）；护栏句与 ≤200 token 预算沿用 5.2b；个人质量偏好 ≠ 组织 taste（CH-G2 组织层，不经个人设置）；RT-USTY-07；决策 `decisions/2026-11-26-convmem-settings-style-effective.md`；证据 `testing/evidence/convmem-403-settings-style.md`。界面文案与「改了就生效」展示归第 1 线（#306/#347 配合）。

## 观测性卡（P3 小改）

- [x] RT-EXEC-09 委派执行资料埋点（#364 / R-20261126-L1）：委派成功处补发 `session.materials.exec`（`task_id` / `exec_injected` / `reason`：injected、switch_off、empty、role_skipped / `exec_chars`）；DESIGN §10.3 事件字段登记；只记量与原因不记正文；证据 `testing/evidence/convmem-364-exec-telemetry.md`。

## 测试基建卡（W2 本地数据集）

- [x] RT-L2H-05 免 LLM 历史消息批量导入（#440，来源第 6 线 #425 W2）：`POST /api/desktop/v1/history/import`（test_fixtures 门控，生产 404；单次 ≤500 条；role/content/created_at 直写）；全程零模型请求；检索按原始时间命中；证据 `testing/evidence/convmem-440-history-import.md`。

## 撤回清扫加固卡（夜间评审 R-20261127）

- [x] RT-EXEC-10 多段正文记忆撤回整体摘除（#501 / R-20261127-H1，P1）：render 六处摘要过 fold_summary 折叠换行（条目=「来源：」+「摘要：」两行不变量）；strip_memory_from_context 条目边界改按行首「来源：<id>（个人记忆」精确匹配（兄弟条目摘要中的「来源：」字样不误扩）；单测多段整体摘除 + 兄弟条目不误扩；证据 `testing/evidence/convmem-501-strip-boundaries.md`。
