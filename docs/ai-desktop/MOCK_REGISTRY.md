# AI Desktop Mock 注册表与替换计划

状态：已纳入实施（用户 2026-11-20 确认：A 阶段快速完整前端；每里程碑全前端可用；未就绪后端用 Mock；E2E = Feature Map + Playwright；回归覆盖设计特性清单）。  
契约与门禁见 [PLAN.md](PLAN.md) §2–§3；产品要求见 [DESIGN.md](DESIGN.md) §1、§10。

## 1. 规则（不可破）

1. **同一前端**：Mock 与真实服务共用组件、导航、状态归属与 Feature Map 步骤。  
2. **模式显式**：界面/配置标明「模拟」或「真实」；生产配置禁用 Mock 与测试控制入口。  
3. **禁止静默回退**：真实模式缺服务 → 明确不可用，不得偷偷切 Mock。  
4. **替换五步**（每条 M*）：冻结契约 → Mock/真实契约对测 → 混合模式全量 Playwright → 验持久性/副作用 → 从真实路径删替身并更新本表。  
5. **A 出口前**：下表每条有种子、故障夹具、预定替换里程碑；负责人在开工该 Mock 时填入。

## 2. 注册表

| ID | 能力 | A 阶段交付（开发） | 种子 / 故障夹具 | 替换里程碑 | 替换验收（真实） | 状态 |
|---|---|---|---|---|---|---|
| M1 | 对话、任务、上下文、历史、首页卡片 | 有状态 HTTP；SMT-03=`TASK-2026-018`、产品改进、独立会话；受理/拒绝/延迟/误关联/纠正 | 双 Task 草稿串写；迟到响应；操作 ID 冲突 | **B1/B3** | 一条消息一次派发；历史与 ConversationTaskLink 持久；与现有 `/api/conversations*`、`/api/tasks*` 对齐后可混跑 | **混合**：**B1 受理 API + 面板已接真**（T-61；E2E-ADACCEPT 真实 HTTP；丢回执为客户端夹具）；B3 关联已接；首页卡/`draftStore` 仍本地；会话 `POST .../messages` 仍可用 |
| M2 | Run、进度、产物、事件流 | 状态机 + 受控时钟；等待输入/取消/失败/重试；可读产物；重复与过期游标事件 | 断线重连；乱序/重复 event_id；游标过期→重拉快照 | **B2** | HTTP/SSE 重连与进程重启后对账；主 Run 不被迟到事件回退 | **混合**：**B2 SSE + Resume 面板已接真**（T-64；E2E-ADRESUME 真实 HTTP/SSE；fixture 注入无新表）；独立 Run 表与 orchestrator 续跑仍待后续 |
| M3 | 助理配置、记忆、偏好、已读 | 可改配置；保存失败与版本冲突；未读与已读幂等 | 冲突后保留编辑；已读重复提交 | **B3**（基础设置）→ **C2**（记忆建议/投递） | 配置持久与 expected_version；已读跨端一致 | **混合**：**B3 preferences**（T-70）；**记忆列表+纠错/撤回夹具**（T-71/T-72；无新表；E2E-ADMEM/ADMEMW）；**C2 Attention**（RT-ADATTN）；**已读跨端夹具**（T-73；`POST/GET .../assistant/messages/read`；E2E-ADXR；无新表）；持久化记忆表仍待 |
| M4 | 频道、RelOS、MOM/证据权限 | 两角色；浏览来源/对象；撤权；来源失效；**可挂现有真实 MOM desk/频道**（有则真、无则 Mock） | 撤权后搜索/摘要/下载/通知全拒；缓存清除 | **C1** | 组织身份与 ACL 生效；与运营 D3a/真实 desk 读模型汇合 | **混合**：**C1 证据 ACL 已接**（夹具 `x-desktop-principal`；RT-ADACL；E2E-ADSOURCES 真实 HTTP）；运营 desk/频道仍混合；完整 IdP 仍待后续 |
| M5 | 动作审批、业务写入、主动提醒 | 方案版本绑定；批准/拒绝/超时/回执未知；去重/静默/紧急；副作用计数器 | 改参后旧批失效；静音后不再投递普通建议 | **C2** | 真实动作网关：权限、审计、副作用恰好一次；可衔接运营频道真实批准（决定 60） | **混合**：**C2 桌面审批网关已接**（RT-ADAPPR；副作用计数；E2E-ADAPPR）；Attention 真实投递已接；真实 NexusOps 厂写仍待；不并 OpsCase 批准 |
| M6 | 语音与跨端 | 脚本化麦状态、部分/最终转写、插话、断线改文字；双浏览器上下文 | 拒麦权限；草稿冲突双副本 | **D1/D2** | 真实媒体与两设备；指标见 DESIGN §9 D01 | **混合**：**D fixture + T-75 真机 + T-76 ASR stub + T-77 双 context D01**（E2E-ADD01；第二物理机另验） |

SMT-03 在 **A 阶段允许纯 Mock**（`TASK-2026-018`）；进入 B 后与真实 `TaskRecord` id 策略对齐（映射表或改种子），不得破坏已冻结 AD-* 断言的可观察结果。

## 3. 与现有真实能力的关系（混合模式）

A1 一次换壳后，下列能力**优先接真实**（已有协议），其余走 Mock：

| 真实（已有） | Mock 仍覆盖的缺口 |
|---|---|
| 会话/消息/TurnOutcome、任务列表与事件 SSE | 可靠受理 202、操作 ID 幂等、多对多关联 |
| 运营 desk / ops-channel / OpsCase 批准（D0–D2） | 首页卡片区、S1 浮层、证据面板、桌面快照 |
| 内省 unread/findings（若开启） | Personal Agent 设置中心、通用 Attention 策略 |

替换顺序默认：**M1 → M2 → M3 → M4 → M5 → M6**（与里程碑 B1→B2→B3→C1→C2→D 一致）。可并行准备，但合并进真实路径须按上表验收，且每次合并后跑 **完整 Feature Map Playwright**。

### A5 出口（2026-11-20）

| 检查 | 状态 |
|---|---|
| M1–M6 均有 A 阶段交付物或混合真实挂载，且预定替换里程碑已填 | **通过**（M4 证据 ACL 已接；完整 IdP 仍后置） |
| 桌面 AD-* Playwright 全量 | **15/15**（`web/e2e/desktop.spec.ts`；证据 `docs/testing/evidence/ai-desktop-a5.md`） |
| 下一步 | 真机 mic 实现（待确认 [MIC_REAL_PLAN](MIC_REAL_PLAN.md)）；双设备另卡；B1–D fixture 已接；M3 夹具主线已收口 |

## 4. 证据与登记

- 用例：`docs/testing/CASES.md`、`features.json` 中 AD-*；清单逐条追溯在 A0 建立。  
- 覆盖：`docs/feature-map/COVERAGE.md`；每条替换附 evidence（Mock 绿 + 真实集成绿）。  
- 本表「状态」列取值：`计划中` → `A 已接` → `混合` → `已替换`（并填 PR/版本）。  
- A5 整体收口证据：`docs/testing/evidence/ai-desktop-a5.md`（含分卡 a5.1–a5.4 索引与 B/C/D 后续项）。
