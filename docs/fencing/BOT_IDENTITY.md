# 阶段 2：开发 Agent 独立身份

## 问题

单人仓库里，PR 作者 = 仓库所有者时，GitHub **不允许作者批准自己的 PR**。
若开启「需要 1 个批准」或 CODEOWNERS，每个 PR 都无法合并，除非管理员绕过——绕过又使门禁失效。

## 定了什么

1. **AI 开发会话使用独立 bot / GitHub App 身份**创建分支与 PR。
2. **人（CODEOWNERS）作为唯一合并批准人**。
3. 作者（bot）不能算作独立审批人；Reviewer Agent 评论不能替代人工批准。

## 推荐做法（GitHub App）

1. 创建 GitHub App（或 fine-grained PAT 的 machine user），权限：`contents: write`、`pull_requests: write`、`checks: read`。
2. 安装到本仓库；Cloud Agent / Cursor / Claude Code 的推送与 `gh pr create` 使用该身份。
3. CODEOWNERS 保持 `@Grant-Huang`（或后续模块负责人）。
4. 运行：`APPROVALS=1 REQUIRE_CODEOWNERS=true ./scripts/enable_branch_protection.sh`

## 验收

- bot 打开的 PR：`author` ≠ 批准人。
- 无人批准时不能合并。
- 人批准且 CI 绿后可以合并。

## 未做

本 PR **不**创建真实 GitHub App（需所有者在 GitHub UI 操作并保管密钥）。
密钥不得写入仓库。
