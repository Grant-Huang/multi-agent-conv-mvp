#!/usr/bin/env bash
# World Adapter M3：生产读 Playwright + M0～M2 全量回归。
# 用法：在仓库根目录执行 ./scripts/wa-m3-suite.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== Playwright：E2E-WAPROD / WAPISO / WAPREAD / WAPDELAY =="
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
npx playwright test e2e/wa-prod-read.spec.ts
cd "$ROOT"

echo "== M2 回归（含 M0/M1） =="
./scripts/wa-m2-suite.sh

echo "== wa-m3 suite green =="
