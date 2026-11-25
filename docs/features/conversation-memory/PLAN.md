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
- [x] S6-exec 执行路径会话资料完整块（2026-11-24 用户确认：完整块 / 独立开关+512 / 仅 general / 随任务持久）：general 任务上下文追加 render 完整块（512 预裁），coding/reviewer 不注入；RT-EXEC-01～04；决策 `decisions/2026-11-24-convmem-s6-exec-materials.md`
- [ ] 材料化评测集（E0 之外的相关性评测）