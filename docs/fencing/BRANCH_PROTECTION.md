# 阶段 0：master 分支保护

## 目标

第四层（CI）从「建议」变成「门」：不能直推 `master`，不能在必需检查未绿时合并。

## 当前限制

Cloud Agent 与默认 `GITHUB_TOKEN` **没有**仓库 `admin` 权限，无法代所有者写入保护规则。
仓库所有者请在本机执行：

```bash
./scripts/enable_branch_protection.sh
# 阶段 2 之后（bot 身份已就绪）：
APPROVALS=1 REQUIRE_CODEOWNERS=true ./scripts/enable_branch_protection.sh
```

或在 GitHub → Settings → Branches / Rulesets 中按下列勾选。

## 阶段 0 必选设置

1. Require a pull request before merging
2. Require approvals：**0**（单人仓库先可合并；阶段 2 改为 ≥1）
3. Require status checks to pass：`rust`、`web`、`python-websearch`、`python-tools`、`change-policy`、`security`
4. Require branches to be up to date
5. Do not allow bypassing the above settings（enforce admins）
6. 禁止 force push / 删除分支 / 直推 master

## 阶段 2 追加

- Require approvals：**1**
- Require review from Code Owners
- Dismiss stale pull request approvals when new commits are pushed
- 开发 Agent 用 bot 提 PR，人批准（见 [BOT_IDENTITY.md](BOT_IDENTITY.md)）

## 验收

```bash
# 应失败：直推 master（无 admin 绕过时）
git push origin HEAD:master

# 保护详情（需有读权限）
gh api repos/Grant-Huang/multi-agent-conv-mvp/branches/master/protection \
  --jq '{enforce_admins, contexts: .required_status_checks.contexts, approvals: .required_pull_request_reviews.required_approving_review_count}'
```

`protected: true` 但 `required_status_checks.enforcement_level=off` / 空 contexts **不算**完成。
