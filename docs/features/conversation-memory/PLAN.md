# 第 5 线实施计划

总体范围见[特性档案](README.md)，跨线汇合见[集成计划](../../INTEGRATION_PLAN.md)。

- [x] 纳入既有业务内省基线。见[总计划 INT](../../PLAN.md#业务内省int)及已登记用例；沿用现有默认开关。
- [x] 5.1 会话中按需注入（REQ-conversation-memory-01，2026-11-23 用户确认冻结验收）。单用户受控环境验证同一会话续问、来源可查、失效与超时降级，独立于生产站点和首页可靠受理排期。落地：回答路径注入同会话未撤回任务链接的回执摘要（`crates/runtime/src/session_materials.rs`）；RT-SMAT-01～05、E2E-SMAT；证据 `testing/evidence/convmem-s1-session-materials.md`。判断 / 路由 / 执行路径的选材另卡。
- [ ] 5.2 个人记忆（REQ-conversation-memory-02）。以 [T-71 桌面夹具列表](../../testing/evidence/ai-desktop-t71-memory-list.md)与 [T-72 纠错与撤回](../../testing/evidence/ai-desktop-t72-memory-write.md)为交互基线，优先验证用户主动保存、预览、修订与撤回；第 1 线提供用户入口，第 2 线复验隔离和生效时点。
- [ ] 5.3 内省与会话联动（REQ-conversation-memory-04）。利用 DES 样本进行独立验收，保留 INT 既有证据要求。5.1 通过后可与 5.2 并行。
- [ ] 5.4 团队与组织范围（REQ-conversation-memory-03）。服务端身份及来源授权可验证后，逐个接入共享来源和组织判断标准，验证权限撤回及版本生效。

以上为拟议实施顺序。各阶段在任务卡中锁定需求、用例、运行模式和证据后进入开发。
