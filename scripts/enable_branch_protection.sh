#!/usr/bin/env bash
# 开启 master 分支保护（围栏阶段 0）。
#
# 本仓库的 Cloud Agent / 普通 GITHUB_TOKEN 通常没有 admin 权限，无法代你执行。
# 请仓库所有者在本机用有 admin 权限的 gh 登录后运行：
#
#   ./scripts/enable_branch_protection.sh
#
# 阶段 0 默认：必须 PR + 必需 CI，批准数 = 0（单人仓库可合并）。
# 阶段 2（bot 身份就绪）后再把 required_approving_review_count 改为 1，
# 并打开 require_code_owner_reviews。
#
# 参考：docs/fencing/BRANCH_PROTECTION.md、docs/CI_GATES.md 第 3 节。

set -euo pipefail

OWNER="${OWNER:-Grant-Huang}"
REPO="${REPO:-multi-agent-conv-mvp}"
BRANCH="${BRANCH:-master}"
# 阶段 0 = 0；阶段 2 起改为 1
APPROVALS="${APPROVALS:-0}"
REQUIRE_CODEOWNERS="${REQUIRE_CODEOWNERS:-false}"

if ! command -v gh >/dev/null; then
  echo "需要 GitHub CLI (gh)" >&2
  exit 2
fi

echo "对 ${OWNER}/${REPO} 分支 ${BRANCH} 应用保护规则（approvals=${APPROVALS}, codeowners=${REQUIRE_CODEOWNERS}）…"

gh api \
  --method PUT \
  -H "Accept: application/vnd.github+json" \
  "/repos/${OWNER}/${REPO}/branches/${BRANCH}/protection" \
  --input - <<EOF
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "rust",
      "web",
      "python-websearch",
      "python-tools",
      "change-policy",
      "security"
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": ${REQUIRE_CODEOWNERS},
    "required_approving_review_count": ${APPROVALS},
    "require_last_push_approval": false
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "block_creations": false,
  "required_conversation_resolution": false,
  "lock_branch": false,
  "allow_fork_syncing": false
}
EOF

echo "完成。请用下列命令核对："
echo "  gh api /repos/${OWNER}/${REPO}/branches/${BRANCH}/protection --jq '{enforce_admins,required_status_checks,required_pull_request_reviews}'"
