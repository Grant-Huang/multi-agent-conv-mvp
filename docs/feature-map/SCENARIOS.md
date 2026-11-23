# 端到端场景（跨功能）

功能文件（`features/`）按单个功能描述；真实用户走的是**串起多个功能的旅程**。本文件把旅程拆成可执行的端到端场景，并标出每个场景**现在有没有自动化**。用它决定下一批 E2E 先写什么。

图例：✅ 已有浏览器用例走通 · ◐ 只有后端/组件测试 · ✗ 没有任何自动化 · ⚠ 有已知产品缺口（见 [SYSTEM_GAPS.md](SYSTEM_GAPS.md)）。

> “建议用例”只是命名建议，**尚未加入 `docs/testing/CASES.md`**。按 AGENTS.md，新增用例要先在 CASES 定稿并见到业务断言 Red，再改实现。

## 总表

| # | 场景 | 串起的功能 | 覆盖 | 建议用例 |
|---|---|---|---|---|
| S01 | 第一次使用：看状态 → 提问 → 委派写文件 → 下载 | conversation → tasks → artifacts | ✅（分散在 CONV/FILE/TICKER） | — |
| S02 | 高风险任务：待批准 → 看说明 → 批准 → 执行 → 审查 → 完成 | approval → assignment → tasks | ✅ APPROVE/APPROVEINFO | — |
| S03 | 审查不通过 → 自动返工 → 通过 | tasks → reviewer → 返工 | ◐ 后端（`crates/server/tests/review_rework.rs`） | E2E-REWORK |
| S04 | 审查结论“需人工” → 用户处理 | tasks → reviewer → approval | ✅ 经典 E2E-REV；日常入口通过 `E2E-HOMEREV` + 拒绝/返工 `E2E-HOMEREJ`（终态交付回发起会话） | E2E-HOMEREV、E2E-HOMEREJ |
| S05 | 没有写代码助手 → 对话接入 → 改代码 → 看 diff → 审查 | agents → conversation → tasks → git-evidence | ✅ E2E-CODEFLOW（WP3；harness 临时 git workdir + mock ACP） | E2E-CODEFLOW |
| S06 | 任务进行中补充/取消要求 | conversation(follow_up) → tasks | ◐ RT-CONV-01/02，浏览器 ✗ | E2E-SUPPLEMENT |
| S07 | 联网研究报告（检索 + 章节并行） | conversation → reports → tools → artifacts | ◐ RT-REP；浏览器 ✗（受控模型可模拟） | E2E-REPORT |
| S08 | 任务失败/无结果 → 诊断 → 路由日志 → “派错了”反馈 | tasks → observability → feedback | 失败卡片 ✅、诊断 ✅、**反馈 UI ✅**（E2E-FEEDBACK）；反馈→评测 ✗ ⚠（G06 / WP5） | E2E-FEEDBACK |
| S09 | 助手连不上 → 排查 → 诊断包 → 请帮手（经外发脱敏） | agents → egress-dlp | ✅ AGDIAG/AGHELP（模拟 DLP） | — |
| S10 | 第二台电脑接入 → 配对 → 执行组主力/备份接手 | nodes → assignment → tasks | ◐ 后端；浏览器 ✗（本卡未交付） | E2E-NODES（测试节点） |
| S11 | 开启外发检查 → 联网工具/云端 LLM 请求被脱敏或拒绝 | egress-dlp → tools/backends | ◐ RT-EGR（替身）；整链浏览器 ✗ | E2E-EGRESS |
| S12 | 外部 agent 准入：S1→S7 → 激活 → 接任务 → 撤权 | admission → registry → tasks | ◐ HTTP；无界面 ⚠（G03） | HTTP-ADM 已有；界面待产品定 |
| S13 | 服务重启：进行中任务、待批准任务、会话 | store → orchestrator → UI | ✗（G08：运行中任务不能续跑） | E2E-RESTART |
| S14 | MOM：S1 审批 → 下发 → 核验 → 关闭；S2 客诉 | mom（独立体系） | ✅ MOM/COMPLAINT/MOMDYNAMIC | — |
| S15 | **从聊天里处理一个 MOM 案例**（对话 → 案例 → 审批 → 回写） | conversation ↔ mom | ✗ ⚠（G01，根本无此链路） | 先定产品方案 |
| S16 | 窄屏全流程：抽屉 → 新会话 → 发送 → 审批 | navigation → conversation → approval | ◐ MOBILE 只到“选择会话” | E2E-MOBILEFLOW |

## 逐场景说明（只写覆盖不足的）

### S03 审查返工
- **步骤**：要求写文档 → Reviewer 判 fail（受控模型返回 `review_verdict: fail` 一次再 pass）→ 任务回到“排队中”→ 第 2 轮 → 完成。
- **断言**：`task.review` 最终 pass；时间线含“第 1 轮返工”；“执行者”节显示返工轮次 = 1；两轮产物以最后一轮为准。
- **驱动**：在 `web/e2e/harness.mjs` 的 `review_verdict` 分支按消息计数返回 fail→pass（只改测试替身，不改业务）。

### S04 审查结论“需人工”（⚠ 先确认产品行为）
- **现状**：`orchestrator.rs` 中审查失败（`REVIEW_FAILED`）或 Reviewer 判 `needs_human` 时任务置为 `NEEDS_HUMAN` 且**带 result**；界面状态仍显示“待批准”，但“批准执行”按钮只在 `!task.result` 时出现；`approve()` 也只接受“尚未执行”的任务。结果是一个**没有出口**的状态（DESIGN §15 限制 #4）。
- **E2E 做法**：先让产品决定行为（增加“人工判定通过/不通过”？还是重新发起？），再写用例。在此之前只能写“当前行为基线”：状态文案、没有批准按钮、可取消。

### S05 改代码全链路（最值得补）
- **步骤**：“助手”页签接入 `claude`（E2E 环境已有指向 `an-mock-acp` 的包装脚本）→ 发“改代码验证：…”（受控路由到 coding）→ 任务执行 → 时间线有工具调用/权限请求 → git diff 作为证据 → Reviewer → COMPLETED。
- **断言**：执行者为 `claude`；证据里有 diff；工作目录是 git 仓库且用户暂存区未变；审查结论在时间线。
- **覆盖**：✅ `web/e2e/codeflow.spec.ts`（E2E-CODEFLOW）；WP0-2 夹具：`harness.mjs` 临时 git workdir 写入 `[agents.coding].workdir`，`an-mock-acp` 写 `hello.txt`。

### S06 补充要求
- **步骤**：发慢任务 → 在执行中再发“补充：……”（受控模型返回 `follow_up: supplement`）→ 旧任务 CANCELLED，新任务出现且目标合并。
- **断言**：路由失败（受控 500）时旧任务不变；成功时只有一个活跃任务。
- **本卡（WP3）**：未交付（仍仅 RT-CONV）。

### S08 反馈闭环
- **步骤**：展开任务 → “派错了” → 选动作/角色 → “一句说明” → “保存”→ 提示“已记下” → `POST /api/feedback` 持久化。
- **覆盖**：✅ 写入路径 `E2E-FEEDBACK`（`web/e2e/codeflow.spec.ts`）。
- **缺口**：反馈之后**没有任何消费者**（不进评测集、不影响路由）；用例只能断言写入，不能断言“系统变好”（G06 / WP5）。

### S13 重启
- **步骤**：创建待批准任务 + 一个运行中任务 → 重启 `agentnexus`（同一数据库）→ 刷新页面。
- **断言（按代码现状）**：运行中任务变 FAILED（可读原因）；待批准任务重启后仍可批准，执行上下文来自 `tasks.context` 列（DESIGN §15 #2 的“只在内存”描述已过时，见 G08）。
- harness 目前不支持重启同一数据库，需要扩展 `harness.mjs`。

### S15 聊天 ↔ MOM
- 当前代码里对话/路由/编排器没有任何 MOM 入口；MOM 沙盘通过左栏链接进入独立页面，使用自己的 Engine、Master、审批和事件体系。此场景无法写 E2E，必须先有产品方案（见 G01）。

## 写 E2E 时的通用约定

- 数据准备走受控模型触发词（见 [README](README.md#驱动工具速查harness)）；新增触发词要写进 harness 并在 CASES 注明。
- 断言必须带**业务判据**（状态、产物字节、事件序列），不能只断言“按钮存在”；环境失败不算 Red。
- 一个场景一个测试文件段落，用例名 `E2E-<大写字母>`（`check-map.mjs` 用 `E2E-[A-Z]+` 匹配，**含数字或连字符的名字不会被识别**，如 `E2E-MOBILEFLOW` 应写成 `E2E-MOBILEFLOW`）。
- 新用例同步 `CASES.md`、`features.json` 的 `cases` 和 `docs/feature-map/COVERAGE.md`。
