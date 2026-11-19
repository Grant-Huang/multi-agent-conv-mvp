# 围栏体系（本仓库落地）

上位来源：`docs/AUTONOMOUS_DEV_ACTION_GUIDE.md`（由《AI 内服提升中 Agent 围栏建设建议…》整理）。
本目录放**可执行的落地说明**，不重复指南全文。

| 文件 | 内容 |
|---|---|
| [BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) | 阶段 0：master 分支保护（脚本与核对） |
| [BOT_IDENTITY.md](BOT_IDENTITY.md) | 阶段 2：开发 Agent 独立身份与人批准 |
| [SHADOW_REVIEW.md](SHADOW_REVIEW.md) | 阶段 2：合并前 AI 审查影子模式 |
| [EVAL_LOOP.md](EVAL_LOOP.md) | 阶段 3：Bug/事故 → 测试/Eval 闭环 |
| [PRODUCT_FENCES.md](PRODUCT_FENCES.md) | 阶段 4：工程围栏 → 平台能力映射 |
| [REVIEW_VERDICT_DESIGN.md](REVIEW_VERDICT_DESIGN.md) | 阶段 4：审查进待人工的通过/退回设计（未改 orchestrator） |

原则：**指导不是安全边界**。`AGENTS.md` 里的禁止事项必须靠 CI、分支保护、权限和运行时强制。
