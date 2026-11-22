#!/usr/bin/env bash
# World Adapter M2：写路径 Playwright + 合同回归 + M0/M1 全量。
# 用法：在仓库根目录执行 ./scripts/wa-m2-suite.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== wa-m2-regression（RT-WADES 写/读 + RT-DES/DESC） =="
./scripts/wa-m2-regression.sh

echo "== Playwright：E2E-WAEXEC / WARECV / WADYN / WALOOP =="
cd web
CHROME="${PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH:-}"
if [[ -z "$CHROME" ]]; then
  if [[ -x /usr/local/bin/google-chrome ]]; then
    CHROME=/usr/local/bin/google-chrome
  elif [[ -x /usr/bin/chromium ]]; then
    CHROME=/usr/bin/chromium
  fi
fi
export PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH="${CHROME:-}"
npx playwright test e2e/wa-des-write.spec.ts
cd "$ROOT"

echo "== M1 回归（含 M0） =="
./scripts/wa-m1-suite.sh

echo "== wa-m2 suite green =="
