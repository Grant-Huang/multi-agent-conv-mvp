# AI Desktop V1.7 对照现状：缺口与计划调整

状态：缺口分析有效；**§5–§6 产品方向已于 2026-11-20 用户确认**（决定 61）。对照 [DESIGN.md](DESIGN.md) / [PLAN.md](PLAN.md) / [FEATURE_MAP.md](FEATURE_MAP.md) / [MOCK_REGISTRY.md](MOCK_REGISTRY.md) 与主分支代码。  
基线：设计规格所校订的协议基线 + 当前主分支已落地能力（运营桌面 D0–D2、业务内省 S0–S7、WP1 部分）。

## 1. 结论

V1.7 把产品定义成**组织级 AI 工作桌面**（S1/S2/S3 + 首页分区 + 侧栏 + 统一面板），并规定 **A 先完整前端（Mock 贯通）→ B/C/D 按能力换真实服务**。  
当前主 Web 仍是「会话列表 + 中间对话/运营桌面页签 + 右侧任务/内省」的 MVP 壳，**不是** V1.7 的桌面壳。运营桌面（看板）与运营频道批准已落地，但只覆盖 V1.7 里「运营入口复用 MOM 三入口」的一小段。

**已确认方向：不采用「Desktop + 旧壳」双轨，直接做组织级 AI 桌面。** A1 **一次换壳**；原 WP1/运营/内省能力作为桌面内模块与 Mock→真实替换项，不再保留平行一级导航。原先 INTEGRATION_PLAN 的打通目标并入 Desktop 里程碑（尤其 B/C 与 M4/M5），不另起一套前端。

```mermaid
flowchart TB
  subgraph target[V1.7 目标壳]
    Home[我的 AI 桌面 三区]
    S123[S1 浮层 / S2 分屏 / S3 专注]
    Panel[统一侧边面板]
    Ops[运营：看板·任务·资源]
  end
  subgraph now[现状主 Web]
    Conv[会话列表 + ChatView]
    Tab[会话 | 运营桌面页签]
    Right[任务 / 内省 / 助手…]
    OpsBoard[OpsDesk 仅看板]
  end
  Home -.->|缺口 AD-G1| Conv
  S123 -.->|缺口 AD-G2| Tab
  Ops -.->|缺口 AD-G3| OpsBoard
  Panel -.->|缺口 AD-G4| Right
```

## 2. 已对齐 / 可复用（不是缺口）

| V1.7 要求 | 现状 | 说明 |
|---|---|---|
| 复用会话/任务 HTTP + SSE | 已有 | DESIGN §4 已核对；A 阶段可直接接真实接口 |
| 运营桌面入口 + MOM 三入口 IA | 看板已有 | D0–D2：desk API、OpsDesk、ops-channel、卡片批准 |
| 案例卡批准（scope/plan_revision） | 已有 | 决定 60；对应 Desktop C 的「动作审批」前身之一 |
| 业务内省未读/页签 | 已有 | `/api/introspection/*`；可作主动消息/组织来源的一种，但不是 Personal Agent |
| ChatCard / 消息锚点 Message.id | 已有 | 与草稿/阅读锚点设计兼容 |
| Task 事件 after 游标 | 部分 | 有 `GET /api/tasks/{id}?after=`；无 Desktop 游标回放 SSE |

## 3. Desktop 专属缺口（AD-G）

严重度相对 **V1.7 A 出口**（完整前端可回归）而言。

| # | 缺口 | 严重度 | 证据 | 影响 | 建议挂靠 |
|---|---|---|---|---|---|
| **AD-G1** | 无「我的 AI 桌面」首页三区与卡片模型 | 高 | `App.tsx` 默认进会话列表，无 zone/card snapshot | A 的 SMT-03 多入口（AD-NAV）无法成立 | Desktop **A1** |
| **AD-G2** | 无 S1/S2/S3 工作区状态机 | 高 | 仅 ChatView + TaskPanel 并排，无浮层/专注布局与恢复 | A02/A03、AD-LAYOUT 不过 | Desktop **A1/A2** |
| **AD-G3** | 运营桌面缺「任务」「资源」子入口 | 高 | `OpsDesk` 只有看板；OPS_DESK_TASKS §8 待确认 | 与 UI_UX_PRINCIPLES 三入口及 V1.7「复用 MOM 视图」不符 | **D3a**（并入 Desktop A，见 §5） |
| **AD-G4** | 无统一侧边面板栈（证据/历史/设置/助理） | 高 | 右侧多 tab 平铺；无层级返回与草稿保活 | A04/A05、AD-EVIDENCE/PROFILE 不过 | Desktop **A3/A4** |
| **AD-G5** | 草稿、阅读锚点、布局偏好无按 Conversation/Task/设备保存 | 高 | 仅记住当前会话 id；无草稿命名空间 | AD-DRAFT、跨任务串写 | Desktop **A2**（先本地，B3 上云） |
| **AD-G6** | 无「转为任务 / 纠正关联 / 作用域」产品流 | 中 | 发送即现有 TurnOutcome；无 ConversationTaskLink | AD-SCOPE、误关联修正 | Desktop **A2** + 后端 **B3** |
| **AD-G7** | 无 Desktop 扩展协议 `/api/desktop/v1` | 中 | DESIGN §5 提案未实现 | B 以后可靠受理、快照、偏好 | Desktop **B***；A 用 Mock |
| **AD-G8** | 无 Run/Artifact/Approval 独立模型 | 中 | 执行态投影自 TaskRecord + TaskEvent | AD-RESUME、产物版本、审批版本 | Mock **M2/M5** → **B2/C2** |
| **AD-G9** | Personal Agent / 统一设置中心缺失 | 中 | 助手页签≠助理身份；无记忆界面 | AD-PROFILE | Desktop **A4** → **B3/C2** |
| **AD-G10** | 主动消息策略（去重/静音/不抢焦点）未产品化 | 中 | 内省有闸门但非通用 Attention | AD-ATTENTION | Mock **M3/M5** → **C2** |
| **AD-G11** | 语音与跨端仅设计 | 低（相对 A） | 无麦克风链路 | D 里程碑 | Desktop **A5 Mock** → **D** |
| **AD-G12** | Feature Map / 清单逐条追溯未建 | 高（门禁） | `docs/testing` 无 AD-* Case；check-map 未登记 | A 出口被阻断 | Desktop **A0**（A 开发前） |

## 4. 与原 SYSTEM_GAPS / 黄金场景的关系

原 [SYSTEM_GAPS.md](../feature-map/SYSTEM_GAPS.md) 的 G01–G10 **不取消**。Desktop 改变的是**壳与交付顺序**，不是取消打通：

| 原缺口 | 在 Desktop 轴上的位置 |
|---|---|
| G01 MOM↔会话 | WP1 L1–L3；桌面侧通过运营入口 + 频道卡消费；**不**把 case 强行改成 Task（仍方案 A） |
| G04 审查需人工死胡同 | 仍 WP2；Desktop 映射「需人工处理」文案，待审批仅当等待原因是审批 |
| G08 重启续跑 | 对应 Desktop B2 / WP7；A 阶段 Mock 模拟检查点即可 |
| G03/G09 身份与外发 | Desktop C / WP6；企业身份前界面只展示模拟审批语义（DESIGN §4） |
| GS1 制造异常闭环 | **保留为跨模块黄金场景**；其 UI 入口逐步迁到 Desktop 运营区 + 工作区，而不是另起第三套页 |

## 5. 对现行计划的调整（已确认）

### 5.1 单一产品轴：组织级 AI 桌面

| 项 | 定案 |
|---|---|
| 产品 | **直接做**组织级 AI 桌面（V1.7），不保留「旧 MVP 壳 + Desktop」双轨 |
| 交付 | A 快速完整前端 → 每里程碑全前端可测 → B/C/D 按 [MOCK_REGISTRY.md](MOCK_REGISTRY.md) 替换 |
| 原 WP1/运营/内省 | **桌面内模块**与真实数据源；打通断言并入 AD-* / GS1，不另起一级导航 |
| 高风险文件 | 规则不变：改 orchestrator/store 等仍先方案后批准 |

### 5.2 运营桌面 D3 拆分（已按推荐口径确认）

| 卡 | 内容 | 归属 |
|---|---|---|
| **D3a** | 运营模块内二级：看板 / 任务（只读工作项投影）/ 资源（只读缺口，标仿真） | Desktop **A**；无桌面批准按钮 |
| **D3b / L4** | `an-des-mcp` 连接器 | 单独方案；不堵 D3a |

### 5.3 WP1 进度表纠偏

D0–D2 已完成；待做并入 Desktop：**D3a**、L0/L1/L3、L4（外置方案）。

### 5.4 近期执行顺序（已确认）

1. **A0**：特性清单逐条追溯 → Feature/Case；登记 AD-*；Mock 注册表开工填负责人。  
2. **A1 一次换壳**：首页三区 + S1/S2/S3；旧「会话|运营桌面」顶栏导航移除，入口重定向进桌面模块。  
3. **D3a** 与 A1 同里程碑或紧随小 PR。  
4. **A2–A5**：草稿/证据/设置/M1–M6 贯通；SMT-03 **纯 Mock 允许**。  
5. **B/C/D**：按 MOCK_REGISTRY 替换；原 WP1 L1–L3、GS1 在真实 MOM/频道能力就绪后于壳内验收。  

## 6. 拍板记录（2026-11-20 用户确认）

| # | 问题 | 决定 |
|---|---|---|
| 1 | 双轨？ | **不采用**。直接做组织级 AI 桌面 |
| 2 | D3a 口径 | **采用** §5.2（任务=案例工作项投影；资源=只读缺口；L4 外置） |
| 3 | A1 换壳 | **一次换壳**，旧路径重定向 |
| 4 | SMT-03 | **A 阶段允许纯 Mock**（`TASK-2026-018`）；B 起与真实 Task 对齐 |
| — | 代码 | **确认文档后**再动业务代码；Mock→真实替换计划见 [MOCK_REGISTRY.md](MOCK_REGISTRY.md) |

用户对计划正文的四项要求与既有写法对应关系：

| 要求 | 写在哪里 |
|---|---|
| 1 快速搭建完整前端 | [PLAN.md](PLAN.md) §1 首句与里程碑 A；[DESIGN.md](DESIGN.md) §1 交付 A |
| 2 每里程碑全前端可用，缺后端用 Mock；E2E=Feature Map+Playwright | PLAN §1 表「出口」列；DESIGN §1、§10；FEATURE_MAP 执行约定 |
| 3 Mock 开发与取代计划 | PLAN §2；**明细注册表** [MOCK_REGISTRY.md](MOCK_REGISTRY.md) |
| 4 回归严格满足设计特性清单 | PLAN §3；DESIGN §10；清单原文 [agentnexus_ai_desktop_design_checklist-2.md](agentnexus_ai_desktop_design_checklist-2.md) |
