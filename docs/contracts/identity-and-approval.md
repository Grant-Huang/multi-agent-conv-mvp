# 共享契约：身份与审批

| 项目 | 内容 |
|---|---|
| 状态 | **部分有效**（A1–A7、B1–B4 已确认；真实登录与夹具迁移仍待定） |
| 核对基线 | 远端 `master` 的 `43014db`（2026-11-22） |
| 适用特性 | 任务审批、助手配置确认、Agent 准入、MOM 案例批准、运营频道批准、AI 桌面审批与证据权限、World Adapter 执行、业务内省权限 |
| 确认决策 | [2026-11-22-c2-m2-minimal-contracts](../decisions/2026-11-22-c2-m2-minimal-contracts.md) |

## 1. 范围

所有“需要人同意才能继续”的动作，以及所有“按人决定能不能看、能不能做”的检查。

## 2. 现状

目前没有登录。各特性各自处理身份和批准：

| 特性 | 现在怎么认人 | 现在怎么批准 | 位置 |
|---|---|---|---|
| 任务审批（引擎） | 不认人；只监听本机，加 Host 校验 | `POST /api/tasks/{id}/approve`，不记录批准人 | `crates/server/src/lib.rs`；DESIGN 第 13.1 节 |
| 助手配置 | 不认人 | 一次性确认凭证（10 分钟） | DECISIONS 第 1、9 条 |
| Agent 准入 | 不认人 | `approver` 是任意字符串 | 台账 R-20261110-M1 |
| MOM / 运营频道 | 固定夹具身份 `supervisor` | 卡片带 `scope`、`plan_revision`，重复点击幂等，方案过期回 409 | 决定 60；[mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) 第 2 节 |
| AI 桌面 C2 动作网关 | 夹具请求头 `x-desktop-principal` | `/api/desktop/v1/approvals/*`：绑定 `revision`，拒绝/过期/升版后不可执行；`decided_by` 落库 | #198；决定 [2026-11-21-ai-desktop-c2-gateway-attn](../decisions/2026-11-21-ai-desktop-c2-gateway-attn.md) |
| AI 桌面证据 | 同上夹具 | 证据按授权读取，无授权回 403 | 决定 69 |
| World Adapter DES 写 | 沿用 MOM 批准语义 | Propose/Execute 绑定 `case_id` + `plan_revision`；过期或变化 → `APPROVAL_INVALID` | #199；决定 [2026-11-21-wa-m2-1-green-service-layer](../decisions/2026-11-21-wa-m2-1-green-service-layer.md) |
| 业务内省 | `is_operator` 恒为真，测试可注入拒绝 | — | PLAN「INT」S5 |

## 3. 已确认的不变量

| # | 不变量 | 来源 |
|---|---|---|
| A1 | 一次批准绑定**操作者、动作参数版本和有效期**；参数变化、过期或权限撤回都必须重新确认；拒绝和超时阻止继续执行。版本字段：MOM / World Adapter 用 `plan_revision`；Desktop C2 动作网关用 `revision` | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) 第 6 节；#198、#199 |
| A2 | 审批界面展示的内容就是被批准的内容：按服务端保存的提案展示；动作内容或前置条件变了就生成新提案、重新审批 | [mom/WORLD_ADAPTER.md](../mom/WORLD_ADAPTER.md) 第 4.3 节 |
| A3 | 同一批准只能消费一次；重复提交按幂等键返回原结果，不重复执行。同幂等键、不同载荷 → 冲突（如 `IDEMPOTENCY_CONFLICT`），可找回原执行记录，不第二套执行 | WORLD_ADAPTER 第 4.4 节；决定 60；RT-WADES-10；RT-ADAPPR-04 |
| A4 | 能力列表只用于发现，服务端每次重新验证授权 | WORLD_ADAPTER 第 3 节 |
| A5 | 会话 agent 只能观察、续读、形成方案和查看结果；审批决定由人从授权入口作出 | WORLD_ADAPTER 第 3 节 |
| A6 | 开放生产写入前，必须完成生产身份和角色映射；生产授权额外绑定实际操作者和职责范围 | WORLD_ADAPTER 第 1、4.4 节 |
| A7 | 查询、引用、通知、检索和执行都按服务端认证的身份与授权范围过滤；权限撤回后清理相应缓存和预览 | ai-desktop/DESIGN 第 5 节 |
| B1 | **人的操作凭证与 agent 隔离**：批准、配置确认、准入审批、业务写批准只接受浏览器会话持有的凭证（例如 HttpOnly cookie 加 Origin 校验），系统启动的子进程拿不到。实现可分阶段；本阶段至少在文档与夹具路径上声明隔离，不得把 agent 可持有的令牌当作人的批准凭证 | 决定 2026-11-22-c2-m2-minimal-contracts（D2） |
| B2 | 每次批准都记录批准人、时间、批准的内容摘要（或可核对的版本/摘要引用） | 同上；C2 已落 `decided_by` |
| B3 | 所有批准走同一个模型（A1～A3）；不同特性只是不同的动作类型与**授权入口**。C2/M2：运营业务写批准入口 = 运营频道 OpsCase 卡；桌面任务级动作 = Desktop 审批网关；两入口、同一模型，不合并 API | 同上（D1）；决定 60 |
| B4 | 没有真实身份前，夹具身份在界面和记录里标明“测试身份” | 同上；宪章原则 8 |

## 4. 建议补充的不变量（助手建议，待确认）

（本轮无新增建议；原 B1–B4 已升入第 3 节。）

## 5. 待决定

1. 真实登录排期：不挡 C2/M2 契约生效；排到 Desktop 后续能力或 World Adapter M3 之前再定（D3 已确认「本轮不排期」）。
2. 现有夹具身份（`supervisor`、`alice`）在接入真实身份后如何迁移。

## 6. 验收

| 不变量 | Case ID | 场景 |
|---|---|---|
| A1 / A2 | RT-ADAPPR-01、RT-WADES-09 | 合法审批成功；修订变化后旧批准被拒 |
| A1 | RT-ADAPPR-02、RT-ADAPPR-03 | 升版或拒绝后执行被拒 |
| A3 | RT-ADAPPR-04、RT-WADES-10 | 恰好一次；幂等冲突可找回原 `execution_id` |
| A5 / B3 | 决定 60；运营卡路径 | 人从授权入口批准；agent 不自批 |
| B2 | RT-ADAPPR-01 | `decided_by` 非空 |
