#!/usr/bin/env bash
# World Adapter M1：远程 DES 合同 + 浏览器 + M0 全量回归。
# 用法：在仓库根目录执行 ./scripts/wa-m1-suite.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== RT-WADES (mom_world_des) =="
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture

echo "== Playwright：E2E-WADES / WAREAD / WAOBS =="
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
npx playwright test e2e/wa-des-remote.spec.ts
cd "$ROOT"

echo "== M0 回归（wa-m0-suite） =="
./scripts/wa-m0-suite.sh

echo "== wa-m1 suite green =="
